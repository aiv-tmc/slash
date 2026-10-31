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
        mine_cell_for(chain.state_mut(), i, alice);
    }
    // The outputs pack the spent input exactly: 25 to bob, 24 change,
    // 1 fee. Alice keeps her remaining 50 unspent cells outside this
    // transaction.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50)],
        &[(0, 25, bob), (25, 49, alice), (49, 50, state::FEE_VAULT)],
        0,
    );
    let b = mine_block_at(&chain, chain.tip_height() + 1, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(bob), 25);
    assert_eq!(chain.state.balance(alice), 99 - 25 + 500 + 1);
}

#[test]
fn test_transfer_rejects_bad_signature() {
    let _tmp = setup_test_dir("bad_sig");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (_alice_sk, alice) = test_keypair(1);
    let (_bob_sk, bob) = test_keypair(2);
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    // A signature from the wrong key must not authorize the spend. bob is
    // the well-formed-output owner; alice is not.
    let tx = chain::Tx {
        from: alice,
        inputs: vec![(0, 100)],
        outputs: vec![
            chain::Output {
                start: 0,
                end: 25,
                to: bob,
                lock: None,
            },
            chain::Output {
                start: 25,
                end: 49,
                to: alice,
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
