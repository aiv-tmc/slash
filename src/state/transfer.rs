// The transaction entry point takes the consensus parameter set verbatim
// (sender, inputs, outputs, signature, height, chain id, scheme, memo);
// bundling it into a struct would churn every caller — consensus,
// mempool, fixtures, FFI — without removing a real hazard.
#![allow(clippy::too_many_arguments)]

//! The transfer pipeline: one shared core of predicates and staging that
//! the committing engine (`tx`, and the consensus-internal `apply_transfer`)
//! and the read-only admission twin (`tx_speculative`, in the `speculative`
//! child module) both drive. A transfer is staged on a touched-page
//! overlay and applied to the live map only when every check — packing,
//! per-page balance, ownership with locks, the treasury-lock rule, the
//! governance-mint provenance rule, and the global lock cap — passed, so
//! a rejected transaction leaves no partial mutation behind. Because the
//! predicates and the staging live in exactly one place, the admission
//! verdict is bit-identical to the engine verdict by construction.
//!
//! The predicate core lives in the `gates` child module and the staging
//! machinery in the `staging` child module; this file keeps the entry
//! points and the direct-spend helper. The import block below serves the
//! whole family: the child modules address these names through
//! `use super::*`, so the list keeps every name a child references even
//! where this file alone would need fewer.

use super::page::Output;
use super::params::{
    page_id, page_offset, DEV_FUND, FEE_VAULT, MIN, MIN_FEE_DENOMINATOR, N, TREASURY,
};
use super::state::State;

/// The admission twin of the transfer engine. It runs the identical
/// gates and staging against `&self` and commits nothing, so mempool
/// admission costs work proportional to the transaction's footprint
/// instead of a clone of the whole ledger.
#[path = "transfer_speculative.rs"]
mod speculative;

/// The predicate core: every check that does not mutate state, in one
/// place so the committing engine and the admission twin both enter
/// through it, so no gate can drift between mempool admission and block
/// execution.
#[path = "transfer_gates.rs"]
mod gates;

/// The staging machinery: the touched-page overlay, the staged-transfer
/// record, and the commit that publishes it. Kept beside the engine so
/// the entry points stay a thin dispatch layer.
#[path = "transfer_stage.rs"]
mod staging;

impl State {
    /// Execute a signed transaction: every predicate, then commit.
    /// Rejection leaves the state untouched. Under the `SLASH_TRACE_TX`
    /// environment variable the refusal phase (gates or staging) is
    /// logged with the sender and inputs, so a failing template or test
    /// localizes the defect from a single run.
    pub fn tx(
        &mut self,
        from: [u8; 32],
        inputs: &[(u64, u64)],
        outputs: &[Output],
        sig: &[u8],
        height: u64,
        chain_id: &[u8],
        scheme: u8,
        memo: &Option<Vec<u8>>,
    ) -> bool {
        if !self.transfer_gates(&from, inputs, outputs, sig, chain_id, scheme, memo) {
            if std::env::var_os("SLASH_TRACE_TX").is_some() {
                eprintln!(
                    "[txs] transfer refused at gates: from {} inputs {:?} scheme {}",
                    hex::encode(from),
                    inputs,
                    scheme
                );
            }
            return false;
        }
        let Some(staged) = self.stage_transfer(from, inputs, outputs, height) else {
            if std::env::var_os("SLASH_TRACE_TX").is_some() {
                eprintln!(
                    "[txs] transfer refused at staging: from {} inputs {:?} height {}",
                    hex::encode(from),
                    inputs,
                    height
                );
            }
            return false;
        };
        self.commit_transfer(staged);
        true
    }

    /// Stage and commit a transfer whose validity comes from consensus
    /// rules rather than a transaction signature: the coinbase mint, the
    /// curve pairing mints and payouts, and the proof-of-work slot claim
    /// all reach the engine through this entry. The staged pipeline is
    /// the same one signed transactions use, so ownership, per-page
    /// balance, and the lock cap hold identically.
    pub(crate) fn apply_transfer(
        &mut self,
        from: [u8; 32],
        inputs: &[(u64, u64)],
        outputs: &[Output],
        height: u64,
    ) -> bool {
        let Some(staged) = self.stage_transfer(from, inputs, outputs, height) else {
            return false;
        };
        self.commit_transfer(staged);
        true
    }

    /// Remove an owned span from the live map for a spend, re-checking
    /// ownership with locks first. The balance cache debits in lockstep.
    ///
    /// The cache is checked before any page surgery: on a consistent
    /// state the covering entry always holds at least the span (every
    /// credit lands through the same commit path), but a corrupted or
    /// hand-built state must refuse the spend cleanly instead of
    /// panicking in debug or wrapping to a bogus near-maximum balance in
    /// release. The lazy-default convention — an absent treasury entry
    /// stands for the full unissued supply, an absent entry of any other
    /// owner stands for zero — is applied only for this read; the write
    /// below stores the exact post-debit value.
    pub(crate) fn remove_for_spend(
        &mut self,
        owner: [u8; 32],
        start: u64,
        end: u64,
        height: u64,
    ) -> bool {
        if !self.owns(owner, start, end, height) {
            return false;
        }
        let default = if owner == TREASURY { N } else { 0 };
        let cached = self.balance_cache.get(&owner).copied().unwrap_or(default);
        let width = end - start;
        if cached < width {
            // The cache disagrees with the page map about how much this
            // owner holds; spending would desynchronize them further, so
            // the state refuses instead of mutating.
            return false;
        }
        let pid = page_id(start);
        let page = self.get_page_mut(pid);
        if !page.remove_range(page_offset(start), page_offset(start) + width) {
            return false;
        }
        self.evict_if_pristine(pid);
        *self.balance_cache.entry(owner).or_insert(default) = cached - width;
        true
    }
}
