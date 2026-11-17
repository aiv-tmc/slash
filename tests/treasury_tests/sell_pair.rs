//! Sell-side pairing: a treasury return is refunded at 98% of the curve
//! price while the vault covers it, and the curve position recovers
//! after the sale.

use super::*;

#[test]
fn test_treasury_sell_pair_payout_98pct() {
    let _tmp = setup_test_dir("treasury_sell_pair");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();

    // Seed the seller with a 500-cell coinbase.
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();

    // Buy 100 cells: pay 100 into the vault, 298 change, 2 cells fee.
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
    assert_eq!(chain.state.balance(state::TREASURY_VAULT), 100);
    assert_eq!(chain.state.total_sold, 100);

    // Sell 100 cells back: 100 to TREASURY plus a one-cell fee (101 inputs).
    // sell_price(100, 100) == 98, paid out of the vault.
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
    let payout = payout_tx_on(&chain.state, &chain.chain_id, 98, seller);
    let b3 = mine_next_block(&chain, state::TREASURY, vec![sell, payout]);
    chain.apply(b3).unwrap();

    // The vault keeps the 2-cell spread; the sell counters advance.
    assert_eq!(chain.state.balance(state::TREASURY_VAULT), 2);
    assert_eq!(chain.state.total_bought_back, 100);
    // The sell hands exactly the returned width (100) to the treasury
    // and the vault refunds the sell price (98); the declared 1-cell
    // fee output is not executed, the mandatory fee floor of 101 / 200
    // being zero. After the buy round (500 seeded, minus the 100-cell
    // payment and the 2-cell fee floor of 400 / 200, plus the 100-cell
    // mint) the seller holds 498, so the round ends at 496.
    assert_eq!(chain.state.balance(seller), 496);
}

#[test]
fn test_treasury_sell_pair_insufficient_vault_rejected() {
    let _tmp = setup_test_dir("treasury_empty_vault");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();

    // Sell without any prior buy: the vault is empty, so the payout has no
    // canonical vault inputs and the block is invalid.
    let sell = make_tx(
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
    // The live vault is empty, so the payout claims vault cells that do
    // not exist; a treasury payout without vault funding invalidates the
    // block.
    let payout = make_tx(
        state::TREASURY,
        &TREASURY_SK,
        &[(0, 98)],
        &[state::Output {
            start: 0,
            end: 98,
            to: seller,
            lock: None,
        }],
        &chain.chain_id,
    );
    let height = chain.tip_height() + 1;
    let mut b2 = mine_valid_block(&chain, 1_700_000_000 + height, state::TREASURY, 1);
    b2.txs.extend([sell, payout]);
    grind_nonce(&chain, &mut b2);
    assert!(!chain.validate_block(&b2));
}

#[test]
fn test_treasury_sell_pair_restores_curve_position() {
    // Golden symmetry vector: a full buy-then-sell round trip returns
    // total_sold to its starting value and records the buy-back, so the
    // next buy reprices at the original curve position.
    let _tmp = setup_test_dir("treasury_curve_symmetry");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, seller) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, seller, vec![]);
    chain.apply(b1).unwrap();
    let price_before = treasury::buy_price(50, chain.state.total_sold);

    // Buy 100 cells: pay 100 into the vault, keep 298 cells of change,
    // pay the two-cell minimum fee.
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
    assert_eq!(chain.state.total_sold, 100);

    // Sell the same 100 cells back: the payout is exactly sell_price(100, 100).
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
    let payout = payout_tx_on(&chain.state, &chain.chain_id, 98, seller);
    let b3 = mine_next_block(&chain, state::TREASURY, vec![sell, payout]);
    chain.apply(b3).unwrap();

    // The curve position returns to its starting value and the next buy
    // reprices exactly where the first one did.
    assert_eq!(chain.state.total_sold, 0);
    assert_eq!(chain.state.total_bought_back, 100);
    assert_eq!(
        treasury::buy_price(50, chain.state.total_sold),
        price_before
    );
}
