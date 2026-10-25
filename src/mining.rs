// `mine_parallel` takes one parameter per mining input: the arity mirrors
// the consensus-mandated hash field list, which grouping would hide at
// call sites, so the arity lint is allowed at module scope.
#![allow(clippy::too_many_arguments)]

//! RandomX proof-of-work mining with parallel nonce search.
//!
//! The miner constructs hash inputs through the shared `chain::pow_input`
//! builder — the same function the verifier uses — so a nonce that
//! satisfies the difficulty target is valid consensus-wide by construction.

use rust_randomx::{Context, Hasher as RandomXHasher};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// One cache entry: the RandomX context, the timestamp of its last use,
/// and whether the context holds the full dataset.
struct CachedContext {
    context: Arc<Context>,
    last_used: Instant,
    /// Whether the context holds the full RandomX dataset. Light
    /// verification contexts are cheap to rebuild; full mining contexts
    /// pin a large memory allocation and are expensive to recreate, so
    /// the eviction policy uses this flag to protect the mining tip's
    /// dataset from verification traffic.
    full_mem: bool,
}

/// Global cache of RandomX contexts keyed by the block's `prev` hash, so
/// mining and verification reuse the expensive contexts across attempts
/// that share the same `prev`.
static CONTEXT_CACHE: OnceLock<std::sync::Mutex<HashMap<[u8; 32], CachedContext>>> =
    OnceLock::new();

/// How long a cached context stays alive after its last use.
const CONTEXT_CACHE_TTL: Duration = Duration::from_secs(600);
/// Maximum number of cached RandomX contexts. Each context pins a large
/// memory allocation, so the count is bounded in addition to the TTL: a peer
/// streaming headers with random prev hashes must not be able to exhaust
/// node memory by forcing the creation of unbounded contexts.
const MAX_CONTEXT_CACHE_ENTRIES: usize = 8;

/// Hash evaluations between wall-clock refreshes inside the nonce loops.
/// The proof-of-work input binds the one-second-resolution timestamp, so a
/// search that outlives its starting second keeps hashing an input the
/// consensus clock has already moved past. The interval is sized against
/// the expected per-hash cost: frequent enough that the stamped time never
/// lags the wall clock, rare enough that the clock read stays noise.
const TIME_REFRESH_INTERVAL: u64 = 1 << 16;

/// Retrieve a cached RandomX Context for the given prev hash, creating one
/// when no live entry exists. `full_mem` must be true for mining and may be
/// false for verification. Eviction prefers light verification contexts —
/// they are cheap to rebuild, while evicting a full mining context forces a
/// large reallocation before the next mining attempt can hash a single
/// nonce, so header-sync verification traffic recycles verification entries
/// among themselves without displacing the mining tip's dataset.
pub fn get_context(prev: &[u8; 32], full_mem: bool) -> Arc<Context> {
    let key = blake3::hash(prev).into();
    let now = Instant::now();

    // Refresh and return a live entry when one exists. A light entry cannot
    // serve a mining request, so full requests miss on light entries and
    // recreate them with the full dataset.
    let cache = CONTEXT_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(entry) = cache.lock().unwrap().get_mut(&key) {
        let full_enough = !full_mem;
        if full_enough && now.duration_since(entry.last_used) <= CONTEXT_CACHE_TTL {
            entry.last_used = now;
            return Arc::clone(&entry.context);
        }
    }

    // Create a fresh context. Light contexts are preferred for verification
    // because they use far less memory; mining always needs the full dataset.
    let ctx = Arc::new(Context::new(prev, full_mem));

    let mut cache = cache.lock().unwrap();
    // Enforce the entry-count bound before inserting. The victim is the
    // least recently used light context when one exists, falling back to
    // plain least-recently-used order only when the cache holds
    // exclusively full contexts.
    if cache.len() >= MAX_CONTEXT_CACHE_ENTRIES {
        let victim = cache
            .iter()
            .filter(|(_, v)| !v.full_mem)
            .min_by_key(|(_, v)| v.last_used)
            .or_else(|| cache.iter().min_by_key(|(_, v)| v.last_used))
            .map(|(k, _)| *k);
        if let Some(victim) = victim {
            cache.remove(&victim);
        }
    }
    cache.insert(
        key,
        CachedContext {
            context: Arc::clone(&ctx),
            last_used: now,
            full_mem,
        },
    );
    ctx
}

/// A candidate block assembled by the miner but not yet validated. `time`
/// carries the timestamp the search actually ground against: the block
/// must be stamped with the same value or the proof is invalid for the
/// header that commits to it.
pub struct MinerBlock {
    pub nonce: u64,
    pub value: u64,
    pub time: u64,
}

/// Search for a valid nonce in parallel across all available cores.
/// Each worker strides nonces by the worker count so no two workers evaluate
/// the same candidate, and every call draws a fresh random nonce base so two
/// searches that start within the same wall-clock second — the common case
/// for a retry after a stale-time rejection — cover disjoint subspaces with
/// overwhelming probability instead of rescanning the identical sequence.
/// The search stops as soon as any worker satisfies the difficulty target,
/// finds an acceptable proof-of-work slot, or observes the cancellation flag.
pub fn mine_parallel(
    prev: [u8; 32],
    time: u64,
    height: u64,
    miner: [u8; 32],
    diff: u64,
    cancel: Arc<AtomicBool>,
    threads: usize,
    slot_check: impl Fn(u64) -> bool + Send + Sync + 'static,
) -> Option<MinerBlock> {
    let slot_check = Arc::new(slot_check);
    let ctx = get_context(&prev, true);
    let found = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel::<MinerBlock>();
    // Random base for this search call: overlapping invocations within one
    // second no longer share a nonce sequence.
    let base = rand::random::<u64>();
    for i in 0..threads {
        let tx = tx.clone();
        let cancel = cancel.clone();
        let found = found.clone();
        let ctx = Arc::clone(&ctx);
        let slot_check = Arc::clone(&slot_check);
        std::thread::spawn(move || {
            let hasher = RandomXHasher::new(ctx);
            let mut nonce = base.wrapping_add(i as u64);
            // Each worker owns its timestamp copy; a winning worker
            // publishes the value it ground against so the caller stamps
            // the block with the matching time.
            let mut time = time;
            let mut iterations: u64 = 0;
            while !cancel.load(Ordering::Relaxed) && !found.load(Ordering::Relaxed) {
                iterations = iterations.wrapping_add(1);
                if iterations.is_multiple_of(TIME_REFRESH_INTERVAL) {
                    let now = chrono::Utc::now().timestamp() as u64;
                    if now > time {
                        time = now;
                    }
                }
                let inp = crate::chain::pow_input(prev, time, height, miner, nonce);
                let out = hasher.hash(&inp);
                let h = out.as_ref();
                let u = u64::from_le_bytes(h[..8].try_into().unwrap());
                let slot = u % crate::state::N;
                if u % diff == 0 && slot_check(slot) {
                    // Publish the winning block, signal the other workers,
                    // and exit. The receiver may already have returned, in
                    // which case the send result is ignored.
                    found.store(true, Ordering::Relaxed);
                    let _ = tx.send(MinerBlock {
                        nonce,
                        value: u,
                        time,
                    });
                    break;
                }
                nonce = nonce.wrapping_add(threads as u64);
            }
        });
    }
    drop(tx);
    rx.recv().ok()
}

/// Synchronous single-threaded mining used by tests and the miner RPC path.
/// `slot_check` lets the caller reject proof-of-work slots that target
/// non-treasury cells without invalidating the whole search. The search
/// starts from a random nonce and refreshes the timestamp from the wall
/// clock; the returned block carries the time the proof was ground against.
pub fn mine(
    prev: [u8; 32],
    time: u64,
    height: u64,
    miner: [u8; 32],
    diff: u64,
    cancel: Arc<AtomicBool>,
    slot_check: impl Fn(u64) -> bool,
) -> Option<MinerBlock> {
    let ctx = get_context(&prev, true);
    let hasher = RandomXHasher::new(ctx);
    let mut nonce = rand::random::<u64>();
    let mut time = time;
    let mut iterations: u64 = 0;
    while !cancel.load(Ordering::Relaxed) {
        iterations = iterations.wrapping_add(1);
        if iterations.is_multiple_of(TIME_REFRESH_INTERVAL) {
            let now = chrono::Utc::now().timestamp() as u64;
            if now > time {
                time = now;
            }
        }
        let inp = crate::chain::pow_input(prev, time, height, miner, nonce);
        let out = hasher.hash(&inp);
        let h = out.as_ref();
        let u = u64::from_le_bytes(h[..8].try_into().unwrap());
        let slot = u % crate::state::N;
        if u % diff == 0 && slot_check(slot) {
            return Some(MinerBlock {
                nonce,
                value: u,
                time,
            });
        }
        nonce = nonce.wrapping_add(1);
    }
    None
}

/// Header-only nonce search used by the P2P mining loop. Unlike `mine` and
/// `mine_parallel` it accepts any proof-of-work slot cell because the block
/// template's slot is validated against the treasury ownership table later,
/// during full block validation. Returns the winning nonce, the slot cell
/// derived from the hash output, and the timestamp the search ground
/// against — which may be later than the template time when a long search
/// advanced it — or None on cancellation.
pub fn mine_block(
    prev: [u8; 32],
    time: u64,
    height: u64,
    miner: [u8; 32],
    diff: u64,
    cancel: Arc<AtomicBool>,
) -> Option<(u64, u64, u64)> {
    let found = mine(prev, time, height, miner, diff, cancel, |_| true)?;
    Some((found.nonce, found.value % crate::state::N, found.time))
}

/// Mining facade used by the P2P node. The node runs this blocking search on
/// the thread-pool via `spawn_blocking` and receives a fully assembled block
/// template whose transaction list and fee claims it fills in afterwards.
pub struct Miner;

impl Miner {
    /// Run the single-threaded nonce search and package the result into a
    /// block template. `cancel` and `slot_check` are optional conveniences:
    /// mining proceeds without cancellation, and without a slot predicate
    /// every treasury-range solution is accepted. The block timestamp is
    /// the wall clock at search start; the search advances it while it
    /// runs, and the returned block is stamped with the time the winning
    /// proof was ground against, keeping the header and the proof
    /// consistent under the median-time and future-time rules.
    pub fn mine(
        prev: [u8; 32],
        height: u64,
        miner: [u8; 32],
        diff: u64,
        cancel: Option<Arc<AtomicBool>>,
        slot_check: Option<Arc<dyn Fn(u64) -> bool + Send + Sync>>,
    ) -> Option<crate::chain::Block> {
        let time = chrono::Utc::now().timestamp() as u64;
        let cancel = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let slot_check = slot_check.unwrap_or_else(|| Arc::new(|_| true));
        let found = mine(prev, time, height, miner, diff, cancel, move |slot: u64| {
            slot_check(slot)
        })?;
        Some(crate::chain::Block {
            version: 1,
            prev,
            time: found.time,
            height,
            nonce: found.nonce,
            miner,
            mined_cell: found.value % crate::state::N,
            txs: vec![],
            fee_claims: vec![],
            difficulty: diff,
            page_roots: std::collections::BTreeMap::new(),
        })
    }
}
