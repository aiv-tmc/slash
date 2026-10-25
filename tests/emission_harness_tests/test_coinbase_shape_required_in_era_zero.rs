//! `test_coinbase_shape_required_in_era_zero` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_coinbase_shape_required_in_era_zero() {
    // A block in era 0 without a coinbase transaction is invalid.
    reset_testnet();
    let chain = slash::chain::Chain::genesis();
    let mut block = prepared_tail_block(&chain, [7u8; 32], false);
    block.txs = vec![];
    let block = chain.prepare_block(block);
    assert!(!chain.validate_block(&block));
}
