//! `dev_fee_enforced_by_block_execution_when_active` — one test case of
//! the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fee_enforced_by_block_execution_when_active() {
    // Consensus parity: a block carrying a non-compliant user transaction
    // must fail apply() while the rule is active, and the same
    // transaction with the dev share must apply. The coinbase stays
    // clean because the test zeroes the coinbase dev share (mine_fee_bps),
    // isolating the transaction rule under test. The non-compliant block
    // is assembled by injecting the transaction into an already-valid
    // empty block: the mining fixture by design panics on a template it
    // cannot execute, so a test that expects a rejection builds the
    // invalid block itself.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    chain.state_mut().mine_fee_bps = 0;
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 10_000);
    let non_compliant = vec![
        Output {
            start: 0,
            end: 9_000,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: 9_000,
            end: 9_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 9_999,
            end: 10_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &non_compliant, &chain.chain_id);
    let mut bad = mine_next_block(&chain, [7u8; 32], vec![]);
    bad.txs.push(tx);
    assert!(
        chain.apply(bad).is_err(),
        "consensus must reject a block with a non-compliant user transaction"
    );
    let compliant = vec![
        Output {
            start: 0,
            end: 8_999,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: 8_999,
            end: 9_000,
            to: DEV_FUND,
            lock: None,
        },
        Output {
            start: 9_000,
            end: 9_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 9_999,
            end: 10_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &compliant, &chain.chain_id);
    let good = mine_next_block(&chain, [7u8; 32], vec![tx]);
    chain
        .apply(good)
        .expect("consensus must accept the compliant transaction");
    assert_eq!(chain.state.balance(DEV_FUND), 1);
}
