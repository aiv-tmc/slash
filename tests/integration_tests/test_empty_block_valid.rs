//! `test_empty_block_valid` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_empty_block_valid() {
    // A block with no transactions beyond the implicit coinbase rules is
    // valid at every era: the PoW slot mint alone is a complete payload.
    let mut chain = Chain::genesis();
    let miner = [1u8; 32];
    let b = mine_valid_block(&chain, 1, miner, chain.next_difficulty());
    let b = chain.prepare_block(b);
    chain.apply(b.clone()).unwrap();
    assert_eq!(chain.tip_height(), 1);
}
