//! `test_coinbase_outputs_only_to_miner` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_coinbase_outputs_only_to_miner() {
    // Every coinbase output must go to the block miner; an output routed to
    // a third party makes the block invalid.
    reset_testnet();
    let chain = slash::chain::Chain::genesis();
    let miner = [7u8; 32];
    let mut block = prepared_tail_block(&chain, miner, true);
    let cb = &mut block.txs[0];
    let (s, e) = cb.inputs[0];
    cb.outputs = vec![
        chain::Output {
            start: s,
            end: e - 100,
            to: miner,
            lock: None,
        },
        chain::Output {
            start: e - 100,
            end: e,
            to: [9u8; 32],
            lock: None,
        },
    ];
    let block = chain.prepare_block(block);
    assert!(!chain.validate_block(&block));
}
