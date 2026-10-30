//! `treasury_and_pairing_paths_exempt_from_dev_fee` — one test case of
//! the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn treasury_and_pairing_paths_exempt_from_dev_fee() {
    // The mint half of a buy pair never carries a dev-fund output, and
    // the treasury address is exempt from the rule, so a fully paired
    // buy block applies with the rule active even though the mint itself
    // pays no dev share. The pairing payment is an ordinary user
    // transaction and pays its own one-cell share. The coinbase stays
    // clean because mine_fee_bps is zeroed.
    //
    // The mint inputs are the range immediately after the coinbase
    // range: the coinbase claims the canonical frontier first inside the
    // block, so a mint selecting the same canonical range would find it
    // already spent and the pairing classification would fail. Reserving
    // the successor range keeps the pair executable.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    chain.state_mut().mine_fee_bps = 0;
    let (sk, pk) = slash::crypto::generate_keypair();
    // Fund the buyer high above the canonical treasury frontier so the
    // canonical coinbase range and the mint reservation never overlap,
    // and reserve the mint range: the coinbase claims the canonical
    // frontier first inside the block, so the mint selects the successor
    // range. The buyer funding and the reservation ride one
    // `state_mut()` access each: the chain is fixture-owned and the
    // reference is unique.
    for i in 10_000..10_600 {
        chain.state_mut().mine(i, pk, 0);
    }
    let (s, e) = chain
        .state
        .select_single(slash::state::TREASURY, 500)
        .expect("treasury funds the mint reservation");
    assert_eq!(e - s, 500);
    // The payment: an ordinary user transaction paying its own one-cell
    // dev share, the mandatory fee, and the change back to the buyer.
    let payment_outputs = vec![
        Output {
            start: 10_000,
            end: 10_500,
            to: slash::state::TREASURY_VAULT,
            lock: None,
        },
        Output {
            start: 10_500,
            end: 10_501,
            to: DEV_FUND,
            lock: None,
        },
        Output {
            start: 10_501,
            end: 10_504,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 10_504,
            end: 10_600,
            to: pk,
            lock: None,
        },
    ];
    let payment = make_tx(
        pk,
        &sk,
        &[(10_000, 10_600)],
        &payment_outputs,
        &chain.chain_id,
    );
    assert!(
        chain.validate_tx(&payment),
        "the pairing payment is an ordinary user transaction and pays its share"
    );
    // The mint: treasury-signed, no dev output, exempt from the rule.
    let mint_outputs = vec![Output {
        start: s,
        end: e,
        to: [9u8; 32],
        lock: None,
    }];
    let mint = make_tx(
        TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &mint_outputs,
        &chain.chain_id,
    );
    assert!(
        chain.validate_tx(&mint),
        "treasury transactions are exempt from the dev-fund rule"
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![payment, mint]);
    chain
        .apply(b)
        .expect("the paired buy block applies with the rule active");
    assert_eq!(chain.state.balance([9u8; 32]), 500);
}
