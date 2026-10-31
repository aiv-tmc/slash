//! src/lib.rs — crate root for the Slash node library.
//!
//! The library exposes the consensus engine (chain + state), the treasury
//! curve, governance, networking, storage, and the FFI/RPC surfaces. The
//! process-wide [`TESTNET_MODE`] selector pins the signature domain for
//! the whole process; every component reads it when a chain identifier is
//! required, so a node can never mix mainnet and testnet semantics.
//!
//! Persistence goes through a single manifest snapshot plus a
//! crash-recovery journal: [`save`] writes the consistent triple (blocks,
//! state, governance metadata) and the journal replays blocks committed
//! after the last manifest, so a crash leaves either the previous or the
//! next consistent state, never a torn one.
//!
//! Layout:
//! - `load` — the startup recovery path: manifest load, snapshot
//!   fallback, journal replay, history rebuild, and the replay-domain
//!   gates, with its unit tests inline (test-only)

pub mod chain;
pub mod crypto;
pub mod dao;
pub mod ffi;
pub mod global_root;
pub mod governance;
pub mod mining;
pub mod onion;
pub mod p2p;
pub mod rpc;
pub mod simulation;
pub mod state;
pub mod storage;
pub mod treasury;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_fixtures;

mod load;

pub use load::{load_pruned, prune};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::SystemTime;

/// Process-wide network selector. `false` pins every signature hash to the
/// mainnet chain identifier; `true` switches the whole process to the
/// testnet domain. Every component reads this flag when a chain identifier
/// is required, so the two replay domains can never mix inside one
/// process.
pub static TESTNET_MODE: AtomicBool = AtomicBool::new(false);

/// The cached chain triple: manifest stamp, shared chain. The stamp is
/// the manifest's file metadata (length plus modification time), which
/// the hot path compares without opening the file. A manifest write goes
/// through a fresh temporary file and a rename, and the rename carries
/// the temporary's just-created modification time, so every successful
/// save moves the stamp; two saves cannot share a timestamp because the
/// fsync between them costs far more than a filesystem timestamp tick.
/// A false hit would therefore require two different manifests of equal
/// length produced within one timestamp tick, which the write path makes
/// impossible in practice.
type CachedChain = Option<((u64, SystemTime), Arc<chain::Chain>)>;

/// In-memory chain cache shared by RPC handlers and the FFI layer.
/// Repeated queries reuse the same [`Arc`] instead of re-reading and
/// re-validating the manifest from disk on every call.
static CHAIN_CACHE: OnceLock<RwLock<CachedChain>> = OnceLock::new();

/// Global pending-transaction buffer drained by the P2P node on its
/// periodic tick. RPC submissions land here; the network layer validates
/// and gossips them. The capacity is double the mempool cap so bursts
/// between ticks are smoothed without unbounded growth.
static PENDING_TXS: OnceLock<Mutex<Vec<chain::Tx>>> = OnceLock::new();

/// Read the manifest's metadata stamp: file length and modification time.
/// This is the only disk touch on the hot path of [`cached_load`], so a
/// cache hit costs one `stat` instead of a full file read plus a hash.
/// Returns `None` when no manifest exists yet.
fn manifest_stamp() -> Option<(u64, SystemTime)> {
    let path = crate::storage::data_path("manifest.bin");
    let md = std::fs::metadata(&path).ok()?;
    let mtime = md.modified().ok()?;
    Some((md.len(), mtime))
}

/// Hex-decode a 32-byte key from its string form. Malformed hex and
/// wrong-length input are rejected; every caller maps the error to the
/// same generic rejection, so the error carries no extra detail.
fn decode_key(s: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(s)?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("key must decode to exactly 32 bytes"))
}

/// The pending buffer capacity: twice the mempool cap the network layer
/// accepts. The P2P node drains the buffer into the mempool on its
/// periodic tick, so the bound only smooths bursts between ticks.
const PENDING_CAP: usize = crate::p2p::MEMPOOL_CAP * 2;

/// Submit a transaction into the global pending buffer. Returns false when
/// the buffer is full, so a submission flood cannot grow the queue without
/// bound and the caller can surface a structured rejection instead of a
/// silent drop. The P2P node drains this buffer periodically and runs full
/// mempool validation before broadcasting.
pub fn submit_pending_tx(tx: chain::Tx) -> bool {
    let buf = PENDING_TXS.get_or_init(|| Mutex::new(Vec::new()));
    match buf.lock() {
        Ok(mut guard) if guard.len() < PENDING_CAP => {
            guard.push(tx);
            true
        }
        _ => false,
    }
}

/// Remove and return all currently pending transactions.
pub fn drain_pending_txs() -> Vec<chain::Tx> {
    let buf = PENDING_TXS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = buf.lock() {
        std::mem::take(&mut *guard)
    } else {
        Vec::new()
    }
}

/// Select the chain identifier that matches the global network mode.
/// Keeping this in one place guarantees that a loaded chain never mixes
/// identifiers between mainnet and testnet semantics.
fn active_chain_id() -> Vec<u8> {
    if TESTNET_MODE.load(Ordering::SeqCst) {
        chain::TESTNET_ID.as_bytes().to_vec()
    } else {
        chain::MAINNET_ID.as_bytes().to_vec()
    }
}

/// Load the chain through the in-memory cache. Returns a shared reference
/// so that repeated RPC and FFI queries do not duplicate memory. The hot
/// path compares the manifest's metadata stamp and returns the cached
/// chain on a match; a miss reloads the chain from disk and re-stamps
/// the cache. The stamp is sampled again after the load, so a manifest
/// written while the load was in flight is picked up by the next call.
pub fn cached_load() -> Arc<chain::Chain> {
    let cache = CHAIN_CACHE.get_or_init(|| RwLock::new(None));
    let stamp = manifest_stamp();
    if let Ok(guard) = cache.read() {
        if let Some((cached_stamp, chain)) = &*guard {
            if stamp.as_ref().is_some_and(|s| s == cached_stamp) {
                return Arc::clone(chain);
            }
        }
    }
    let chain = Arc::new(load());
    let stamp = manifest_stamp();
    if let Ok(mut guard) = cache.write() {
        match stamp {
            Some(s) => *guard = Some((s, Arc::clone(&chain))),
            // Without a manifest there is no stable stamp; keep the cache empty.
            None => *guard = None,
        }
    }
    Arc::clone(&chain)
}

/// Persist the full chain (blocks + state + governance metadata) to disk
/// atomically via the manifest. The manifest guarantees that readers always
/// see a consistent snapshot. Checkpoints, version-bit deployments, and the
/// chain identifier are written with the blocks so a restart restores the
/// identical validation rules and replay domain; without them soft-fork
/// gates would silently vanish.
/// After a successful manifest write the crash-recovery journal is truncated
/// because the manifest now covers every journaled block.
pub fn save(chain: &chain::Chain) {
    match storage::save_manifest(
        &chain.blocks,
        &chain.state,
        chain.base_height,
        &chain.checkpoints,
        &chain.version_bits.deployments,
        chain.chain_id.as_bytes(),
    ) {
        Ok(()) => {
            if let Err(e) = storage::journal_truncate() {
                eprintln!("[save] journal truncate failed: {}", e);
            }
        }
        Err(e) => eprintln!("[save] manifest storage failed: {}", e),
    }
}

/// Load the chain from disk. If block data is missing but a snapshot exists,
/// fall back to the latest snapshot and replay pruned blocks.
pub fn load() -> chain::Chain {
    load::load_pruned()
}
