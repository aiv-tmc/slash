//! Sell-side rejections: overpriced payouts, double payouts, payouts
//! without a sell, and unpaired sells burning their cells are pinned.

use super::*;

#[test]
fn test_treasury_payout_above_sell_price_rejected() {
    // A payout exceeding the curve sell price would drain the vault and
    // must invalidate the block.
    let _tmp = setup_test_dir("treasury_overpay");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();
    let buy = make_tx(
        seller,
        &sk,
        &[(0, 400)],
        &[
            state::Output {
                start: 0,
                end: 100,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 100,
                end: 398,
                to: seller,
                lock: None,
            },
            state::Output {
                start: 398,
                end: 400,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint = mint_tx(&chain, 100, seller);
    let b2 = mine_next_block(&chain, state::TREASURY, vec![buy, mint]);
    chain.apply(b2).unwrap();

    let sell = make_tx(
        seller,
        &sk,
        &[(100, 201)],
        &[
            state::Output {
                start: 100,
                end: 200,
                to: state::TREASURY,
                lock: None,
            },
            state::Output {
                start: 200,
                end: 201,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    // sell_price(100, 100) is 98; refunding 99 overpays the seller.
    // The scratch copy detaches the value from the chain's shared `Arc`,
    // so the simulated vault funding never touches the live state.
    let mut sim = (*chain.state).clone();
    sim.mine(0, state::TREASURY_VAULT, 2);
    sim.mine(1, state::TREASURY_VAULT, 2);
    let payout = payout_tx_on(&sim, &chain.chain_id, 99, seller);
    let b3 = mine_next_block(&chain, state::TREASURY, vec![sell, payout]);
    assert!(!chain.validate_block(&b3));
}

#[test]
fn test_treasury_double_payout_against_single_sell_rejected() {
    // One sell funds exactly one payout; a second payout has no pending
    // sell and invalidates the block.
    let _tmp = setup_test_dir("treasury_double_payout");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();
    let buy = make_tx(
        seller,
        &sk,
        &[(0, 400)],
        &[
            state::Output {
                start: 0,
                end: 100,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 100,
                end: 398,
                to: seller,
                lock: None,
            },
            state::Output {
                start: 398,
                end: 400,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint = mint_tx(&chain, 100, seller);
    let b2 = mine_next_block(&chain, state::TREASURY, vec![buy, mint]);
    chain.apply(b2).unwrap();

    let sell = make_tx(
        seller,
        &sk,
        &[(100, 201)],
        &[
            state::Output {
                start: 100,
                end: 200,
                to: state::TREASURY,
                lock: None,
            },
            state::Output {
                start: 200,
                end: 201,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let payout_one = payout_tx_on(&chain.state, &chain.chain_id, 98, seller);
    // Fund a second vault input so the second payout is structurally
    // spendable; it must still be rejected for lack of a pending sell.
    // The scratch copy detaches the value from the chain's shared `Arc`,
    // so the simulated vault funding never touches the live state.
    let mut sim = (*chain.state).clone();
    sim.mine(500, state::TREASURY_VAULT, 2);
    sim.mine(501, state::TREASURY_VAULT, 2);
    let payout_two = payout_tx_on(&sim, &chain.chain_id, 98, seller);
    let b3 = mine_next_block(&chain, state::TREASURY, vec![sell, payout_one, payout_two]);
    assert!(!chain.validate_block(&b3));
}

#[test]
fn test_treasury_payout_without_sell_rejected() {
    // A payout without an earlier sell in the same block has no funding
    // reason and invalidates the block even when the vault is funded.
    let _tmp = setup_test_dir("treasury_orphan_payout");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();
    let buy = make_tx(
        seller,
        &sk,
        &[(0, 400)],
        &[
            state::Output {
                start: 0,
                end: 100,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 100,
                end: 398,
                to: seller,
                lock: None,
            },
            state::Output {
                start: 398,
                end: 400,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint = mint_tx(&chain, 100, seller);
    let b2 = mine_next_block(&chain, state::TREASURY, vec![buy, mint]);
    chain.apply(b2).unwrap();

    let payout = payout_tx_on(&chain.state, &chain.chain_id, 98, seller);
    let b3 = mine_next_block(&chain, state::TREASURY, vec![payout]);
    assert!(!chain.validate_block(&b3));
}

#[test]
fn test_treasury_unpaired_sell_burns_without_payout() {
    // A payment to the treasury address without a matching payout is an
    // ordinary transfer: the cells become treasury-owned again, the block
    // stays valid, and the curve counters do not move.
    let _tmp = setup_test_dir("treasury_unpaired_sell");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();
    let sold_before = chain.state.total_sold;

    let burn = make_tx(
        seller,
        &sk,
        &[(0, 101)],
        &[
            state::Output {
                start: 0,
                end: 100,
                to: state::TREASURY,
                lock: None,
            },
            state::Output {
                start: 100,
                end: 101,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let b2 = mine_next_block(&chain, state::TREASURY, vec![burn]);
    chain.apply(b2).unwrap();
    assert_eq!(chain.state.total_sold, sold_before);
    assert_eq!(chain.state.total_bought_back, 0);
    assert_eq!(chain.state.balance(seller), 500 - 101);
}
