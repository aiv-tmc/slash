//! The cached Merkle level stack of one page.
//!
//! A serving node computes the page root rarely and serves proofs often,
//! so the tree is built once per mutation and cached as a vector of
//! levels: level zero holds the leaf hashes in range order, and every
//! following level holds the parents of the previous one, each half the
//! size. The total footprint is roughly two leaf vectors. The pairing
//! rule is fixed and shared with proof verification: odd levels duplicate
//! their last element before pairing, so every level transition consumes
//! exactly one sibling per node.

/// The cached Merkle level stack of one page. `leaves` holds one hash
/// per range in key order; `levels` holds the parent levels above the
/// leaves, each half the size of the one below, ending with the single
/// root hash. The total size is roughly two leaf vectors.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageTree {
    /// Leaf hashes, one per range, in map key order.
    pub(crate) leaves: Vec<[u8; 32]>,
    /// Parent levels above the leaves; the last level holds the root.
    pub(crate) levels: Vec<Vec<[u8; 32]>>,
}

impl PageTree {
    /// Build the full stack from the leaf hashes with the shared
    /// chunking rule: odd levels duplicate their last element before
    /// pairing.
    pub(crate) fn build(leaves: Vec<[u8; 32]>) -> Self {
        let mut levels = Vec::new();
        let mut level = leaves.clone();
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|c| {
                    let left = c[0];
                    let right = c.get(1).copied().unwrap_or(left);
                    let mut h = blake3::Hasher::new();
                    h.update(&left);
                    h.update(&right);
                    *h.finalize().as_bytes()
                })
                .collect();
            levels.push(level.clone());
        }
        Self { leaves, levels }
    }

    /// The root commitment: the single element of the last level, or the
    /// only leaf when the page holds exactly one range.
    pub(crate) fn root(&self) -> [u8; 32] {
        self.levels
            .last()
            .and_then(|top| top.first())
            .copied()
            .unwrap_or_else(|| self.leaves[0])
    }

    /// The sibling of the node at `idx` inside one level: the next
    /// element for even indices, the previous one for odd indices. An
    /// odd-length level duplicates its last element when pairing, so an
    /// even index at the end of the level pairs with itself.
    pub(crate) fn sibling_at(level: &[[u8; 32]], idx: usize) -> [u8; 32] {
        if idx.is_multiple_of(2) {
            level.get(idx + 1).copied().unwrap_or(level[idx])
        } else {
            level[idx - 1]
        }
    }
}
