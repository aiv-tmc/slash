//! Resource limits of the network layer.
//!
//! Every constant here is a DoS boundary: frame caps bound allocations,
//! token buckets bound per-peer message rates, connection caps bound
//! inbound floods, strike tables bound ban bookkeeping, and the replay
//! cache bounds the onion-seen set. None of them is a tuning knob.
//! Constants used from sibling submodules are crate-visible; the two
//! limits other crates read (mempool capacity, pruned retention) are
//! public.

/// Maximum serialized request-response frame; larger frames are rejected
/// before deserialization so a malicious peer cannot force giant
/// allocations.
pub(crate) const MAX_FRAME: usize = 10 * 1024 * 1024;

/// Number of blocks retained in pruned mode.
pub const PRUNED_KEEP_BLOCKS: u64 = 10_000;

/// Mempool capacity in transactions. Admission evicts the lowest-fee
/// transaction first, so the pool always keeps the most valuable backlog.
pub const MEMPOOL_CAP: usize = 10_000;

/// Maximum open inbound connections node-wide; excess inbound dials are
/// refused so a connection flood cannot starve outbound slots.
pub(crate) const MAX_INBOUND_CONNECTIONS: usize = 64;

/// Maximum connections accepted from one IPv4 /24 or IPv6 /64 subnet. A
/// Sybil cluster behind one subnet key shares one budget, which is the
/// primary defense against cheap multi-identity floods.
pub(crate) const MAX_CONNECTIONS_PER_SUBNET: usize = 8;

/// Peer misbehavior strikes before a ban activates.
pub(crate) const PEER_BAN_STRIKES: u32 = 3;
/// Base ban duration in seconds at the strike threshold.
pub(crate) const PEER_BAN_BASE_SECS: u64 = 60;
/// Maximum ban duration in seconds; the exponential backoff saturates here.
pub(crate) const PEER_BAN_MAX_SECS: u64 = 3600;

/// Tip polling: how many peers are sampled per sync tick, and how many
/// ticks pass between full polls of every peer.
pub(crate) const SYNC_SAMPLE_SIZE: usize = 8;
pub(crate) const SYNC_FULL_POLL_EVERY: u64 = 12;

/// Peer exchange: batch size and the cap of the known-addresses table.
pub(crate) const PEER_EXCHANGE_BATCH: usize = 8;
pub(crate) const KNOWN_ADDRS_CAP: usize = 4096;

/// Onion replay cache capacity. Identifiers evict oldest-first (FIFO), and
/// the cache persists to disk so a restart cannot reopen the replay
/// window.
pub(crate) const ONION_REPLAY_CACHE: usize = 10_000;

/// Refill rates and burst capacities of the per-peer token buckets. One
/// bucket per message class stops a single peer from flooding any single
/// subsystem while leaving other classes usable.
pub(crate) const TX_BUCKET_REFILL_PER_SEC: f64 = 2.0;
pub(crate) const TX_BUCKET_BURST: f64 = 8.0;
pub(crate) const BLOCK_BUCKET_REFILL_PER_SEC: f64 = 0.2;
pub(crate) const BLOCK_BUCKET_BURST: f64 = 2.0;
pub(crate) const ONION_BUCKET_REFILL_PER_SEC: f64 = 0.5;
pub(crate) const ONION_BUCKET_BURST: f64 = 4.0;

/// The request-class bucket: refills at one token per second up to a
/// burst of eight. Request messages are strictly more expensive per unit
/// than gossip — one GetBlocks serializes up to eight blocks — so the
/// channel gets its own bucket sized to the honest sync profile: headers
/// and tip polls are cheap and frequent, full block ranges are rare.
pub(crate) const REQ_BUCKET_REFILL_PER_SEC: f64 = 1.0;
pub(crate) const REQ_BUCKET_BURST: f64 = 8.0;

/// Per-request token costs inside the request bucket. The cost mirrors
/// the serving work: a tip answer is one lookup, a single block is one
/// serialization of up to 1 MiB, a block range is up to eight of those,
/// and a forwarded onion pays the envelope deserialization plus the peel.
pub(crate) const REQ_COST_TIP: f64 = 1.0;
pub(crate) const REQ_COST_BLOCK: f64 = 2.0;
pub(crate) const REQ_COST_BLOCKS: f64 = 6.0;
pub(crate) const REQ_COST_HEADERS: f64 = 1.0;
pub(crate) const REQ_COST_ONION_KEY: f64 = 1.0;
pub(crate) const REQ_COST_FORWARD_ONION: f64 = 4.0;

/// Serving bound for one onion envelope, checked before the
/// allocation-heavy deserialization. An honest envelope is a few KiB per
/// layer — the fixed 1024-byte layer bodies plus the 2048-byte inner
/// transaction — so even a deliberately long path stays far below this
/// bound; a frame beyond it cannot be a valid onion and is refused at a
/// length comparison instead of a 10 MiB deserialize. The bound is local
/// admission policy like MAX_FRAME, applied after the request bucket has
/// priced the frame; consensus never sees these bytes.
pub(crate) const MAX_ONION_ENVELOPE: usize = 1024 * 1024;

/// Parked-block memory price: every 256 KiB of block bytes a peer parks
/// in a retention pool costs one block-bucket token. A maximum-size one
/// MiB block costs four tokens — more than the whole burst — so a single
/// parked block already empties the bucket and forces a full refill
/// cycle before the peer's next block message is admitted.
const BLOCK_BYTES_PER_TOKEN: f64 = 256.0 * 1024.0;

/// Consecutive over-limit requests before one strike is recorded. A
/// single refused request is ordinary congestion and costs nothing; a
/// peer that keeps pressing against the bucket after the refusals began
/// is draining the node on purpose, which is what the strike ladder is
/// for. The counter resets on the next admitted request.
pub(crate) const REQ_REFUSAL_STRIKE_THRESHOLD: u8 = 3;

/// How far ahead of the local tip an advertised tip is trusted enough to
/// start a header sync. Honest peers advertise at most a bounded window
/// ahead; claims beyond the bound are ignored until the chain approaches
/// them.
pub(crate) const SYNC_MAX_TIP_AHEAD: u64 = 20_160;

/// The serving cap for one GetBlocks response. Eight maximum-size blocks
/// fit comfortably inside the request-response frame limit, while the sync
/// loop still requests the next chunk immediately after a full one, so this
/// constant doubles as the loop's chunk detector.
pub(crate) const SYNC_BLOCKS_PER_RESPONSE: usize = 8;

/// A token bucket for one message class of one peer. Tokens refill
/// linearly up to a burst cap; a take succeeds only when enough tokens
/// are available and consumes them immediately. Every bucket starts with
/// one full token so the first message a freshly connected peer sends is
/// admitted at once: connection startup is never itself a punishable
/// event, and a rate-limit refusal stays an honest drop rather than the
/// first entry in a strike record.
pub(crate) struct RateBucket {
    tokens: f64,
    last: std::time::Instant,
}

impl RateBucket {
    fn new() -> Self {
        Self {
            tokens: 1.0,
            last: std::time::Instant::now(),
        }
    }

    /// Try to consume `cost` tokens, refilling linearly since the last
    /// take. Returns true when the take succeeded; the bucket never goes
    /// negative, so a refused flood costs the caller nothing.
    fn take(&mut self, cost: f64, refill_per_sec: f64, burst: f64) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * refill_per_sec).min(burst);
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Consume `cost` tokens unconditionally, clamping at zero. Unlike
    /// take, the charge always applies: whenever the debt exceeds the
    /// balance the bucket ends empty, so a peer that parks more bytes
    /// than its budget covers loses its entire remaining allowance and
    /// waits out a full refill cycle before any further block message
    /// is admitted. The refill runs first, so the charge always prices
    /// against the freshly refilled balance.
    fn charge(&mut self, cost: f64, refill_per_sec: f64, burst: f64) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * refill_per_sec).min(burst);
        self.tokens = (self.tokens - cost).max(0.0);
    }
}

impl Default for RateBucket {
    fn default() -> Self {
        Self::new()
    }
}

/// The four per-peer token buckets — transactions, blocks, onion relays,
/// and requests — plus the consecutive request-refusal counter the
/// serving side reads. A peer flooding a single class is throttled in
/// that class alone. Fields are crate-visible because the handlers
/// consume them directly.
#[derive(Default)]
pub(crate) struct PeerBuckets {
    pub(crate) tx: RateBucket,
    pub(crate) block: RateBucket,
    pub(crate) onion: RateBucket,
    pub(crate) req: RateBucket,
    /// Consecutive request-bucket refusals. Sustained over-limit
    /// requesting — not a single honest burst — is what earns a strike,
    /// so the count lives beside the bucket that produces it.
    req_refusals: u8,
}

impl PeerBuckets {
    /// Admit one transaction message under the transaction bucket.
    pub(crate) fn take_tx(&mut self) -> bool {
        self.tx.take(1.0, TX_BUCKET_REFILL_PER_SEC, TX_BUCKET_BURST)
    }

    /// Admit one block message under the block bucket.
    pub(crate) fn take_block(&mut self) -> bool {
        self.block
            .take(1.0, BLOCK_BUCKET_REFILL_PER_SEC, BLOCK_BUCKET_BURST)
    }

    /// Admit one onion relay under the onion bucket.
    pub(crate) fn take_onion(&mut self) -> bool {
        self.onion
            .take(1.0, ONION_BUCKET_REFILL_PER_SEC, ONION_BUCKET_BURST)
    }

    /// Charge the block bucket for memory parked on the node's behalf.
    /// The ingress path reports the serialized size of every block it
    /// parks in the side-fork or orphan pool for this peer, and each
    /// 256 KiB costs one token: flooding the retention pools now starves
    /// the flooder's own gossip budget, because every parked block
    /// zeroes the bucket and blocks the peer's other messages behind a
    /// full refill cycle.
    pub(crate) fn charge_block_bytes(&mut self, bytes: u64) {
        let cost = bytes as f64 / BLOCK_BYTES_PER_TOKEN;
        self.block
            .charge(cost, BLOCK_BUCKET_REFILL_PER_SEC, BLOCK_BUCKET_BURST);
    }

    /// Admit one request message under the request bucket, spending the
    /// given cost.
    pub(crate) fn take_req(&mut self, cost: f64) -> bool {
        self.req
            .take(cost, REQ_BUCKET_REFILL_PER_SEC, REQ_BUCKET_BURST)
    }

    /// Reset the sustained-request-refusal counter after an admitted
    /// request, because an honest peer may hit a temporary bucket limit.
    pub(crate) fn clear_req_refusal(&mut self) {
        self.req_refusals = 0;
    }

    /// Record one refused request and report whether the refusal ladder
    /// reached its strike threshold.
    pub(crate) fn note_req_refusal(&mut self) -> bool {
        self.req_refusals = self.req_refusals.saturating_add(1);
        self.req_refusals >= REQ_REFUSAL_STRIKE_THRESHOLD
    }
}
