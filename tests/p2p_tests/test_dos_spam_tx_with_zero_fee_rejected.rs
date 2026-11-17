//! `test_dos_spam_tx_with_zero_fee_rejected` — one test case of the
//! `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_dos_spam_tx_with_zero_fee_rejected() {
    let _tmp = setup_test_dir("dos_spam");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    // Alice is the public key matching the signing secret used below.
    let (sk, pk) = crypto::generate_keypair();
    let alice = pk;

    // Seed alice by direct mining: a treasury mint without a matching buy
    // payment is invalid by the pairing rules, so it cannot seed a test
    // that is not about pairing.
    for i in 0..50 {
        chain.state_mut().mine(i, alice, 0);
    }

    // A transaction with no fee-vault output is refused by both
    // validation paths: mempool admission rejects it structurally, and
    // a block carrying it is invalid. Injected into an empty block
    // because the mining fixture panics on templates it cannot execute.
    let outputs = vec![state::Output {
        start: 0,
        end: 50,
        to: [2u8; 32],
        lock: None,
    }];
    let tx = make_tx(alice, &sk, &[(0, 50)], &outputs, &chain.chain_id);
    assert!(
        !chain.validate_tx(&tx),
        "a zero-fee transaction is refused at the mempool"
    );
    let mut bad = mine_next_block(&chain, [1u8; 32], vec![]);
    bad.txs.push(tx);
    assert!(
        chain.apply(bad).is_err(),
        "a block carrying a zero-fee transaction is invalid"
    );

    // The same transfer with the mandatory fee applies and moves the
    // balance.
    let outputs = vec![
        state::Output {
            start: 0,
            end: 49,
            to: [2u8; 32],
            lock: None,
        },
        state::Output {
            start: 49,
            end: 50,
            to: state::FEE_VAULT,
            lock: None,
        },
    ];
    let tx = make_tx(alice, &sk, &[(0, 50)], &outputs, &chain.chain_id);
    let b2 = mine_next_block(&chain, [1u8; 32], vec![tx]);
    chain.apply(b2).unwrap();
    assert_eq!(chain.state.balance([2u8; 32]), 49);
}
