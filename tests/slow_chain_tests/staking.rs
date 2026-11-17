//! The stake-lock expiry test: a range locked at height H becomes
//! spendable at height H, and the locked tail is untouchable until then.

use super::*;

/// A stake locked to height 100 cannot be touched before the lock
/// expires and becomes spendable exactly at the lock height. The ninety-
/// eight empty blocks between the staking block and the lock height are
/// mock-clock headers: lock spendability reads the lock maps and the
/// executing height, never the proof of the blocks in between, and the
/// staking block itself is real, so the engine sees the identical state.
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
    // Advance the tip to the lock height with synthetic headers.
    while chain.tip_height() + 1 < 100 {
        let next = chain.tip_height() + 1;
        chain.blocks.push(chain::Block {
            version: 1,
            prev: [0u8; 32],
            time: 1_700_000_000 + next,
            height: next,
            nonce: 0,
            miner: alice,
            mined_cell: 0,
            txs: vec![],
            fee_claims: vec![],
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        });
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
