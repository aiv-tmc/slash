//! `test_empty_block_valid` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_empty_block_valid() {
    // A block carrying no transactions beyond the mandatory coinbase is
    // valid: the PoW slot mint alone completes the payload. The coinbase
    // is mandatory while the subsidy is positive, so the block is built
    // through the mining fixture, which attaches it.
    let mut chain = Chain::genesis();
    let miner = [1u8; 32];
    let b = mine_next_block(&chain, miner, vec![]);
    chain.apply(b).unwrap();
    assert_eq!(chain.tip_height(), 1);
}
