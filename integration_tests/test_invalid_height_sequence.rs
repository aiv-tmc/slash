//! `test_invalid_height_sequence` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_invalid_height_sequence() {
    // A block whose declared height does not match the position its
    // linkage implies is invalid.
    let chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    let mut b = mine_valid_block(&chain, 2, miner, chain.next_difficulty());
    b.height = 5;
    assert!(!chain.validate_block(&b));
}
