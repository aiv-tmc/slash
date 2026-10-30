//! `test_block_hash_deterministic` — one test case of the `integration_tests` suite.
//!
//! The fixture `mine_valid_block` helper comes from the crate root via
//! `use super::*;`; the `Chain` type is imported explicitly so this file
//! compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_block_hash_deterministic() {
    // The same block content hashes to the same value across calls; a
    // single-field change alters the hash completely. The chain is only
    // ever read here, so the binding is immutable.
    let chain = Chain::genesis();
    let miner = [1u8; 32];
    let b = mine_valid_block(&chain, 1, miner, chain.next_difficulty());
    let h1 = chain.block_hash(&b);
    let h2 = chain.block_hash(&b);
    assert_eq!(h1, h2);
    let mut b2 = b.clone();
    b2.nonce += 1;
    assert_ne!(chain.block_hash(&b2), h1);
}
