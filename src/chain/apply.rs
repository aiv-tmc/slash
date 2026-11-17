//! Block application: the single write path where a validated block
//! becomes part of the chain.

use std::sync::Arc;

use super::chain::Chain;
use super::types::Block;
use super::types::BlockError;

impl Chain {
    /// Apply a fully valid block extending the tip: metadata checks,
    /// speculative execution, commit of the post-state, history record,
    /// and version-bit update. The block must extend the current tip;
    /// fork handling lives in `process_block`.
    pub fn apply(&mut self, block: Block) -> Result<(), BlockError> {
        self.check_block_meta(&block)?;
        let temp = self.execute_block_state(&self.state, &block)?;
        self.blocks.push(block.clone());
        // The executed post-state becomes the tip state behind a fresh
        // `Arc`; the history records the same allocation, so no second
        // copy exists anywhere.
        self.state = Arc::new(temp);
        self.record_history(block.height);
        // Count this block's version bits and advance every deployment's
        // lifecycle; the borrow of the applied block keeps the chain
        // borrow disjoint while the deployment table is mutated.
        self.update_version_bits(&block);
        Ok(())
    }

    /// Feed a recovered block (from the crash-recovery journal) into
    /// the chain without re-checking metadata: linkage to the current
    /// tip must hold, and the block executes through the ordinary
    /// speculative pipeline. Recovery replays committed blocks only, so a
    /// linkage break means the journal is ahead of the manifest and
    /// replay stops at the first gap.
    pub fn apply_recovered(&mut self, block: Block) -> Result<(), BlockError> {
        if block.height != self.tip_height() + 1 || block.prev != self.tip_hash() {
            return Err(BlockError::InvalidPrevHash);
        }
        let temp = self.execute_block_state(&self.state, &block)?;
        self.blocks.push(block.clone());
        self.state = Arc::new(temp);
        self.record_history(block.height);
        self.update_version_bits(&block);
        Ok(())
    }
}
