//! src/chain/params.rs — the consensus parameters of the chain layer.
//! Every constant here is part of the consensus rules: changing one is a
//! consensus fork, not a configuration change, and must go through the
//! version-bits process described in the project documentation.

/// The mainnet network identifier. Part of every transaction signature
/// hash: a signature made under one identifier never verifies under the
/// other, which is the replay isolation between networks. The identifier
/// tracks the crate version and changes whenever consensus behavior
/// changes, per the project's network-identifier policy.
pub const MAINNET_ID: &str = "/slash/0.5.0";

/// The testnet network identifier, distinct from the mainnet identifier
/// for the same replay-isolation reason.
pub const TESTNET_ID: &str = "/slash/test/0.5.0";

/// Initial coinbase subsidy at genesis height (era 0), in cells.
pub const SUBSIDY_INITIAL: u64 = 500;

/// Blocks per halving era; nine eras take the subsidy to zero.
pub const HALVING_INTERVAL: u64 = 420_480;

/// First era with a zero coinbase subsidy. From this era on the miner
/// revenue is the perpetual one-cell proof-of-work slot plus fees.
pub const EMISSION_END_ERA: u64 = 9;

/// Retarget period in blocks; the same window length counts version-bit
/// signals for soft-fork deployments.
pub const DIFFICULTY_ADJUSTMENT_INTERVAL: u64 = 2_016;

/// Target block time in seconds.
pub const TARGET_BLOCK_TIME_SECS: u64 = 300;

/// Maximum serialized block size. The limit applies at every block
/// validation path (main extension, fork admission, fork validation,
/// reorg replay), so no path can smuggle in an oversized block after a
/// stricter rule activates.
pub const MAX_BLOCK_SIZE_BYTES: usize = 1_048_576;

/// Maximum transactions per block. The cap bounds per-block state
/// transition work independently of the byte-size limit.
pub const MAX_TX_PER_BLOCK: usize = 1_000;

/// Maximum input ranges and touched pages per transaction. The caps stop
/// range-amplification and cross-page fan-out attacks: a single
/// transaction cannot force the state engine to walk an outsized number
/// of ranges or dirty pages, so per-transaction validation work stays
/// bounded by the caps rather than by attacker-chosen input shapes.
pub const MAX_RANGES_PER_TX: usize = 4096;
pub const MAX_PAGES_PER_TX: usize = 64;

/// Maximum serialized signature length. The deployed schemes sign with
/// 64-byte Ed25519 signatures, so anything larger is malformed before any
/// hashing or signature-parse work happens. The cap keeps the wire format
/// self-describing; a future signature scheme that needs a larger
/// signature changes this constant together with its scheme gate.
pub const MAX_SIG_BYTES: usize = 64;

/// Maximum serialized memo length. The memo carries a bincode-encoded
/// DaoOp — a few hundred bytes at most — so the cap is generous margin
/// over the largest legal payload while still bounding the decode and
/// hashing work every node pays inside an otherwise valid transaction.
pub const MAX_MEMO_BYTES: usize = 4096;

/// Maximum reorg depth: the node refuses to reorg beyond the retained
/// state history, so an attacker cannot unwind long-confirmed history by
/// publishing a deep side fork.
pub const MAX_REORG_DEPTH: u64 = 100;

/// Maximum side forks tracked alongside the main chain. The pool is
/// bounded so a flood of competing branches cannot exhaust memory; the
/// work-order tie keeps the heaviest branch first, so the node always
/// considers the best fork within the bound.
pub const MAX_SIDE_FORKS: usize = 100;

/// Maximum orphan blocks processed in a single adoption pass. The cap
/// keeps the per-block work of an adoption pass bounded even when the
/// orphan pool is full.
pub const MAX_ORPHAN_ADOPTION: usize = 25;

/// Maximum number of reorg candidate blocks the two-phase reorg
/// machinery inspects when picking the winning side fork. The candidate
/// walk stops at the first branch with strictly more work, so the cap
/// limits the search without changing the outcome: any fork with less
/// work than the main chain inside the window is irrelevant to the
/// reorg decision.
pub const REORG_CANDIDATE_LIMIT: usize = 100;

/// Maximum blocks replayed when rebuilding the state history after a
/// restart. The replay starts from the newest exact anchor available
/// (genesis for a short chain, the pruned snapshot for a pruned chain)
/// and never exceeds this bound, so recovery time stays proportional to
/// the snapshot spacing instead of the chain length.
pub const HISTORY_REBUILD_MAX_REPLAY: u64 = 10_000;

/// How many blocks a checkpoint is trusted for without rechecking the
/// full header chain below it. Deep history behind a checkpoint is
/// skipped during sync exactly because the checkpoint commits to it.
pub const CHECKPOINT_TRUST_DEPTH: u64 = 10_000;
