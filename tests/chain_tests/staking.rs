//! Staking tests: a locked output cannot be spent before its height and
//! becomes spendable exactly at it, with the heights between bridged by
//! synthetic headers instead of mined blocks.

use super::*;

#[test]
fn test_stake_locks_until_height() {
    let _tmp = setup_test_dir("stake_lock");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx = make_tx_at(
        &chain,
        alice,
        &alice_sk,
        &[(0, 100)],
        &[(0, 99, alice), (99, 100, state::FEE_VAULT)],
        1000,
    );
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    // alice: 99 cells of locked change + 500 coinbase + 1 slot mint; the
    // 100 funded cells moved through the transaction and only the locked
    // change, the subsidy, and the slot remain on her balance.
    assert_eq!(chain.state.balance(alice), 99 + 500 + 1);
    // Spending the locked range before its height is refused.
    let early = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 99)],
        &[(0, 98, alice), (98, 99, state::FEE_VAULT)],
        0,
    );
    assert!(!chain.validate_tx(&early));
}

#[test]
fn test_stake_spendable_at_lock_height() {
    let _tmp = setup_test_dir("stake_unlock");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx = make_tx_at(
        &chain,
        alice,
        &alice_sk,
        &[(0, 100)],
        &[(0, 99, alice), (99, 100, state::FEE_VAULT)],
        100,
    );
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    // Advance the tip to the lock height with synthetic headers: the
    // spendability check reads only the lock maps and the executing
    // height, so the empty blocks between the staking block and the lock
    // height are mock-clock headers, not mined proofs.
    while chain.tip_height() + 1 < 100 {
        push_synthetic_headers(&mut chain, 1, chain::TARGET_BLOCK_TIME_SECS);
    }
    // At height 100 the locked range is spendable: cell 99 belongs to the
    // fee vault since the staking block, so the spend covers only the
    // 99 cells alice still owns.
    let later = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 99)],
        &[(0, 98, alice), (98, 99, state::FEE_VAULT)],
        0,
    );
    assert!(chain.validate_tx(&later));
}
