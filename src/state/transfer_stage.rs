//! The staging machinery of the transfer pipeline: the touched-page
//! overlay a transfer is speculatively applied to, the staged record of
//! its effects, and the commit that publishes a fully checked transfer
//! to the live map. Staging never mutates the live state; commit runs
//! only after every predicate passed, so publishing is infallible.

use crate::state::page::{Output, Page, R};
use crate::state::params::{page_id, page_offset, page_size, MAX_LOCKED_CELLS, N, TREASURY};
use crate::state::State;
use std::collections::{BTreeMap, HashMap};

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

/// Log one staging refusal when the trace switch is set. The site name is
/// a stable identifier for tests and operators; consensus never depends on
/// whether the line prints.
fn stage_trace(site: &str, from: [u8; 32], inputs: &[(u64, u64)]) {
    if std::env::var_os("SLASH_TRACE_TX").is_some() {
        eprintln!(
            "[txs] stage {site} refused: from {} inputs {:?}",
            hex::encode(from),
            inputs
        );
    }
}

/// The first cell of the span that is not owned by `owner`, with its
/// actual owner: the ownership refusal names the divergence instead of
/// leaving the operator guessing which half of the map disagrees.
fn ownership_probe(
    state: &State,
    owner: [u8; 32],
    s: u64,
    e: u64,
) -> Option<(u64, Option<[u8; 32]>)> {
    (s..e).find_map(|c| match state.get(c) {
        Some((_, _, o)) if o != owner => Some((c, Some(o))),
        None => Some((c, None)),
        _ => None,
    })
}

/// The staged effect of a transfer that passed every check: the touched
/// pages in overlay form, the balance-cache deltas, the lock
/// registrations, and the executing height the lock-creation map reads.
/// Committing is infallible: every predicate ran during staging.
pub(crate) struct StagedTransfer {
    height: u64,
    overlay: BTreeMap<u16, Page>,
    delta_cache: HashMap<[u8; 32], i128>,
    staged_locks: Vec<(u64, u64, u64)>,
}

impl State {
    /// Stage a transfer on a touched-page overlay. Pages the transaction
    /// does not touch are never cloned: the overlay materializes a page
    /// from the live map — or in its pristine treasury form when the live
    /// map has no entry — on first touch and mutates the copy. Ownership
    /// is verified against the live map, which staging never modifies.
    /// Returns None on any refusal, leaving the live state untouched.
    pub(crate) fn stage_transfer(
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
            stage_trace("treasury lock", from, inputs);
            return None;
        }
        // An inert lock — an expiry at or below the executing height —
        // binds nothing: it would sit in the lock maps as dead weight,
        // keep its cells out of canonical selection, and consume cap
        // headroom while never delaying a single spend. Refusing it at
        // the shared staging gate keeps every engine caller — the
        // committing engine and the read-only admission twin alike —
        // from registering one.
        if outputs
            .iter()
            .any(|o| o.lock.is_some_and(|expiry| expiry <= height))
        {
            stage_trace("inert lock", from, inputs);
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
                if std::env::var_os("SLASH_TRACE_TX").is_some() {
                    let probe = ownership_probe(self, from, s, e);
                    eprintln!(
                        "[txs] stage ownership detail: from {} span ({s}, {e}) first_bad_cell={:?} actual_owner={:?}",
                        hex::encode(from),
                        probe.map(|(c, _)| c),
                        probe.and_then(|(_, o)| o)
                    );
                }
                stage_trace("ownership", from, inputs);
                return None;
            }
            let page = overlay.entry(pid).or_insert_with(|| {
                self.pages
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| pristine_page(pid))
            });
            if !page.remove_range(page_offset(s), page_offset(s) + (e - s)) {
                stage_trace("range removal", from, inputs);
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
                stage_trace("per-page balance", from, inputs);
                return None;
            }
        }
        // Global lock cap: the live map plus this transaction's locked
        // cells must stay within MAX_LOCKED_CELLS. Only locks still live
        // at the executing height count — a lock binds spending while its
        // expiry strictly exceeds the height, the predicate the ownership
        // scan uses — so expired entries neither block spending nor
        // consume capacity. The lock index answers the live count in
        // O(1) amortized time and remains exact while it is unbuilt (see
        // `locks.rs`).
        let live_locks = self.live_lock_count(height);
        let new_locked: u64 = staged_locks.iter().map(|(s, e, _)| e - s).sum();
        if live_locks.saturating_add(new_locked) > MAX_LOCKED_CELLS {
            stage_trace("global lock cap", from, inputs);
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
    pub(crate) fn commit_transfer(&mut self, staged: StagedTransfer) {
        // Every cell of every locked output range is registered
        // individually in both global maps — the expiry it binds spending
        // with, and the creation height governance snapshot weight reads.
        // The live-lock index is settled to the executing height first so
        // every booking lands on the correct side of the watermark. A
        // cell the map already holds is an expired registration — the
        // ownership gate refuses live-locked spends — so its booking is
        // withdrawn before the new one lands.
        self.lock_index.settle(&self.locks, staged.height);
        for (s, e, lock) in staged.staged_locks {
            for cell in s..e {
                if let Some(&old) = self.locks.get(&cell) {
                    self.lock_index.unbook(old);
                }
                self.locks.insert(cell, lock);
                self.lock_created.insert(cell, staged.height);
                self.lock_index.book(lock);
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
}

#[cfg(test)]
mod inert_lock_tests {
    use super::*;

    /// A lock whose expiry is at or below the executing height binds
    /// nothing, so the staging gate refuses the whole transfer; an
    /// expiry strictly above the height is admissible. The gate runs in
    /// the shared staging path, so the committing engine and the
    /// read-only admission twin enforce the identical rule.
    #[test]
    fn inert_lock_refused_at_the_executing_height() {
        let (_sk, pk) = crate::crypto::generate_keypair();
        let mut state = State::genesis();
        for c in 0..60 {
            state.mine(1000 + c, pk, 0);
        }
        // Boundary: expiry exactly equal to the executing height is
        // already inert, because a lock binds only while its expiry
        // strictly exceeds the height.
        let expired_outputs = vec![
            Output {
                start: 1000,
                end: 1050,
                to: pk,
                lock: Some(0),
            },
            Output {
                start: 1050,
                end: 1060,
                to: pk,
                lock: None,
            },
        ];
        assert!(
            state
                .stage_transfer(pk, &[(1000, 1060)], &expired_outputs, 0)
                .is_none(),
            "an inert lock must be refused"
        );
        // The refusal leaves no partial mutation behind.
        assert_eq!(state.locks.len(), 0);
        // A strictly live expiry passes the same gate.
        let live_outputs = vec![
            Output {
                start: 1000,
                end: 1050,
                to: pk,
                lock: Some(1),
            },
            Output {
                start: 1050,
                end: 1060,
                to: pk,
                lock: None,
            },
        ];
        assert!(
            state
                .stage_transfer(pk, &[(1000, 1060)], &live_outputs, 0)
                .is_some(),
            "a live lock must be admitted"
        );
    }
}
