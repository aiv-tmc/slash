//! src/storage/mod.rs — persistence for blocks, state, snapshots, and the
//! chain metadata. Every write is atomic (temp file, fsync, rename), so a
//! crash leaves either the previous or the next consistent state, never a
//! torn file. The manifest is published last: a reader that sees it is
//! guaranteed that the referenced files are complete and consistent.
//!
//! Every on-chain path is resolved against the library data directory:
//! once `slash_init` installs a base, all file access stays inside it
//! without ever changing the process working directory, so an embedding
//! application's own relative-path I/O is never re-anchored as a side
//! effect of initializing the library.
//!
//! Layout:
//! - `journal` — the crash-recovery journal of applied blocks
//! - `state` — the state header + page files (`save`/`load`)
//! - `manifest` — the atomic chain snapshot (blocks + state + metadata)
//! - `snapshot` — full-state snapshots for pruned recovery

mod journal;
mod manifest;
mod snapshot;
mod state;

pub use journal::*;
pub use manifest::*;
pub use snapshot::*;
pub use state::*;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The library data directory. Set once through `slash_init`; while it
/// stays unset, every relative path resolves against the current working
/// directory, which preserves the standalone-node behavior where the
/// process itself was started inside the data directory.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Resolve one on-chain relative path against the configured data
/// directory. An unset base yields the plain relative path, so callers
/// that never invoke `slash_init` observe the previous behavior exactly.
pub(crate) fn data_path(rel: &str) -> PathBuf {
    let base = DATA_DIR.get_or_init(|| PathBuf::from("."));
    base.join(rel)
}

/// Install the data directory. Called by `slash_init` exactly once per
/// process; a second call is refused so an embedding application cannot
/// silently re-anchor an in-use library into another directory.
pub(crate) fn set_data_dir(path: PathBuf) -> Result<(), PathBuf> {
    DATA_DIR.set(path)
}

/// State header: the index of every dirty page plus the consensus fields
/// that do not live inside the page files (locks, balance cache, treasury
/// curve counters, signer keys, and the DAO tables).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Header {
    /// Ids of the pages persisted under `pages/`.
    page_ids: Vec<u16>,
    /// Cell-indexed lock expiries.
    locks: std::collections::BTreeMap<u64, u64>,
    /// Cell-indexed lock creation heights, mirroring the expiry map one
    /// entry per locked cell so snapshot-pinned governance weight
    /// survives a restart exactly as it was.
    lock_created: std::collections::BTreeMap<u64, u64>,
    /// Incremental balance cache, persisted so a restart does not replay
    /// the whole chain to rebuild balances.
    balance_cache: HashMap<[u8; 32], u64>,
    /// Public key of the treasury signer.
    treasury_signer: [u8; 32],
    /// Number of cells sold by the bonding curve so far.
    total_sold: u64,
    /// Number of cells bought back by the bonding curve so far.
    total_bought_back: u64,
    /// Public key of the dev fund signer.
    dev_fund_signer: [u8; 32],
    /// Dev fee on user transfers in basis points.
    dev_fee_bps: u64,
    /// Dev fee on the coinbase subsidy in basis points.
    mine_fee_bps: u64,
    /// DAO proposals and counters.
    dao: crate::dao::DaoState,
    /// Emergency pause state in effect at the persisted height: the flag
    /// bits under freeze and the height at which the freeze expires, so a
    /// restart resumes with exactly the freeze the chain had.
    pub emergency_flags: u8,
    pub emergency_until: u64,
}

/// Full chain metadata record: the pruned base height, the checkpoint map,
/// the version-bit deployment table, and the chain identifier that pins
/// the replay domain.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChainMeta {
    pub base_height: u64,
    pub checkpoints: BTreeMap<u64, [u8; 32]>,
    pub deployments: Vec<crate::chain::Deployment>,
    pub chain_id: Vec<u8>,
}

/// Content stamp of one persisted data file: its byte length and a BLAKE3
/// digest taken at save time. Stamps turn the manifest into an integrity
/// record: the in-memory chain cache derives its key from the manifest
/// bytes, so embedding stamps invalidates the cache on every save, and
/// the loader rejects a data file that no longer matches its stamp
/// instead of restoring a mixed triple.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FileStamp {
    pub(crate) len: u64,
    pub(crate) digest: [u8; 32],
}

/// The manifest: paths to the three files that together form one
/// consistent chain snapshot, plus a content stamp for each file.
/// Written last, after all referenced files are complete.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    blocks_path: String,
    state_header_path: String,
    chain_meta_path: String,
    blocks_stamp: FileStamp,
    state_stamp: FileStamp,
    meta_stamp: FileStamp,
}
