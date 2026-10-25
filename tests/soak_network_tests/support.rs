//! Shared infrastructure of the network soak: a deterministic
//! pseudo-random generator and the in-memory node wrapper that the
//! builders and the driver operate on.

use super::*;

/// Deterministic xorshift64 generator. The fixed seed keeps the soak
/// reproducible: a failing run can be replayed operation by operation.
pub struct Rng(pub u64);

impl Rng {
    /// Advance the generator and return the next 64-bit value. The method
    /// is named `advance` rather than `next` so it never reads as the
    /// `Iterator` protocol: the generator is a plain state machine, not
    /// an iterator adapter.
    pub fn advance(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform value in `[0, n)`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.advance() % n
    }
}

/// One in-memory node: a chain and the node's deterministic miner
/// keypair, addressed by a display name used in failure messages.
pub struct Node {
    pub name: &'static str,
    pub chain: slash::chain::Chain,
    pub miner_sk: [u8; 32],
    pub miner_pk: [u8; 32],
}

impl Node {
    /// Create a node on a fresh treasury genesis with a miner keypair
    /// derived deterministically from the node name.
    pub fn new(name: &'static str) -> Self {
        let mut seed = [0u8; 32];
        for (i, b) in name.as_bytes().iter().enumerate() {
            seed[i % 32] ^= b;
        }
        let miner_sk = seed;
        let miner_pk = slash::crypto::public_from_secret(&seed);
        Node {
            name,
            chain: treasury_genesis(),
            miner_sk,
            miner_pk,
        }
    }

    /// Build the next block carrying `txs` for this node's miner and
    /// apply it, returning the applied block so the driver can track
    /// heights.
    pub fn mine(&mut self, txs: Vec<slash::chain::Tx>) -> slash::chain::Block {
        let b = mine_next_block(&self.chain, self.miner_pk, txs);
        self.chain.apply(b.clone()).expect("soak block must apply");
        b
    }
}

/// Recompute the total supply by walking every page's ownership ranges
/// and summing their widths; the result must always equal N, pinning the
/// cell-conservation invariant independently of the balance cache. The
/// ownership ranges are keyed by start cell; each range records its end
/// and owner, so the width is `end - key`.
pub fn assert_invariants(node: &mut Node) {
    let mut total = 0u128;
    for id in 0..slash::state::page_count() {
        let page = node.chain.state.get_page(id);
        for (&start, r) in page.ranges.iter() {
            total += (r.e - start) as u128;
        }
    }
    assert_eq!(
        total,
        slash::state::N as u128,
        "{}: conservation violated",
        node.name
    );
    let tip = node.chain.tip_hash();
    assert!(!tip.is_empty(), "{}: empty tip hash", node.name);
}
