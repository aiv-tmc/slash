//! `test_coinbase_canonical_inputs_reproduce_root` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_coinbase_canonical_inputs_reproduce_root() {
    // The coinbase inputs must be the canonical select_single treasury
    // range. A hand-crafted range with the same total amount but a different
    // position must fail validation because every node reproduces the
    // identical inputs to agree on page roots.
    reset_testnet();
    let chain = slash::chain::Chain::genesis();
    let miner = [7u8; 32];
    let mut block = prepared_tail_block(&chain, miner, true);
    let cb = &mut block.txs[0];
    let (s, e) = cb.inputs[0];
    cb.inputs = vec![(s + PAGE_SIZE, e + PAGE_SIZE)];
    let block = chain.prepare_block(block);
    assert!(!chain.validate_block(&block));
}
