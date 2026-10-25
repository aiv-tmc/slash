//! `dev_fee_rounding_pins_minimum_one_cell` — one test case of the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fee_rounding_pins_minimum_one_cell() {
    // Rounding boundary vectors for the basis-point share with bps = 1:
    // every transfer below 10_000 cells owes exactly one cell, and the
    // requirement scales linearly above that.
    assert_eq!(required(9_999, 1), 1);
    assert_eq!(required(10_000, 1), 1);
    assert_eq!(required(250_000, 1), 25);
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 250_010);
    let total_out = 250_000u64;
    // A transfer of 250_000 cells owes 25 cells to the dev fund; 24 is
    // rejected by the mempool even though the fee-vault share is correct.
    let outputs_short = vec![
        Output {
            start: 0,
            end: total_out - 26,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: total_out - 26,
            end: total_out - 25,
            to: DEV_FUND,
            lock: None,
        },
        Output {
            start: total_out - 25,
            end: 249_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 249_999,
            end: 250_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &outputs_short, &chain.chain_id);
    assert!(
        !chain.validate_tx(&tx),
        "one cell below the exact share must be rejected"
    );
    // The exact share is accepted.
    let outputs_exact = vec![
        Output {
            start: 0,
            end: total_out - 25,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: total_out - 25,
            end: total_out,
            to: DEV_FUND,
            lock: None,
        },
        Output {
            start: total_out,
            end: 249_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 249_999,
            end: 250_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &outputs_exact, &chain.chain_id);
    assert!(
        chain.validate_tx(&tx),
        "the exact basis-point share must satisfy the rule"
    );
}
