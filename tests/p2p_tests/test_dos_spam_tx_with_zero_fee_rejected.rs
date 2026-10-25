//! `test_dos_spam_tx_with_zero_fee_rejected` — one test case of the `p2p_tests` suite.
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

    // Seed alice with 50 cells; the seed block is mined by the treasury so
    // the reward cell can never invalidate the seed transaction.
    let seed = chain::Output {
        start: 0,
        end: 50,
        to: alice,
        lock: None,
    };
    let seed_tx = chain::Tx {
        from: state::TREASURY,
        inputs: vec![(0, 50)],
        outputs: vec![seed],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    let b1 = mine_next_block(&chain, state::TREASURY, vec![seed_tx]);
    chain.apply(b1).unwrap();

    let outputs = vec![state::Output {
        start: 0,
        end: 50,
        to: [2u8; 32],
        lock: None,
    }];
    let tx = make_tx(alice, &sk, &[(0, 50)], &outputs, &chain.chain_id);

    let b2 = mine_next_block(&chain, state::TREASURY, vec![tx]);
    chain.apply(b2).unwrap();
    assert_eq!(chain.state.balance([2u8; 32]), 50);
}
