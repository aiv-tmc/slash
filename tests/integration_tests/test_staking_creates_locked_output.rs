//! `test_staking_creates_locked_output` — a transaction whose change
//! returns to the sender with a lock records that lock in the state, and
//! the locked cells reject spends until the expiry height.

use super::*;
use slash::state;

#[test]
fn test_staking_creates_locked_output() {
    let _tmp = setup_test_dir("staking_creates_locked_output");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (alice_sk, alice) = test_keypair(9);
    let miner = [7u8; 32];
    let height = chain.tip_height() + 1;
    for i in 0..100 {
        mine_cell_for(&mut chain.state, i, alice);
    }

    // Stake cells 0..99 back to the sender, locked until height 1000,
    // paying the one-cell fee from the last cell. A third-party miner
    // collects the subsidy so the balance assertions stay exact.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 100)],
        &[(0, 99, alice), (99, 100, state::FEE_VAULT)],
        1000,
    );
    let b = mine_block_at(&chain, height, 1, miner, 1, vec![tx]);
    chain.apply(b).unwrap();

    // The locked balance is still owned and counted.
    assert_eq!(chain.state.balance(alice), 99);

    // Spending the locked range through the mempool entry point is
    // rejected while the lock is live. The spend covers only the cells
    // alice still owns: cell 99 left her control as the staking fee.
    let early = make_tx_at(
        &chain,
        alice,
        &alice_sk,
        &[(0, 99)],
        &[(0, 98, alice), (98, 99, state::FEE_VAULT)],
        0,
    );
    assert!(!chain.validate_tx(&early));

    // Advance the tip to the expiry height with synthetic headers: lock
    // spendability reads the lock maps and the executing height, never
    // the proof of the blocks in between, so the hundreds of empty
    // interstitial blocks are mock-clock headers, not mined proofs.
    while chain.tip_height() + 1 < 1000 {
        let next = chain.tip_height() + 1;
        chain.blocks.push(slash::chain::Block {
            version: 1,
            prev: [0u8; 32],
            time: 1_700_000_000 + next,
            height: next,
            nonce: 0,
            miner,
            mined_cell: 0,
            txs: vec![],
            fee_claims: vec![],
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        });
    }
    // At the expiry height the locked range is spendable.
    let later = make_tx_at(
        &chain,
        alice,
        &alice_sk,
        &[(0, 99)],
        &[(0, 98, alice), (98, 99, state::FEE_VAULT)],
        0,
    );
    assert!(chain.validate_tx(&later));
}
