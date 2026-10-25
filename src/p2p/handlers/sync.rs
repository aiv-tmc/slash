//! Request serving and response-driven sync: capped header/block ranges,
//! onion-key registration with challenge attestation, request-channel
//! throttling, and the tip-comparison pipeline that drives header-first
//! synchronization.

use super::super::node::Node;
use super::super::util::peer_id_ed25519_key;
use super::super::wire::{Req, Resp};
use libp2p::request_response;
use libp2p::PeerId;

impl Node {
    /// Request-response events: inbound requests are served under the
    /// per-peer request bucket, inbound responses drive the sync pipeline
    /// and attestation verification, and failures record strikes.
    pub(super) async fn handle_req_resp(
        &mut self,
        event: request_response::Event<Req, Resp>,
    ) -> anyhow::Result<()> {
        use request_response::Event;
        match event {
            Event::Message { peer, message } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let response = self.serve_request(peer, request).await;
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .req_resp
                        .send_response(channel, response);
                }
                request_response::Message::Response { response, .. } => {
                    self.handle_response(peer, response).await;
                }
            },
            Event::OutboundFailure { peer, .. } => {
                self.record_strike(peer);
            }
            Event::InboundFailure { peer, .. } => {
                self.record_strike(peer);
            }
            Event::ResponseSent { .. } => {}
        }
        Ok(())
    }

    /// Serve one inbound request from local state, or refuse it under
    /// the request bucket. The request channel is the most expensive
    /// per-message surface of the node — a single GetBlocks serializes
    /// up to 64 blocks — so every inbound request spends tokens priced
    /// by serving cost. A refused request gets an empty Ack; a peer that
    /// keeps pressing against the bucket past three consecutive refusals
    /// records a strike, while one admitted request clears the count, so
    /// honest sync is never punished and only sustained draining climbs
    /// the ban ladder.
    async fn serve_request(&mut self, peer: PeerId, req: Req) -> Resp {
        use super::super::limits::*;
        let cost = match &req {
            Req::GetBlock(..) => REQ_COST_BLOCK,
            Req::GetBlocks(..) => REQ_COST_BLOCKS,
            Req::GetTip => REQ_COST_TIP,
            Req::GetHeaders(..) => REQ_COST_HEADERS,
            Req::GetOnionKey | Req::GetOnionKeyV2(..) => REQ_COST_ONION_KEY,
            Req::ForwardOnion(..) => REQ_COST_FORWARD_ONION,
        };
        // Every refused request is answered with an empty Ack; the peer
        // learns nothing about the local state and pays nothing. A
        // refusal past the consecutive threshold additionally records a
        // strike, so the ban ladder engages only on sustained draining.
        let refusal = {
            let entry = self.buckets.entry(peer).or_default();
            if entry.take_req(cost) {
                entry.clear_req_refusal();
                None
            } else if entry.note_req_refusal() {
                Some(true)
            } else {
                Some(false)
            }
        };
        if let Some(past_threshold) = refusal {
            if past_threshold {
                self.record_strike(peer);
            }
            return Resp::Ack;
        }
        match req {
            Req::GetBlock(height) => {
                let idx = height.wrapping_sub(self.chain.base_height) as usize;
                let block = self
                    .chain
                    .blocks
                    .get(idx)
                    .filter(|_| height >= self.chain.base_height && idx < self.chain.blocks.len())
                    .cloned();
                Resp::Block(block)
            }
            Req::GetBlocks(from, to) => {
                let mut out = Vec::new();
                for h in from..to {
                    let idx = h.wrapping_sub(self.chain.base_height) as usize;
                    if h < self.chain.base_height || idx >= self.chain.blocks.len() {
                        break;
                    }
                    out.push(self.chain.blocks[idx].clone());
                    if out.len() >= 64 {
                        break;
                    }
                }
                Resp::Blocks(out)
            }
            Req::GetTip => Resp::Tip(self.chain.tip_height(), self.chain.tip_hash()),
            Req::GetHeaders(from, to) => {
                let mut out = Vec::new();
                for h in from..to {
                    let idx = h.wrapping_sub(self.chain.base_height) as usize;
                    if h < self.chain.base_height || idx >= self.chain.blocks.len() {
                        break;
                    }
                    out.push(crate::chain::BlockHeader::from_block(
                        &self.chain.blocks[idx],
                    ));
                    if out.len() >= 256 {
                        break;
                    }
                }
                Resp::Headers(out)
            }
            Req::GetOnionKey => {
                // The legacy unauthenticated answer is retained as a
                // fallback for peers that predate the attestation scheme.
                // The requester's identity proves nothing here, so the
                // routing table is left untouched: only keys verified on
                // the response side ever populate it.
                Resp::OnionKey(self.onion_public())
            }
            Req::GetOnionKeyV2(nonce) => {
                // The challenge nonce binds the signed answer to this
                // exchange: the signature covers nonce||key, so a replayed
                // attestation from an earlier session cannot be re-bound
                // to a fresh nonce without the identity secret.
                let key = self.onion_public();
                let mut msg = nonce.to_vec();
                msg.extend_from_slice(&key);
                let digest = blake3::hash(&msg);
                let sig = crate::crypto::sign(&self.node_secret(), digest.as_bytes());
                Resp::OnionKeyV2(nonce, key, sig)
            }
            Req::ForwardOnion(bytes) => {
                self.handle_onion(peer, &bytes).await;
                Resp::Ack
            }
        }
    }

    /// Handle a response to one of our requests: header sync, block sync,
    /// and onion-key attestation verification. Only a response that
    /// answers the exact challenge issued at connection time, carries a
    /// valid identity signature, and advertises a sound curve point enters
    /// the routing table, which closes the relay key-substitution and
    /// attestation-replay holes.
    async fn handle_response(&mut self, peer: PeerId, response: Resp) {
        match response {
            Resp::Tip(height, _hash) => {
                // A tip claim beyond the bounded lookahead is ignored:
                // honest peers never advertise more than a bounded window
                // ahead, and chasing an absurd claim would start a header
                // fetch the claimant must prove, block by block, at
                // RandomX cost.
                let within_bound =
                    height <= self.chain.tip_height() + super::super::limits::SYNC_MAX_TIP_AHEAD;
                if height > self.chain.tip_height() && !self.syncing && within_bound {
                    self.syncing = true;
                    let _ = self.swarm.behaviour_mut().req_resp.send_request(
                        &peer,
                        Req::GetHeaders(self.chain.tip_height() + 1, height + 1),
                    );
                }
            }
            Resp::Headers(headers) => {
                if self.chain.validate_headers_batch(&headers) {
                    let from = headers.first().map(|h| h.height).unwrap_or(0);
                    let to = headers.last().map(|h| h.height + 1).unwrap_or(0);
                    if to > from {
                        // Remember the end of the header window: block
                        // responses keep requesting the next chunk until
                        // the window is exhausted.
                        self.sync_upto = Some(to);
                        let _ = self
                            .swarm
                            .behaviour_mut()
                            .req_resp
                            .send_request(&peer, Req::GetBlocks(from, to));
                    }
                } else {
                    self.record_strike(peer);
                    self.syncing = false;
                }
            }
            Resp::Blocks(blocks) => {
                let mut failed = false;
                let mut last_height = 0u64;
                let mut count = 0usize;
                for block in blocks {
                    last_height = block.height;
                    count += 1;
                    if self.chain.process_block(block).is_err() {
                        failed = true;
                        break;
                    }
                }
                if failed {
                    self.record_strike(peer);
                    self.syncing = false;
                    self.sync_upto = None;
                } else if count == super::super::limits::SYNC_BLOCKS_PER_RESPONSE
                    && self.sync_upto.is_some_and(|to| last_height + 1 < to)
                {
                    // The window is not exhausted and the peer served a
                    // full chunk: request the next one immediately
                    // instead of waiting for the next tip poll, so
                    // catch-up advances the whole window per round.
                    let to = self.sync_upto.unwrap_or(last_height + 1);
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .req_resp
                        .send_request(&peer, Req::GetBlocks(last_height + 1, to));
                } else {
                    self.syncing = false;
                    self.sync_upto = None;
                }
            }
            Resp::Block(_) => {}
            Resp::OnionKey(key) => {
                // Legacy unauthenticated key: usable only as a fallback
                // when no attested key exists for the peer. A low-order
                // key would make every envelope routed through it
                // readable by anyone, so the probe is applied here too;
                // an unusable key is skipped without touching reputation,
                // because the legacy path carries no attestation at all.
                let probe = x25519_dalek::StaticSecret::from(self.onion_secret());
                if crate::crypto::x25519_shared(&probe, &key).is_some() {
                    self.onion_keys.entry(peer).or_insert(key);
                    self.onion_key_to_peer.entry(key).or_insert(peer);
                }
            }
            Resp::OnionKeyV2(nonce, key, sig) => {
                // The response must answer the exact challenge issued at
                // connection time. Removing the entry also consumes it:
                // each challenge is single-use, so a peer cannot answer
                // one challenge twice or relay a captured attestation to
                // a different session.
                match self.onion_challenges.remove(&peer) {
                    Some(expected) if expected == nonce => {}
                    _ => return,
                }
                // Attested key: verify the Ed25519 signature over the
                // challenge nonce and key against the peer's identity key.
                let mut msg = nonce.to_vec();
                msg.extend_from_slice(&key);
                let digest = blake3::hash(&msg);
                let ok = peer_id_ed25519_key(&peer)
                    .is_some_and(|pk| crate::crypto::verify(&pk, digest.as_bytes(), &sig));
                if !ok {
                    return;
                }
                // A valid signature proves possession of the identity
                // secret, but a low-order X25519 key is still soundless:
                // every Diffie-Hellman output computed against it is the
                // all-zero string, so any envelope built through this
                // relay would be decryptable by anyone. The key is
                // probed with a Diffie-Hellman exchange against this
                // node's own onion secret before it enters the table, and
                // a peer advertising such a key is struck: the
                // advertisement is semantically invalid, not merely
                // unlucky.
                let probe = x25519_dalek::StaticSecret::from(self.onion_secret());
                if crate::crypto::x25519_shared(&probe, &key).is_none() {
                    self.record_strike(peer);
                    return;
                }
                self.onion_keys.insert(peer, key);
                self.onion_key_to_peer.insert(key, peer);
            }
            // Acknowledgement to our own forwarded onion: nothing to do.
            Resp::Ack => {}
        }
    }
}
