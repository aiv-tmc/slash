//! src/state/locks.rs — lock-map maintenance and the live-lock index.
//!
//! Expired lock registrations are dead weight in the maps: they no
//! longer bind spending, no longer count against the global lock cap,
//! and no longer carry governance weight. Pruning retires them exactly
//! when the executing height reaches their expiry, never earlier, and
//! always removes the expiry and the creation entry together so the two
//! maps stay one-to-one mirrors of each other.
//!
//! The live-lock count — entries whose expiry is strictly above the
//! executing height — feeds the global cap gate in the transfer engine.
//! `LockIndex` answers that count in O(1) amortized time from a
//! histogram plus a watermark. The index summarizes the lock map only;
//! it never introduces state of its own, and it is exact at every
//! moment: while unbuilt the count scans the lock map directly, and the
//! first mutation settles (or rebuilds) the index, so the count and the
//! map never disagree in either phase.

use super::state::State;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// O(1)-amortized bookkeeping for the number of live lock registrations.
///
/// `hist` counts the entries whose expiry is strictly above `watermark`,
/// bucketed by expiry height; `expired` counts everything at or below
/// the watermark. The live count at height `h` is then
/// `total - expired - sum(hist(watermark, h])`, which the histogram
/// answers without touching the lock map itself.
///
/// `built` marks an index whose contents mirror the lock map exactly.
/// Every state starts unbuilt: the lock maps are authoritative until
/// the engine itself registers or removes entries, and every reader
/// checks the flag and scans the lock map directly while the index is
/// unbuilt, so an untrusted index can never influence consensus.
/// Snapshot recovery anchors the index at the snapshot height before
/// serving any query, which is the one path that builds it eagerly.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LockIndex {
    /// Entries with expiry at or below `watermark`.
    expired: u64,
    /// Live entries with expiry above `watermark`, bucketed by expiry.
    hist: BTreeMap<u64, u64>,
    /// The height through which `expired` has been folded.
    watermark: u64,
    /// Whether `hist`/`expired` mirror the lock map exactly.
    built: bool,
}

impl LockIndex {
    /// The exact number of live registrations at `height`, where `total`
    /// is the current size of the lock map. While the index is unbuilt
    /// the count scans the lock map directly, which is exact by
    /// construction; once built it is O(1) amortized.
    fn live_at(&self, locks: &BTreeMap<u64, u64>, total: u64, height: u64) -> u64 {
        if !self.built {
            return locks.values().filter(|&&expiry| expiry > height).count() as u64;
        }
        let pending: u64 = if height > self.watermark {
            self.hist
                .range((self.watermark.saturating_add(1))..=height)
                .map(|(_, count)| *count)
                .sum()
        } else {
            0
        };
        total.saturating_sub(self.expired).saturating_sub(pending)
    }

    /// Fold every histogram bucket at or below `height` into `expired`
    /// and advance the watermark. The watermark never regresses: callers
    /// at lower heights see a no-op, which is exact because such heights
    /// only query ranges the watermark already passed. The first settle
    /// on an unbuilt index rebuilds it against the lock map; every later
    /// settle folds at most the entries whose expiry fell between the
    /// old watermark and the new height, so each registration is folded
    /// exactly once per watermark crossing.
    pub(super) fn settle(&mut self, locks: &BTreeMap<u64, u64>, height: u64) {
        if !self.built {
            self.rebuild(locks, height);
        }
        if height > self.watermark {
            let folded: Vec<(u64, u64)> = self
                .hist
                .range((self.watermark.saturating_add(1))..=height)
                .map(|(&expiry, &count)| (expiry, count))
                .collect();
            for (expiry, count) in folded {
                self.expired = self.expired.saturating_add(count);
                self.hist.remove(&expiry);
            }
            self.watermark = height;
        }
    }

    /// Register one entry with the given expiry in the books.
    pub(super) fn book(&mut self, expiry: u64) {
        if expiry <= self.watermark {
            self.expired = self.expired.saturating_add(1);
        } else {
            *self.hist.entry(expiry).or_insert(0) += 1;
        }
    }

    /// Withdraw one entry's booking. The entry may sit on either side of
    /// the watermark depending on when it was booked, so the expiry
    /// decides which side of the books it leaves.
    pub(super) fn unbook(&mut self, expiry: u64) {
        if expiry <= self.watermark {
            self.expired = self.expired.saturating_sub(1);
        } else if let Some(count) = self.hist.get_mut(&expiry) {
            *count -= 1;
            if *count == 0 {
                self.hist.remove(&expiry);
            }
        }
    }

    /// Rebuild the books from the lock map at `height`: entries expiring
    /// at or below the height land in `expired`, everything else in the
    /// histogram, and the watermark starts at the height. Called when a
    /// state materialized from storage is about to serve consensus
    /// queries, so every later count is O(1) amortized.
    fn rebuild(&mut self, locks: &BTreeMap<u64, u64>, height: u64) {
        self.hist.clear();
        self.expired = 0;
        self.watermark = height;
        for (&_cell, &expiry) in locks {
            self.book(expiry);
        }
        self.built = true;
    }
}

impl State {
    /// The exact number of lock registrations live at `height`: entries
    /// whose expiry is strictly above the height. This is the global
    /// cap's denominator; it is exact whether or not the index has been
    /// built, and O(1) amortized once it has.
    pub fn live_lock_count(&self, height: u64) -> u64 {
        self.lock_index
            .live_at(&self.locks, self.locks.len() as u64, height)
    }

    /// Force the index to mirror the lock map at `height`. Used where a
    /// state materialized from storage is about to serve consensus
    /// queries (snapshot recovery), so every later count is O(1).
    pub fn rebuild_lock_index(&mut self, height: u64) {
        self.lock_index.rebuild(&self.locks, height);
    }

    /// Retire every lock whose expiry is at or below `height`. The two
    /// lock maps carry one entry per locked cell, so a retired cell
    /// leaves both maps in the same pass; a cell present in only one map
    /// never happens through the transfer engine and is removed here
    /// just the same, keeping the maps consistent for every reader. The
    /// live-lock index is settled to the pruning height first, so every
    /// withdrawn entry books out of the `expired` side deterministically.
    pub fn prune_locks(&mut self, height: u64) {
        self.lock_index.settle(&self.locks, height);
        let expired: Vec<u64> = self
            .locks
            .iter()
            .filter(|(_, &expiry)| expiry <= height)
            .map(|(&cell, _)| cell)
            .collect();
        for cell in expired {
            if let Some(expiry) = self.locks.remove(&cell) {
                self.lock_index.unbook(expiry);
            }
            self.lock_created.remove(&cell);
        }
    }
}
