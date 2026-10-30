//! `test_era_eight_to_nine_transition` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_era_eight_to_nine_transition() {
    // The era 8 -> 9 transition pins four consensus behaviors at once: the
    // last era-8 block still requires a one-cell coinbase, the first era-9
    // block must omit the coinbase entirely, a payload-free era-9 block funds
    // the miner purely from the PoW slot cell, and an unsigned treasury
    // transaction at payload position 0 is rejected by the treasury
    // signature rule rather than slipping through the coinbase-shape gate
    // (which is disabled at zero subsidy).
    let last_eight = 9 * slash::chain::HALVING_INTERVAL - 1;
    let first_nine = 9 * slash::chain::HALVING_INTERVAL;

    // Era-8 tail: coinbase required and worth exactly one cell.
    let mut chain = chain_with_next_height(last_eight);
    let miner = [7u8; 32];
    let b8 = prepared_tail_block(&chain, miner, true);
    assert_eq!(b8.txs.len(), 1);
    assert_eq!(
        b8.txs[0]
            .outputs
            .iter()
            .map(|o| o.end - o.start)
            .sum::<u64>(),
        1
    );
    chain.apply(b8).expect("era-8 tail must apply");

    // Era-9 head: empty payload, no coinbase, miner funded by the slot only.
    let mut chain = chain_with_next_height(first_nine);
    let b9 = prepared_tail_block(&chain, miner, false);
    assert!(b9.txs.is_empty());
    chain.apply(b9.clone()).expect("era-9 head must apply");
    assert_eq!(chain.state.balance(miner), 1);

    // Unsigned treasury transaction at position 0 of an era-9 block is
    // rejected (no coinbase carve-out exists at zero subsidy).
    let user = [9u8; 32];
    let (s, e) = chain.state.select_single(TREASURY, 50).unwrap();
    let tx = Tx {
        from: TREASURY,
        inputs: vec![(s, e)],
        outputs: vec![chain::Output {
            start: s,
            end: e,
            to: user,
            lock: None,
        }],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    let mut bad = prepared_tail_block(&chain, miner, false);
    bad.txs = vec![tx];
    let bad = chain.prepare_block(bad);
    assert!(!chain.validate_block(&bad));
}
