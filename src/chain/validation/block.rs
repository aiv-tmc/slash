//! Block-level validation entry point used before gossip admission, mining
//! template assembly, and sync processing apply a block anywhere.

use crate::chain::chain::Chain;
use crate::chain::params::MAX_BLOCK_SIZE_BYTES;
use crate::chain::types::Block;

impl Chain {
    /// The block pre-checks: active-rule enforcement (version and size)
    /// plus the full speculative state transition. Used by gossip
    /// admission, the mining template loop, and the sync pipeline before a
    /// block is applied anywhere.
    pub fn validate_block(&self, block: &Block) -> bool {
        if self.check_block_meta(block).is_err() {
            return false;
        }
        // Enforce an activated RequireBlockVersion2 deployment.
        let rules = crate::governance::active_rules(&self.version_bits.deployments, block.height);
        if rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::RequireBlockVersion2))
            && block.version < 2
        {
            return false;
        }
        // Enforce an activated MaxBlockSize deployment. The serialized
        // length is computed once and compared against the stricter of
        // the active deployment limit and the base consensus limit.
        let limit = rules
            .iter()
            .find_map(|r| match r {
                crate::governance::Rule::MaxBlockSize { max_bytes } => Some(*max_bytes),
                _ => None,
            })
            .unwrap_or(MAX_BLOCK_SIZE_BYTES)
            .min(MAX_BLOCK_SIZE_BYTES);
        if bincode::serialize(block)
            .map(|b| b.len())
            .unwrap_or(usize::MAX)
            > limit
        {
            return false;
        }
        self.execute_block_state(&self.state, block).is_ok()
    }

    /// Validate a mining template: every structural and execution check
    /// [`Chain::validate_block`] performs except the proof-of-work
    /// verification, which cannot succeed before the nonce exists. The
    /// miner uses this to reject invalid candidate transactions before
    /// grinding; the fully mined block still passes through the complete
    /// validator before application.
    pub fn validate_block_template(&self, block: &Block) -> bool {
        if self.check_block_meta(block).is_err() {
            return false;
        }
        let rules = crate::governance::active_rules(&self.version_bits.deployments, block.height);
        if rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::RequireBlockVersion2))
            && block.version < 2
        {
            return false;
        }
        let limit = rules
            .iter()
            .find_map(|r| match r {
                crate::governance::Rule::MaxBlockSize { max_bytes } => Some(*max_bytes),
                _ => None,
            })
            .unwrap_or(MAX_BLOCK_SIZE_BYTES)
            .min(MAX_BLOCK_SIZE_BYTES);
        if bincode::serialize(block)
            .map(|b| b.len())
            .unwrap_or(usize::MAX)
            > limit
        {
            return false;
        }
        self.execute_block_template(&self.state, block).is_ok()
    }
}
