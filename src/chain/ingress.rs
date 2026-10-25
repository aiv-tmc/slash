//! Block ingress: the single dispatch entry for inbound blocks.
//!
//! `process_block` routes every inbound block to exactly one of three
//! destinations: the main-chain extension path, the fork-competition
//! path, or the orphan pool. The routing decision is made from height and
//! parent linkage alone, so every node dispatches identical blocks
//! identically and the three pools never overlap.
//!
//! Both retention pools are bounded twice: by their entry counts and by
//! a byte ceiling over the serialized sizes of the parked blocks. The
//! count caps alone let an adversary park on the order of half a gigabyte
//! of unvalidated, proof-of-work-free input in every node's memory with
//! well-formed block-shaped messages; the byte ceilings close that
//! exposure while the count caps stay in force as a secondary bound.
//! Every entry carries its size from the moment of admission, so the
//! budget check is a sum of cached integers and the eviction loop
//! subtracts exactly what it removes.

use super::chain::Chain;
use super::params::MAX_SIDE_FORKS;
use super::reorg::fork_heavier_than_main;
use super::types::{Block, BlockError, BlockProcessResult};

/// Byte ceiling for the side-fork pool. A single block is capped at one
/// MiB by the structural gate, so this ceiling admits dozens of
/// maximum-size competing branches while cutting the pre-fix worst case
/// of one hundred full blocks by two thirds.
const MAX_SIDE_FORK_BYTES: u64 = 32 * 1024 * 1024;

/// Byte ceiling for the orphan pool. Orphans are by definition
/// unvalidated, so this pool is the cheapest memory an adversary can
/// occupy; the ceiling keeps the whole pool below one fifth of the
/// pre-fix worst case of four hundred full blocks, and the eviction
/// pressure stays the only relief a flooding peer has to defeat.
const MAX_ORPHAN_BYTES: u64 = 64 * 1024 * 1024;

impl Chain {
    /// The single entry point for incoming blocks. Blocks that continue
    /// the tip apply immediately; blocks at or below the tip, and blocks
    /// that extend a retained side fork, enter the fork-competition
    /// path; blocks whose parent is unknown to both the main chain and
    /// the side-fork pool wait in the orphan pool. Routing a side-fork
    /// extension into the competition (rather than into the orphan pool)
    /// is what lets a competing branch keep growing: the fork walk inside
    /// `validate_fork_chain` follows side-fork parents, and a branch that
    /// accumulates strictly more work than the main chain over the
    /// contested span is adopted through `reorg_to_fork`.
    pub fn process_block(&mut self, block: Block) -> Result<BlockProcessResult, BlockError> {
        let height = block.height;
        let tip = self.tip_height();
        let hash = self.block_hash(&block);
        if self.is_known_block(hash, height, tip) {
            return Err(BlockError::Duplicate);
        }
        if height == tip + 1 && block.prev == self.tip_hash() {
            // Main-chain extension: the common case.
            if !self.validate_block(&block) {
                return Err(BlockError::InvalidTx);
            }
            self.apply(block)?;
            self.process_orphans();
            return Ok(BlockProcessResult::ExtendedMain);
        }
        if height <= tip || self.side_forks.contains_key(&block.prev) {
            // Competing branch: a block inside the retained window, or an
            // extension of a retained side fork. Both validate through
            // the same cursor walk, so an extending block simply
            // lengthens the branch it builds on.
            if !self.validate_side_fork_entry(&block) {
                return Err(BlockError::InvalidTx);
            }
            match self.validate_fork_chain(&block) {
                Ok(fork) if fork_heavier_than_main(&fork, &self.blocks) => {
                    let disconnected: Vec<u64> = (fork[0].height..=tip).collect();
                    self.reorg_to_fork(&fork)?;
                    return Ok(BlockProcessResult::Reorged {
                        disconnected,
                        connected: fork.iter().map(|b| b.height).collect(),
                    });
                }
                Ok(_fork) => {
                    // Not heavier: retain as a side fork for a possible
                    // future reorg. Evict the oldest entries until the
                    // pool fits both the count cap and the byte ceiling;
                    // each evicted entry subtracts its own cached size,
                    // so the running total stays exact across the loop.
                    // A candidate whose size alone exceeds the ceiling
                    // cannot be split, so the loop drains the pool and the
                    // insert proceeds with a single entry: the structural
                    // block-size limit keeps even that bounded.
                    let entry_size = bincode::serialize(&block)
                        .map(|b| b.len() as u64)
                        .unwrap_or(u64::MAX);
                    let mut bytes: u64 =
                        self.side_forks.values().map(|(_, size)| *size as u64).sum();
                    while self.side_forks.len() >= MAX_SIDE_FORKS
                        || bytes.saturating_add(entry_size) > MAX_SIDE_FORK_BYTES
                    {
                        if let Some(oldest) = self.side_forks.keys().next().copied() {
                            if let Some((_, evicted)) = self.side_forks.remove(&oldest) {
                                bytes = bytes.saturating_sub(evicted as u64);
                            }
                        } else {
                            break;
                        }
                    }
                    self.side_forks.insert(hash, (block, entry_size as u32));
                    return Ok(BlockProcessResult::SideFork);
                }
                Err(e) => return Err(e),
            }
        }
        // Unknown parent on the main chain and outside the side-fork
        // pool: park in the orphan pool; process_orphans retries parked
        // blocks whenever the tip advances. The pool is bounded by its
        // entry count at four times the side-fork cap and by a byte
        // ceiling, so a flood of parentless blocks cannot exhaust memory
        // no matter how large each block is.
        let entry_size = bincode::serialize(&block)
            .map(|b| b.len() as u64)
            .unwrap_or(u64::MAX);
        let mut bytes: u64 = self
            .orphan_blocks
            .values()
            .map(|(_, size)| *size as u64)
            .sum();
        while self.orphan_blocks.len() >= MAX_SIDE_FORKS * 4
            || bytes.saturating_add(entry_size) > MAX_ORPHAN_BYTES
        {
            if let Some(oldest) = self.orphan_blocks.keys().next().copied() {
                if let Some((_, evicted)) = self.orphan_blocks.remove(&oldest) {
                    bytes = bytes.saturating_sub(evicted as u64);
                }
            } else {
                break;
            }
        }
        self.orphan_blocks.insert(hash, (block, entry_size as u32));
        Ok(BlockProcessResult::Orphan)
    }

    /// Duplicate detection across the main vector and both pools. The
    /// main vector holds blocks strictly above `base_height` (the block
    /// at height `h` sits at index `h - base_height - 1`), so the index
    /// is computed with `checked_sub` and the lookup is skipped at or
    /// below the base: a block there cannot duplicate retained history,
    /// and the subtraction can never underflow.
    fn is_known_block(&self, hash: [u8; 32], height: u64, tip: u64) -> bool {
        if height > self.base_height && height <= tip {
            if let Some(index) = height
                .checked_sub(self.base_height)
                .and_then(|offset| offset.checked_sub(1))
            {
                if self
                    .blocks
                    .get(index as usize)
                    .is_some_and(|b| self.block_hash(b) == hash)
                {
                    return true;
                }
            }
        }
        self.side_forks.contains_key(&hash) || self.orphan_blocks.contains_key(&hash)
    }

    /// Structural gate for side-fork entries: reject garbage heights,
    /// difficulties, and sizes before any expensive validation work. A
    /// block one above the tip is admitted only when it extends a
    /// retained side fork; anything higher waits for its parent instead
    /// of entering the competition early.
    pub(crate) fn validate_side_fork_entry(&self, block: &Block) -> bool {
        if block.height == 0 {
            return false;
        }
        if block.height > self.tip_height() && !self.side_forks.contains_key(&block.prev) {
            return false;
        }
        if block.difficulty == 0 {
            return false;
        }
        if bincode::serialize(block)
            .map(|b| b.len())
            .unwrap_or(usize::MAX)
            > super::params::MAX_BLOCK_SIZE_BYTES
        {
            return false;
        }
        true
    }
}
