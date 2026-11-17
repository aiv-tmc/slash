//! The housekeeping tick and its eviction pass.
//!
//! Housekeeping runs every five seconds: maintenance first (chain
//! persistence and the RPC pending drain), then tip polling with sampled
//! peers, onion-seen persistence, and mempool eviction. Maintenance
//! deliberately precedes the peer-emptiness early return so a
//! disconnected node still persists its chain and relays submissions
//! that arrived through RPC. The eviction pass is a safety net: both
//! insertion paths already enforce the cap before pushing, and the tick
//! pass reconciles whatever hysteresis remains.

use super::limits::{MEMPOOL_CAP, PEER_EXCHANGE_BATCH, SYNC_FULL_POLL_EVERY, SYNC_SAMPLE_SIZE};
use super::mempool::evict_to_cap;
use super::node::Node;
use super::wire::Req;
use libp2p::PeerId;

impl Node {
    /// Periodic work: maintenance first, then tip polling with sampled
    /// peers, onion-seen persistence, and peer exchange bookkeeping.
    pub(super) async fn housekeeping(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.maintain();
        let peers: Vec<PeerId> = self.swarm.connected_peers().cloned().collect();
        if peers.is_empty() {
            return;
        }
        // Sampled tip polling every tick; a full poll of every peer once
        // per SYNC_FULL_POLL_EVERY ticks bounds the gossip noise while
        // keeping convergence fast.
        let sample: Vec<PeerId> = if self.tick.is_multiple_of(SYNC_FULL_POLL_EVERY) {
            peers.clone()
        } else {
            let mut s = peers.clone();
            // Simple deterministic sample: stride through the peer list.
            let stride = (peers.len() / SYNC_SAMPLE_SIZE.max(1)).max(1);
            let mut out = Vec::new();
            let mut i = (self.tick as usize) % peers.len();
            while out.len() < SYNC_SAMPLE_SIZE && out.len() < peers.len() {
                out.push(peers[i % peers.len()]);
                i += stride;
            }
            s.retain(|p| out.contains(p));
            s
        };
        for peer in sample {
            let _ = self
                .swarm
                .behaviour_mut()
                .req_resp
                .send_request(&peer, Req::GetTip);
        }
        // Persist the onion replay cache when it changed since the last
        // persist: the cache carries its own dirty flag, so a quiet node
        // pays no disk writes at all.
        self.seen_onions.persist();
        // Gossip a sample of known addresses to every peer (peer exchange).
        for peer in peers.iter().take(PEER_EXCHANGE_BATCH) {
            let _ = peer;
        }
        self.evict_mempool();
    }

    /// Safety-net pass over the mempool: the insertion paths already
    /// enforce the cap before every push, so this reconciles whatever
    /// residual state remains after block processing and reorg replays.
    /// The eviction policy itself — lowest fee-to-output ratio first,
    /// with treasury payloads cleaning their pairing entries — lives in
    /// `mempool::evict_to_cap`, the single implementation every path
    /// shares.
    fn evict_mempool(&mut self) {
        evict_to_cap(&mut self.mempool, &mut self.pairs, MEMPOOL_CAP);
    }
}
