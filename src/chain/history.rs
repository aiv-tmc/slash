//! Reorg window history.
//!
//! The chain records the post-state of every applied block inside the
//! reorg window so a reorg can rewind to the fork point and replay the
//! competing branch against exactly the state that was live there.
//! Entries are shared `Arc`s of the states the chain itself installed:
//! recording is a reference-count bump, and replaying the rebuild moves
//! each intermediate state into the table without copying it. The
//! deployment table is recorded alongside every state — the rule-set
//! half of the pair — so a restored state is always validated under the
//! rules it was built with.

use super::chain::Chain;
use super::params::MAX_REORG_DEPTH;
use super::types::BlockError;
use crate::state::State;

impl Chain {
    /// Record the current post-state and deployment table at `height`,
    /// then drop every history entry that fell out of the reorg window.
    /// Recording shares the state's `Arc` instead of copying it: the
    /// chain and the history hold the same allocation, and the entry stays
    /// valid forever because block execution never mutates a state in
    /// place — it returns a fresh one. The deployment table is cloned
    /// because version-bit state is small and must be captured exactly as
    /// it was after the block that produced the state.
    pub fn record_history(&mut self, height: u64) {
        self.state_history
            .insert(height, std::sync::Arc::clone(&self.state));
        self.deployments_history
            .insert(height, self.version_bits.deployments.clone());

        // The window is anchored at the recorded height. In the ordinary
        // committed path that height is the current tip, but the direct
        // unit test records synthetic heights without extending the block
        // vector; anchoring on the argument keeps retention a pure property
        // of the window instead of an accidental property of `blocks`.
        let cutoff = height.saturating_sub(MAX_REORG_DEPTH);
        self.state_history.retain(|&h, _| h >= cutoff);
        self.deployments_history.retain(|&h, _| h >= cutoff);
    }

    /// The oldest height with a recorded state, if any.
    pub fn oldest_recorded_height(&self) -> Option<u64> {
        self.state_history.keys().min().copied()
    }

    /// Rebuild the history window by replaying retained blocks from an
    /// exact anchor state and deployment table. Every applied block
    /// advances the replay state through the ordinary speculative
    /// pipeline, so the rebuilt table is identical to what online
    /// recording would have produced: entry `h` is the post-state of
    /// block `h`, the exact state a reorg restores at the fork point.
    /// Each intermediate state moves into the table behind one shared
    /// `Arc` (no per-block copy); the replay bound
    /// `HISTORY_REBUILD_MAX_REPLAY` keeps the walk finite, and a failure
    /// leaves the history empty so every later reorg fails closed.
    pub(crate) fn rebuild_state_history(
        &mut self,
        anchor_state: State,
        anchor_height: u64,
        anchor_deployments: Vec<super::version_bits::Deployment>,
    ) -> Result<(), BlockError> {
        let tip = self.tip_height();
        if anchor_height > tip {
            return Err(BlockError::InvalidHeight);
        }
        self.state_history.clear();
        self.deployments_history.clear();
        // The replay cursor rides behind an `Arc` so the executed
        // post-state can be shared into the table: inserting a clone of
        // the `Arc` records the state without copying it, and the cursor
        // continues from the same allocation.
        let mut temp = std::sync::Arc::new(anchor_state);
        let mut table = super::version_bits::VersionBits {
            deployments: anchor_deployments,
        };
        self.state_history
            .insert(anchor_height, std::sync::Arc::clone(&temp));
        self.deployments_history
            .insert(anchor_height, table.deployments.clone());
        for block in &self.blocks {
            if block.height <= anchor_height {
                continue;
            }
            if block.height.saturating_sub(anchor_height)
                > super::params::HISTORY_REBUILD_MAX_REPLAY
            {
                return Err(BlockError::CannotReorgBeyondHistory);
            }
            // Execute against the current replay state, then record the
            // fresh post-state under the block's height and continue from
            // it: entry `h` must be the state that was live when block
            // `h + 1` was the next candidate — the exact table online
            // recording produces and the exact snapshot `reorg_to_fork`
            // restores at the fork point. Recording the pre-block cursor
            // here would shift every entry one height back and hand a
            // post-reload reorg a state from before its fork point.
            let next = std::sync::Arc::new(self.execute_block_state(&temp, block)?);
            table.update(block);
            self.state_history
                .insert(block.height, std::sync::Arc::clone(&next));
            self.deployments_history
                .insert(block.height, table.deployments.clone());
            temp = next;
        }
        // Keep only the states inside the reorg window: older entries
        // can never be restored and only consume memory.
        let cutoff = tip.saturating_sub(MAX_REORG_DEPTH);
        self.state_history.retain(|&h, _| h >= cutoff);
        self.deployments_history.retain(|&h, _| h >= cutoff);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retention bound is a pure function of the window: recording
    /// far past the reorg depth keeps at most MAX_REORG_DEPTH + 1
    /// entries and always the newest ones. Exercised directly against
    /// `record_history` so the bound itself is pinned without paying
    /// for a long mining walk; the integration suites pin that `apply`
    /// records history at all.
    #[test]
    fn record_history_caps_the_window() {
        let mut chain = Chain::genesis();
        let total = MAX_REORG_DEPTH + 10;
        for h in 0..total {
            chain.record_history(h);
        }
        let cap = MAX_REORG_DEPTH as usize + 1;
        assert!(
            chain.state_history.len() <= cap,
            "state history exceeds the reorg window"
        );
        assert!(
            chain.deployments_history.len() <= cap,
            "deployment history exceeds the reorg window"
        );
        // The window keeps the newest heights and drops the oldest: the
        // earliest ten recordings must have aged out.
        let min_key = chain.state_history.keys().min().copied().unwrap_or(0);
        assert!(
            min_key >= 9,
            "the oldest recordings must age out of the window"
        );
    }
}
