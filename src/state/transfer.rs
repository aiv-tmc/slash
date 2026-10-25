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
//! The predicate core lives in the `gates` child module; this file keeps
//! the entry points, the staging overlay, and the commit.

use super::page::Output;
use super::page::{Page, R};
use super::params::{
    page_id, page_offset, page_size, DEV_FUND, FEE_VAULT, MAX_LOCKED_CELLS, MIN,
    MIN_FEE_DENOMINATOR, N, TREASURY,
};
use super::state::State;
use std::collections::{BTreeMap, HashMap};

/// The admission twin of the transfer engine. It runs the identical
/// gates and staging against `&self` and commits nothing, so mempool
/// admission costs work proportional to the transaction's footprint
/// instead of a clone of the whole ledger.
#[path = "transfer_speculative.rs"]
mod speculative;

/// The shared predicate core of the transfer engine. Both the committing
/// entry points here and the admission twin enter through it, so no gate
/// can drift between mempool admission and block execution.
#[path = "transfer_gates.rs"]
mod gates;

/// Materialize a pristine page: a single treasury range spanning the whole
/// page. The overlay constructs it whenever a transaction first touches a
/// page the dirty map does not hold, so ownership edits never observe a
/// missing page. The Merkle cache starts empty: the pristine page is
/// fresh, and the first root computation builds the tree against these
/// exact ranges.
fn pristine_page(pid: u16) -> Page {
    let mut p = Page {
        ranges: BTreeMap::new(),
        ..Default::default()
    };
    p.ranges.insert(
        0,
        R {
            e: page_size(pid),
            o: TREASURY,
        },
    );
    p
}

/// The staged effect of a transfer that passed every check: the touched
/// pages in overlay form, the balance-cache deltas, the lock
/// registrations, and the executing height the lock-creation map reads.
/// Committing is infallible: every predicate ran during staging.
struct StagedTransfer {
    height: u64,
    overlay: BTreeMap<u16, Page>,
    delta_cache: HashMap<[u8; 32], i128>,
    staged_locks: Vec<(u64, u64, u64)>,
}

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

    /// Stage a transfer on a touched-page overlay. Pages the transaction
    /// does not touch are never cloned: the overlay materializes a page
    /// from the live map — or in its pristine treasury form when the live
    /// map has no entry — on first touch and mutates the copy. Ownership
    /// is verified against the live map, which staging never modifies.
    /// Returns None on any refusal, leaving the live state untouched.
    fn stage_transfer(
        &self,
        from: [u8; 32],
        inputs: &[(u64, u64)],
        outputs: &[Output],
        height: u64,
    ) -> Option<StagedTransfer> {
        // Locked outputs to the treasury are structurally impossible and
        // refused here so that no caller of the transfer engine can ever
        // route a lock onto an address no key controls.
        if outputs.iter().any(|o| o.to == TREASURY && o.lock.is_some()) {
            return None;
        }
        let mut overlay: BTreeMap<u16, Page> = BTreeMap::new();
        let mut per_page: BTreeMap<u16, i128> = BTreeMap::new();
        // Balance-cache deltas accumulate alongside the overlay and
        // commit only when every check passed. The treasury debit starts
        // from the full unissued supply: the cache is filled lazily, so a
        // debit before any credit exists starts at N.
        let mut delta_cache: HashMap<[u8; 32], i128> = HashMap::new();
        // Lock registrations stage alongside the pages and commit under
        // the same gate.
        let mut staged_locks: Vec<(u64, u64, u64)> = Vec::new();
        // Verify ownership and remove the inputs from the overlay.
        for &(s, e) in inputs {
            let pid = page_id(s);
            if !self.owns(from, s, e, height) {
                return None;
            }
            let page = overlay.entry(pid).or_insert_with(|| {
                self.pages
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| pristine_page(pid))
            });
            if !page.remove_range(page_offset(s), page_offset(s) + (e - s)) {
                return None;
            }
            *per_page.entry(pid).or_insert(0) -= (e - s) as i128;
            *delta_cache.entry(from).or_insert(0) -= (e - s) as i128;
        }
        // Insert the outputs and accumulate the per-page balance delta.
        for out in outputs {
            let pid = page_id(out.start);
            let page = overlay.entry(pid).or_insert_with(|| {
                self.pages
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| pristine_page(pid))
            });
            page.insert_output(
                page_offset(out.start),
                page_offset(out.start) + (out.end - out.start),
                out.to,
            );
            *per_page.entry(pid).or_insert(0) += (out.end - out.start) as i128;
            *delta_cache.entry(out.to).or_insert(0) += (out.end - out.start) as i128;
            if let Some(lock) = out.lock {
                staged_locks.push((out.start, out.end, lock));
            }
        }
        // Per-page balance: each affected page must net to exactly zero,
        // so cells cannot be smuggled across page boundaries inside one
        // transaction.
        for delta in per_page.values() {
            if *delta != 0 {
                return None;
            }
        }
        // Global lock cap: the live map plus this transaction's locked
        // cells must stay within MAX_LOCKED_CELLS. Only locks still live
        // at the executing height count — a lock binds spending while its
        // expiry strictly exceeds the height, the predicate the ownership
        // scan uses — so expired entries neither block spending nor
        // consume capacity.
        let live_locks = self
            .locks
            .values()
            .filter(|&&expiry| expiry > height)
            .count() as u64;
        let new_locked: u64 = staged_locks.iter().map(|(s, e, _)| e - s).sum();
        if live_locks.saturating_add(new_locked) > MAX_LOCKED_CELLS {
            return None;
        }
        Some(StagedTransfer {
            height,
            overlay,
            delta_cache,
            staged_locks,
        })
    }

    /// Commit a staged transfer. Every predicate ran during staging, so
    /// this is infallible: lock registrations, the overlay merge with
    /// pristine eviction, and the balance-cache deltas land under one
    /// gate.
    fn commit_transfer(&mut self, staged: StagedTransfer) {
        // Every cell of every locked output range is registered
        // individually in both global maps — the expiry it binds spending
        // with, and the creation height governance snapshot weight reads.
        for (s, e, lock) in staged.staged_locks {
            for cell in s..e {
                self.locks.insert(cell, lock);
                self.lock_created.insert(cell, staged.height);
            }
        }
        // Apply the overlay to the live map, dropping pages that returned
        // to the canonical single-range pristine form.
        for (pid, page) in staged.overlay {
            if page.ranges.len() == 1 {
                if let Some((&s, r)) = page.ranges.iter().next() {
                    if s == 0 && r.o == TREASURY && r.e == page_size(pid) {
                        self.pages.remove(&pid);
                        continue;
                    }
                }
            }
            self.pages.insert(pid, page);
        }
        // Commit the balance cache deltas. A treasury debit starts from
        // the full unissued supply; every other address starts from zero.
        for (owner, d) in staged.delta_cache {
            let default = if owner == TREASURY { N } else { 0 };
            let entry = self.balance_cache.entry(owner).or_insert(default);
            *entry = (*entry as i128 + d) as u64;
        }
    }

    /// Remove an owned span from the live map for a spend, re-checking
    /// ownership with locks first. The balance cache debits in lockstep.
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
        let pid = page_id(start);
        let page = self.get_page_mut(pid);
        if !page.remove_range(page_offset(start), page_offset(start) + (end - start)) {
            return false;
        }
        self.evict_if_pristine(pid);
        let default = if owner == TREASURY { N } else { 0 };
        *self.balance_cache.entry(owner).or_insert(default) -= end - start;
        true
    }
}
