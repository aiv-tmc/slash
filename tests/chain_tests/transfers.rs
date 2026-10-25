//! Transfer tests: balance movement, signature and ownership failures.

use super::*;

#[test]
fn test_transfer_moves_balance() {
    let _tmp = setup_test_dir("transfer");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    let (_bob_sk, bob) = test_keypair(2);
    for i in 0..100 {
        mine_cell_for(&mut chain.state, i, alice);
    }
    // The outputs pack the inputs exactly: 25 to bob, 24 change, 1 fee.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50)],
        &[(0, 25, bob), (25, 49, alice), (49, 50, state::FEE_VAULT)],
        0,
    );
    let b = mine_next_block(&chain, alice, vec![tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(bob), 25);
    // alice: 100 mined - 50 spent + 24 change + 500 coinbase + 1 slot.
    assert_eq!(chain.state.balance(alice), 100 - 50 + 24 + 500 + 1);
    assert_eq!(chain.state.balance(state::FEE_VAULT), 1);
}

#[test]
fn test_wrong_owner_rejected() {
    // Signature from alice, declared sender bob: ownership and signature
    // checks must fail before any state mutation.
    let _tmp = setup_test_dir("wrong_owner");
    reset_testnet();
    let chain = chain::Chain::genesis();
    let (alice_sk, _alice) = test_keypair(1);
    let (_bob_sk, bob) = test_keypair(2);
    let tx = make_tx_for(
        &chain,
        bob,
        &alice_sk,
        &[(0, 50)],
        &[(0, 49, [9u8; 32]), (49, 50, state::FEE_VAULT)],
        0,
    );
    assert!(!chain.validate_tx(&tx));
}

#[test]
fn test_bad_signature_rejected() {
    let _tmp = setup_test_dir("bad_sig");
    reset_testnet();
    let chain = chain::Chain::genesis();
    let (_alice_sk, alice) = test_keypair(1);
    let tx = chain::Tx {
        from: alice,
        inputs: vec![(0, 50)],
        outputs: vec![
            chain::Output {
                start: 0,
                end: 49,
                to: [9u8; 32],
                lock: None,
            },
            chain::Output {
                start: 49,
                end: 50,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        sig: vec![1u8; 64],
        scheme: 0,
        memo: None,
    };
    assert!(!chain.validate_tx(&tx));
}
