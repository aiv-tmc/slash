//! The `State` aggregate: layout, construction, and read-only accessors.
//!
//! Everything the state engine needs to answer "who owns this cell, and
//! may it move" lives here: the dirty-page map with lazy pristine
//! reconstruction, the two lock maps, the incremental balance cache, and
//! the economic parameters consensus reads on every block. The lock maps
//! are one-to-one mirrors — the expiry map records until when a lock
//! binds spending, and the creation map records at which height the lock
//! was registered, so governance can weigh only the locks that already
//! existed at a proposal's snapshot height. The mutation primitives live
//! in the sibling files: minting in `minting.rs`, lock maintenance in
//! `locks.rs`, the transfer pipeline in `transfer.rs`, fee sweeps in
//! `fees.rs`, and canonical selection in `selection.rs`.

use super::locks::LockIndex;
use super::page::{Page, R};
use super::params::{page_id, page_offset, page_size, PAGE_SIZE, TREASURY};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// The full ledger state: dirty pages, the lock maps, the incremental
/// balance cache, and the economic parameters that consensus reads on
/// every block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    /// Materialized (dirty) pages keyed by page id. Pristine pages are
    /// absent and are reconstructed as a single treasury range on demand.
    pub pages: BTreeMap<u16, Page>,
    /// Locked cells keyed by global index; the value is the first height
    /// at which the cell becomes spendable again. A sorted map so the
    /// unlocked selection can jump over locked runs in one step.
    pub locks: BTreeMap<u64, u64>,
    /// Lock creation heights keyed by global index, mirroring the expiry
    /// map one entry per locked cell. Governance weight counts a locked
    /// cell only when its creation height is not above the proposal
    /// snapshot, which pins voting weight to the state at proposal
    /// submission: cells locked inside the voting window buy no influence
    /// on the proposal already open.
    pub lock_created: BTreeMap<u64, u64>,
    /// Summarizes the lock map for the O(1) live-lock count the global
    /// cap gate uses; it introduces no state of its own and every count
    /// remains exact while it is unbuilt (see `locks.rs`). Snapshots
    /// persist it; the header does not, and a state materialized from
    /// the header rebuilds it lazily on the first mutation.
    #[serde(default)]
    pub lock_index: LockIndex,
    /// Incremental balance cache: owner to total cells. The std HashMap
    /// is deliberate: storage.rs Header carries the same type, and both
    /// are serialized with bincode, so the in-memory and on-disk layouts
    /// agree.
    pub balance_cache: HashMap<[u8; 32], u64>,
    /// Ed25519 public key of the treasury signer. Every non-coinbase
    /// treasury transaction must verify against this key; an all-zero
    /// key disables treasury spending entirely.
    pub treasury_signer: [u8; 32],
    /// Total cells sold by the treasury on the bonding curve.
    pub total_sold: u64,
    /// Total cells bought back by the treasury.
    pub total_bought_back: u64,
    /// Ed25519 public key of the dev fund signer. Spends from DEV_FUND
    /// must verify against this key; an all-zero key disables dev fund
    /// spending.
    pub dev_fund_signer: [u8; 32],
    /// Dev-fund share of user transaction outputs, in basis points. Zero
    /// means the requirement is switched off entirely; positive values
    /// are enforced only while the DevFund deployment is active.
    pub dev_fee_bps: u64,
    /// Dev-fund share of the coinbase subsidy, in basis points.
    pub mine_fee_bps: u64,
    /// On-chain governance bookkeeping: proposals, votes, and schedules.
    pub dao: crate::dao::DaoState,
    /// Emergency pause state, set only through passed EmergencyPause and
    /// EmergencyResume proposals. While a flag bit is set and the height
    /// is below `emergency_until`, the flagged operation classes are
    /// refused in every block; the chain itself never stops — coinbase,
    /// slot, and fee claims keep executing. The fields ride the ordinary
    /// state transition, so a reorg that re-executes the carrying
    /// proposal re-establishes the pause on the surviving branch.
    pub emergency_flags: u8,
    pub emergency_until: u64,
    /// Pages dirtied since the last persisted save. Every mutation goes
    /// through `get_page_mut`, which records the page here; the storage
    /// layer drains the set to append exactly those pages on the next
    /// save. Not persisted: the set is always empty on a fresh process,
    /// so serializing it would write noise.
    #[serde(skip)]
    pub touched_pages: Arc<Mutex<HashSet<u16>>>,
}

impl State {
    /// The genesis state: no materialized pages (the full supply is
    /// implicit in the pristine pages), empty lock maps, and the default
    /// economic parameters. Genesis is the anchor every node agrees on
    /// without trusting any external data.
    pub fn genesis() -> Self {
        Self {
            pages: BTreeMap::new(),
            locks: BTreeMap::new(),
            lock_created: BTreeMap::new(),
            lock_index: LockIndex::default(),
            balance_cache: HashMap::new(),
            treasury_signer: [0u8; 32],
            total_sold: 0,
            total_bought_back: 0,
            dev_fund_signer: [0u8; 32],
            dev_fee_bps: 0,
            mine_fee_bps: 100,
            dao: crate::dao::DaoState::genesis(),
            emergency_flags: 0,
            emergency_until: 0,
            touched_pages: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// The owner of `cell` together with the page-local coordinates:
    /// `(page_id, page_offset, owner)`. Clean pages are reconstructed as
    /// one pristine treasury range, so ownership of an untouched cell is
    /// answered without materializing anything.
    pub fn get(&self, cell: u64) -> Option<(u16, u64, [u8; 32])> {
        let id = page_id(cell);
        let off = page_offset(cell);
        if let Some(page) = self.pages.get(&id) {
            // The last range starting at or below the offset owns the
            // cell; a materialized page covers every offset, so the
            // lookup always resolves.
            let (_, r) = page.ranges.range(..=off).next_back()?;
            Some((id, off, r.o))
        } else {
            Some((id, off, TREASURY))
        }
    }

    /// Whether `owner` may move the whole span `[start, end)` at `height`.
    /// Ownership must hold for every covered cell, so the span is walked
    /// one owner range at a time and any foreign cell refuses the whole
    /// spend. Any lock whose expiry is strictly above the height blocks
    /// the span regardless of who registered it: a lock binds the cell,
    /// not the owner, so an incoming transfer cannot move a cell whose
    /// lock has not yet run out.
    pub(crate) fn owns(&self, owner: [u8; 32], start: u64, end: u64, height: u64) -> bool {
        let mut cur = start;
        while cur < end {
            let Some((id, off, o)) = self.get(cur) else {
                return false;
            };
            if o != owner {
                return false;
            }
            // The owner range containing this cell: either the range
            // found by the lookup, extended to the page end, or the whole
            // pristine page as one range.
            let range_end = match self.pages.get(&id) {
                Some(page) => page
                    .ranges
                    .range(..=off)
                    .next_back()
                    .map(|(_, r)| r.e)
                    .unwrap_or(off + 1),
                None => page_size(id),
            };
            let seg_end = (id as u64 * PAGE_SIZE + range_end).min(end);
            // The lock map is keyed by global cell index and sorted, so
            // the scan visits exactly the span's locked cells; any lock
            // still binding at the height refuses the segment.
            for (&_locked, &until) in self.locks.range(cur..seg_end) {
                if until > height {
                    return false;
                }
            }
            cur = seg_end.max(cur + 1);
        }
        true
    }

    /// A clone of the page, materializing a pristine page first when the
    /// cell has never been touched. Callers that only read reach for
    /// `get`; this is the shared read path for mutations that need the
    /// full range map.
    pub fn get_page(&self, id: u16) -> Page {
        match self.pages.get(&id) {
            Some(p) => p.clone(),
            None => Page {
                ranges: BTreeMap::from_iter([(
                    0,
                    R {
                        e: page_size(id),
                        o: TREASURY,
                    },
                )]),
                ..Default::default()
            },
        }
    }

    /// Materialize and insert a page into the dirty map, returning a
    /// mutable reference. Every mutation path goes through here so a
    /// dirty page always exists before any range surgery, and the same
    /// chokepoint records the page for the next persisted save: the
    /// append-only page log rewrites exactly the recorded set.
    pub fn get_page_mut(&mut self, id: u16) -> &mut Page {
        self.touched_pages
            .lock()
            .expect("touched-pages lock poisoned")
            .insert(id);
        self.pages.entry(id).or_insert_with(|| {
            let mut p = Page {
                ranges: BTreeMap::new(),
                ..Default::default()
            };
            p.ranges.insert(
                0,
                R {
                    e: page_size(id),
                    o: TREASURY,
                },
            );
            p
        })
    }

    /// Drop a dirty page that has returned to a single pristine treasury
    /// range; the implicit state carries the identical commitment, so
    /// the page no longer needs storage.
    pub fn evict_if_pristine(&mut self, id: u16) {
        if let Some(page) = self.pages.get(&id) {
            if page.ranges.len() == 1 {
                if let Some((&s, r)) = page.ranges.iter().next() {
                    if s == 0 && r.o == TREASURY && r.e == page_size(id) {
                        self.pages.remove(&id);
                    }
                }
            }
        }
    }

    /// Balance of an address from the incremental cache. Untouched
    /// addresses report zero; the cache is updated by the same commit
    /// that publishes staged pages, so a balance always matches the page
    /// map exactly.
    pub fn balance(&self, who: [u8; 32]) -> u64 {
        self.balance_cache.get(&who).copied().unwrap_or(0)
    }

    /// Whether an emergency pause flag is live at `height`: the flag bit
    /// must be set in `emergency_flags` and the height must still be
    /// below `emergency_until`. Every block-validation path asks this
    /// question against the pre-block state, so a pause carried by one
    /// block takes effect from the next and never mid-block.
    pub fn emergency_active(&self, flag: u8, height: u64) -> bool {
        self.emergency_flags & flag != 0 && height < self.emergency_until
    }

    /// The global root: blake3 over every page id paired with its root,
    /// with pristine pages contributing their implicit commitment. The
    /// iteration order of the page id range is deterministic, so the
    /// root is a pure function of the state.
    pub fn global_root(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        for id in 0..super::params::page_count() {
            let root = match self.pages.get(&id) {
                Some(page) => page.merkle_root(),
                None => Page::pristine_root(page_size(id)),
            };
            h.update(&id.to_le_bytes());
            h.update(&root);
        }
        *h.finalize().as_bytes()
    }
}
