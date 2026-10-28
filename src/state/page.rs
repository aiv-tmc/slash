//! One page of the ledger: the sorted range map, the cached Merkle
//! commitment, and the ownership mutation primitives. Pages hold at most
//! PAGE_SIZE cells, addressed by page-relative offsets; global cell
//! indices translate to pages through `page_id`. A pristine page is the
//! single canonical treasury range, so a dirty page that returns to it
//! is dropped entirely — the commitment of a missing page is recomputed
//! from the pristine form on demand.
//!
//! The Merkle machinery lives in two focused child modules: `tree_impl`
//! holds the cached level stack, and `proof_impl` holds the public proof
//! structure and its verification. Both are plain data over the range
//! map; this file keeps the Page type and every ownership operation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "page_proof.rs"]
mod proof_impl;
#[path = "page_tree.rs"]
mod tree_impl;

pub use proof_impl::{verify_page_proof, PageProof};
pub(crate) use tree_impl::PageTree;

/// One range inside a page map: the exclusive end offset and the owner.
/// The start offset is the map key, so the map is always dense and
/// sorted.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct R {
    /// Exclusive end offset inside the page.
    pub e: u64,
    /// Owner of every cell in the range.
    pub o: [u8; 32],
}

/// A transaction output: a half-open global range and the recipient.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    /// Global start cell, inclusive.
    pub start: u64,
    /// Global end cell, exclusive.
    pub end: u64,
    /// Recipient.
    pub to: [u8; 32],
    /// If `Some(H)`, the cells are locked until height `H`.
    pub lock: Option<u64>,
}

/// One ledger page: the sorted range map plus the cached Merkle
/// commitment. The cache is skipped on serialization — it rebuilds on
/// first use after load.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Page {
    /// Sorted, gapless, non-overlapping ranges keyed by start offset.
    pub ranges: BTreeMap<u64, R>,
    /// The cached Merkle level stack of the range map. Skipped on
    /// serialization; rebuilds on first use after load.
    #[serde(skip)]
    pub(crate) tree: std::sync::RwLock<Option<PageTree>>,
}

/// Clone copies the range map and starts the cache empty: the cached
/// tree is a pure function of the ranges, so a fresh cache on the clone
/// recomputes byte-identical values on first use while keeping clones
/// cheap and never sharing mutable cache state between the two copies.
impl Clone for Page {
    fn clone(&self) -> Self {
        Self {
            ranges: self.ranges.clone(),
            ..Default::default()
        }
    }
}

/// Equality is equality of the range map. The cached tree is a pure
/// function of the ranges, so two pages with equal maps carry equal
/// commitments regardless of which one has a warm cache.
impl PartialEq for Page {
    fn eq(&self, other: &Self) -> bool {
        self.ranges == other.ranges
    }
}

impl Page {
    /// Run `f` against the cached level stack, building it on first use
    /// after a mutation or a load. The closure runs while the guard is
    /// held, so `f` must only read the tree and must not re-enter this
    /// page's cache. The cheap emptiness check takes a scoped read that
    /// is dropped before the exclusive write, so the two never overlap:
    /// the standard RwLock is neither reentrant nor upgradable. A
    /// poisoned lock is recovered with `into_inner` — the cache is a
    /// pure function of the ranges, so the worst a poisoned guard can
    /// hold is a stale value that the next invalidation discards, and
    /// consensus must not abort on a bookkeeping panic.
    fn with_page_tree<R>(&self, f: impl FnOnce(&PageTree) -> R) -> R {
        {
            let cache = self.tree.read().unwrap_or_else(|e| e.into_inner());
            if let Some(tree) = cache.as_ref() {
                return f(tree);
            }
        }
        let mut cache = self.tree.write().unwrap_or_else(|e| e.into_inner());
        if cache.is_none() {
            let leaves: Vec<[u8; 32]> = self
                .ranges
                .iter()
                .map(|(&s, r)| Self::leaf_hash(s, r.e, &r.o))
                .collect();
            *cache = Some(PageTree::build(leaves));
        }
        f(cache.as_ref().expect("built above"))
    }

    /// Drop the cached level stack. Every mutation of the range map
    /// calls this before returning, so the next read rebuilds against
    /// the fresh ranges; refusal paths leave the cache untouched because
    /// the map did not change.
    fn invalidate_tree(&self) {
        *self.tree.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The Merkle root of the range map: blake3 leaves over
    /// start || end || owner, hashed up in deterministic pairs.
    pub fn merkle_root(&self) -> [u8; 32] {
        if self.ranges.is_empty() {
            panic!("merkle_root of an empty page");
        }
        self.with_page_tree(|tree| tree.root())
    }

    /// The root commitment of a pristine (never-touched) page: a single
    /// range spanning the whole page owned by the treasury. Materialized
    /// pages of that form are evicted from memory and disk, so this is
    /// also the commitment every clean page contributes to the global
    /// root without ever being stored.
    pub fn pristine_root(page_size: u64) -> [u8; 32] {
        Self::leaf_hash(0, page_size, &super::params::TREASURY)
    }

    /// The blake3 leaf hash: start || end || owner, all little-endian.
    fn leaf_hash(start: u64, end: u64, owner: &[u8; 32]) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&start.to_le_bytes());
        h.update(&end.to_le_bytes());
        h.update(owner);
        *h.finalize().as_bytes()
    }

    /// Build a Merkle proof for the range containing the given page-local
    /// offset. Returns None when the offset is outside every range, which
    /// for the final page also covers offsets beyond its shortened span.
    pub fn merkle_proof(&self, offset: u64) -> Option<PageProof> {
        let (&range_start, range) = self.ranges.range(..=offset).next_back()?;
        if offset >= range.e {
            return None;
        }
        let leaf_index = self
            .ranges
            .keys()
            .position(|k| *k == range_start)
            .expect("range came from the map");
        // The cached level stack serves the whole path: the leaf hash
        // comes straight from the leaf vector and every sibling is read
        // from the level the node sits in, so no range is re-hashed per
        // proof request. The walk follows the cached parent chain: one
        // sibling per level transition, exactly the reductions the root
        // was built with — a single-range page has no transitions and
        // gets an empty path whose root is the leaf itself. Each level
        // uses the same pairing the root was built with, so the path
        // verifies against the cached root by construction.
        Some(self.with_page_tree(|tree| {
            let leaf_hash = tree.leaves[leaf_index];
            let mut siblings = Vec::with_capacity(tree.levels.len());
            let mut idx = leaf_index;
            let mut level: &[[u8; 32]] = &tree.leaves;
            for parent in &tree.levels {
                siblings.push(PageTree::sibling_at(level, idx));
                idx /= 2;
                level = parent;
            }
            PageProof {
                leaf_hash,
                siblings,
                leaf_index,
            }
        }))
    }

    /// Insert [start, end) owned by `owner`, merging with adjacent ranges
    /// of the same owner. The caller guarantees the span is currently
    /// free, so the insert can never create an overlap.
    pub fn insert_output(&mut self, start: u64, end: u64, owner: [u8; 32]) {
        self.invalidate_tree();
        self.ranges.insert(start, R { e: end, o: owner });
        self.merge_at(start);
    }

    /// Merge the range at `at` with same-owner neighbours on both sides.
    /// Keeps the map gapless with distinct adjacent owners; called after
    /// every insert so merges stay local and cheap.
    fn merge_at(&mut self, at: u64) {
        // Merge left.
        if let Some((&ps, pr)) = self.ranges.range(..at).next_back() {
            if pr.o == self.ranges[&at].o && pr.e == at {
                let end = self.ranges[&at].e;
                self.ranges.insert(ps, R { e: end, o: pr.o });
                self.ranges.remove(&at);
            }
        }
        // Merge right, locating the current key again because a left
        // merge may have re-keyed it.
        let cur = *self.ranges.range(..=at).next_back().unwrap().0;
        if let Some((&ns, nr)) = self.ranges.range(cur..).nth(1) {
            if nr.o == self.ranges[&cur].o && ns == self.ranges[&cur].e {
                let end = nr.e;
                self.ranges.insert(cur, R { e: end, o: nr.o });
                self.ranges.remove(&ns);
            }
        }
    }

    /// Owner of the range containing the page-local offset, if any.
    pub fn owner_of(&self, offset: u64) -> Option<[u8; 32]> {
        let (_, r) = self.ranges.range(..=offset).next_back()?;
        if offset < r.e {
            Some(r.o)
        } else {
            None
        }
    }

    /// Remove [start, end) from the range map. Every crossed range must
    /// be owned by the same party; the caller verifies ownership before
    /// invoking this through remove_for_spend. Boundary ranges keep their
    /// surviving head and tail, with the tail re-keyed at the removal
    /// end, so the map stays gapless. Returns false — leaving the map
    /// untouched — when the span is not covered by one contiguous run of
    /// ranges.
    pub fn remove_range(&mut self, start: u64, end: u64) -> bool {
        // Snapshot every overlapped range so a refusal restores exact keys.
        let mut affected: Vec<(u64, R)> = Vec::new();
        for (&s, r) in self.ranges.range(..end) {
            if r.e > start {
                affected.push((s, *r));
            }
        }
        if affected.is_empty() {
            return false;
        }
        // Contiguity: the first overlapped range must begin at or before
        // `start`, each following range must begin exactly where the
        // previous ended, and the last must reach at least `end`.
        if affected[0].0 > start {
            return false;
        }
        for w in affected.windows(2) {
            if w[1].0 != w[0].1.e {
                return false;
            }
        }
        let last = *affected.last().expect("non-empty after the check above");
        if last.1.e < end {
            return false;
        }
        for &(s, _) in &affected {
            self.ranges.remove(&s);
        }
        let head_owner = affected[0].1.o;
        if affected[0].0 < start {
            self.ranges.insert(
                affected[0].0,
                R {
                    e: start,
                    o: head_owner,
                },
            );
        }
        if last.1.e > end {
            self.ranges.insert(
                end,
                R {
                    e: last.1.e,
                    o: last.1.o,
                },
            );
        }
        self.invalidate_tree();
        true
    }
}
