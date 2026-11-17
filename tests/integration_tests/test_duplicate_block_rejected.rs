//! `test_duplicate_block_rejected` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_duplicate_block_rejected() {
    // Re-delivering an already-applied block returns Duplicate rather than
    // extending anything twice.
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    let b = mine_valid_block(&chain, 1, miner, chain.next_difficulty());
    let b = chain.prepare_block(b);
    chain.apply(b.clone()).unwrap();

    let result = chain.process_block(b);
    assert!(matches!(result, Err(chain::BlockError::Duplicate)));
}
