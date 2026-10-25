//! The fee-claim sweep.
//!
//! Miners claim accumulated fee-vault cells in their blocks. Every claim
//! must come from the vault, must be unlocked at the executing height,
//! must pay the block miner exactly, must carry no lock, and an empty
//! claim set is a valid no-op — a block may carry no fee claims at all.
//! All amount arithmetic is checked: a malformed claim width fails the
//! sweep instead of wrapping.

use super::page::Output;
use super::params::{page_id, page_offset, FEE_VAULT};
use super::state::State;

impl State {
    /// Claim fee-vault outputs for the miner of the block. The empty
    /// claim set is valid and leaves the vault untouched. Every claim
    /// must pay the block miner exactly, must not carry a lock, and each
    /// claimed range must be unlocked and vault-owned at the executing
    /// height.
    pub fn claim_fees(&mut self, miner: [u8; 32], claims: &[Output], height: u64) -> bool {
        if claims.is_empty() {
            return true;
        }
        if claims.iter().any(|c| c.to != miner) {
            return false;
        }
        // A claim lock would be serialized into the block, committed to
        // by the block hash, and never enforced by this sweep: the credit
        // is spendable immediately. Rather than silently ignoring a
        // field the wire format carries, the sweep refuses locked claims
        // outright, so the field's meaning stays unambiguous for every
        // historical block.
        if claims.iter().any(|c| c.lock.is_some()) {
            return false;
        }
        for claim in claims {
            // A malformed range width, or a zero-width range, is not a
            // claim and fails the sweep instead of wrapping into a bogus
            // balance credit.
            let Some(width) = claim.end.checked_sub(claim.start) else {
                return false;
            };
            if width == 0 {
                return false;
            }
            if claim.to == FEE_VAULT {
                // Vault-to-vault claim: the recipient already owns the
                // cells, so the round trip must leave both the page map
                // and the balance cache untouched. Debiting the cache
                // without a matching credit, or removing and re-inserting
                // the same range, would only push the cached vault balance
                // away from the page map it mirrors. The claim is still
                // fully validated — ownership and lock state at the
                // executing height — before it is skipped.
                if !self.owns(FEE_VAULT, claim.start, claim.end, height) {
                    return false;
                }
                continue;
            }
            if !self.remove_for_spend(FEE_VAULT, claim.start, claim.end, height) {
                return false;
            }
            self.get_page_mut(page_id(claim.start)).insert_output(
                page_offset(claim.start),
                page_offset(claim.start) + width,
                claim.to,
            );
            // The cache credit is bounded by the number of cells N, so
            // the addition cannot overflow in any reachable state; the
            // checked form keeps the invariant local to the line that
            // relies on it.
            let Some(credited) = self
                .balance_cache
                .get(&claim.to)
                .copied()
                .unwrap_or(0)
                .checked_add(width)
            else {
                return false;
            };
            self.balance_cache.insert(claim.to, credited);
        }
        true
    }
}
