//! Fee tests: accumulated fees are claimable by the miner only, and the
//! miner's revenue includes the perpetual slot mint alongside the claim.

use super::*;

#[test]
fn test_miner_claims_accumulated_fees() {
    let _tmp = setup_test_dir("miner_claims");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (alice_sk, alice) = test_keypair(1);
    let miner = test_keypair(7).1;
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 100)],
        &[
            (0, 50, [9u8; 32]),
            (50, 99, alice),
            (99, 100, state::FEE_VAULT),
        ],
        0,
    );
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(state::FEE_VAULT), 1);

    let height = chain.tip_height() + 1;
    let mut b = mine_block_at(&chain, height, 2, miner, 1, vec![]);
    // Claim the accumulated fee.
    b.fee_claims = vec![chain::Output {
        start: 99,
        end: 100,
        to: miner,
        lock: None,
    }];
    let b = chain.prepare_block(b);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(state::FEE_VAULT), 0);
    // miner: 500 coinbase + 1 slot + 1 claimed fee.
    assert_eq!(chain.state.balance(miner), 500 + 1 + 1);
}

#[test]
fn test_fee_claim_to_third_party_rejected() {
    let _tmp = setup_test_dir("third_party_claim");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (alice_sk, alice) = test_keypair(1);
    let miner = test_keypair(7).1;
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 100)],
        &[
            (0, 50, [9u8; 32]),
            (50, 99, alice),
            (99, 100, state::FEE_VAULT),
        ],
        0,
    );
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();

    let height = chain.tip_height() + 1;
    let mut b = mine_block_at(&chain, height, 2, miner, 1, vec![]);
    // A claim paying someone other than the block miner must be refused.
    b.fee_claims = vec![chain::Output {
        start: 99,
        end: 100,
        to: [8u8; 32],
        lock: None,
    }];
    let b = chain.prepare_block(b);
    assert!(chain.apply(b).is_err());
}
