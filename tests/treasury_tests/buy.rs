//! Buy-side pairing: a vault payment funds exactly one mint at the curve
//! price; underpayment, unpaired mints, and double-funded mints are
//! rejected.

use super::*;

#[test]
fn test_treasury_buy_pair_mints_exact_amount() {
    let _tmp = setup_test_dir("treasury_buy_pair");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (_sk, buyer) = crypto::generate_keypair();

    // Fund the buyer with a coinbase block: 500 cells in range (0, 500).
    let b1 = mine_next_block(&chain, buyer, vec![]);
    chain.apply(b1).unwrap();

    // Buy 50 cells: pay 50 into the vault, keep 9 cells of change, pay the
    // one-cell minimum fee. buy_price(50, 0) == 50.
    let buy = make_tx(
        buyer,
        &_sk,
        &[(0, 60)],
        &[
            state::Output {
                start: 0,
                end: 50,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 50,
                end: 59,
                to: buyer,
                lock: None,
            },
            state::Output {
                start: 59,
                end: 60,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint = mint_tx(&chain, 50, buyer);
    let b2 = mine_next_block(&chain, state::TREASURY, vec![buy, mint]);
    chain.apply(b2).unwrap();

    assert_eq!(chain.state.balance(state::TREASURY_VAULT), 50);
    assert_eq!(chain.state.total_sold, 50);
    // The payment funds the mint exactly, so the round is net zero for
    // the buyer: 500 seeded, minus the 50-cell curve payment, plus the
    // 50-cell mint. Change and fee outputs on a treasury-bound payment
    // are not executed; the mandatory fee floor of 60 / 200 is zero, so
    // nothing else leaves the buyer.
    assert_eq!(chain.state.balance(buyer), 500);
}

#[test]
fn test_treasury_buy_pair_underpayment_rejected() {
    let _tmp = setup_test_dir("treasury_underpay");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (_sk, buyer) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, buyer, vec![]);
    chain.apply(b1).unwrap();

    // The vault receives 49 cells, but buy_price(50, 0) == 50.
    let buy = make_tx(
        buyer,
        &_sk,
        &[(0, 60)],
        &[
            state::Output {
                start: 0,
                end: 49,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 49,
                end: 59,
                to: buyer,
                lock: None,
            },
            state::Output {
                start: 59,
                end: 60,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint = mint_tx(&chain, 50, buyer);
    // The block is intentionally invalid: build the raw template, grind
    // the proof slot so the header checks pass, and reject it through
    // the validation path. The positive-path fixture panics on
    // unexecutable blocks instead, so it cannot serve negative tests.
    let height = chain.tip_height() + 1;
    let mut b2 = mine_valid_block(&chain, 1_700_000_000 + height, state::TREASURY, 1);
    b2.txs.extend([buy, mint]);
    grind_nonce(&chain, &mut b2);
    assert!(!chain.validate_block(&b2));
}

#[test]
fn test_treasury_mint_without_payment_rejected() {
    let _tmp = setup_test_dir("treasury_unpaired_mint");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (_sk, buyer) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, buyer, vec![]);
    chain.apply(b1).unwrap();

    // A signed mint with no preceding payment in the same block is invalid.
    let mint = mint_tx(&chain, 50, buyer);
    // The block is intentionally invalid: build the raw template, grind
    // the proof slot so the header checks pass, and reject it through
    // the validation path. The positive-path fixture panics on
    // unexecutable blocks instead, so it cannot serve negative tests.
    let height = chain.tip_height() + 1;
    let mut b2 = mine_valid_block(&chain, 1_700_000_000 + height, state::TREASURY, 1);
    b2.txs.extend([mint]);
    grind_nonce(&chain, &mut b2);
    assert!(!chain.validate_block(&b2));
}

#[test]
fn test_treasury_double_mint_against_single_payment_rejected() {
    let _tmp = setup_test_dir("treasury_double_mint");
    reset_testnet();
    let mut chain = treasury_genesis();
    let (_sk, buyer) = crypto::generate_keypair();
    let b1 = mine_next_block(&chain, buyer, vec![]);
    chain.apply(b1).unwrap();

    // One payment of 100 funds the first 50-cell mint; the second mint must
    // find the payment output consumed and fail.
    let buy = make_tx(
        buyer,
        &_sk,
        &[(0, 120)],
        &[
            state::Output {
                start: 0,
                end: 100,
                to: state::TREASURY_VAULT,
                lock: None,
            },
            state::Output {
                start: 100,
                end: 119,
                to: buyer,
                lock: None,
            },
            state::Output {
                start: 119,
                end: 120,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let mint1 = mint_tx(&chain, 50, buyer);
    // The second mint uses inputs canonical on the state after the first mint.
    // The scratch copy detaches the value from the chain's shared `Arc`,
    // so the simulated first mint never touches the live state.
    let mut sim = (*chain.state).clone();
    sim.mint(
        &mint1.inputs,
        &mint1
            .outputs
            .iter()
            .map(|o| state::Output {
                start: o.start,
                end: o.end,
                to: o.to,
                lock: o.lock,
            })
            .collect::<Vec<_>>(),
        2,
    );
    let mint2 = {
        let (s, e) = sim.select_single(state::TREASURY, 50).unwrap();
        make_tx(
            state::TREASURY,
            &TREASURY_SK,
            &[(s, e)],
            &[state::Output {
                start: s,
                end: e,
                to: buyer,
                lock: None,
            }],
            &chain.chain_id,
        )
    };
    // The block is intentionally invalid: build the raw template, grind
    // the proof slot so the header checks pass, and reject it through
    // the validation path. The positive-path fixture panics on
    // unexecutable blocks instead, so it cannot serve negative tests.
    let height = chain.tip_height() + 1;
    let mut b2 = mine_valid_block(&chain, 1_700_000_000 + height, state::TREASURY, 1);
    b2.txs.extend([buy, mint1, mint2]);
    grind_nonce(&chain, &mut b2);
    assert!(!chain.validate_block(&b2));
}
