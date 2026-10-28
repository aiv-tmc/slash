//! `dev_fee_rounding_pins_minimum_one_cell` — one test case of the
//! `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fee_rounding_pins_minimum_one_cell() {
    // Rounding boundary vectors for the basis-point share with bps = 1:
    // every transfer below 10_000 cells owes exactly one cell, and the
    // requirement scales linearly above that. Both transactions below
    // are shape-identical and differ only in the dev share, so the
    // accept/reject split is attributable to the share alone.
    //
    // Every range is page-local: the engine stages each range on the
    // single page it belongs to, so ranges crossing a page boundary
    // never execute. The 250_000-cell transfer is therefore cut into
    // four page-wide inputs and seven page-local outputs.
    assert_eq!(required(9_999, 1), 1);
    assert_eq!(required(10_000, 1), 1);
    assert_eq!(required(250_000, 1), 25);
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    // Fund exactly the input span: 250_000 cells move, nothing more, so
    // the packing is exact and the only variable is the dev share.
    let inputs = fund(&mut chain, pk, 250_000);
    assert_eq!(inputs, (0, 250_000));
    let page_inputs: Vec<(u64, u64)> = vec![
        (0, 65_536),
        (65_536, 131_072),
        (131_072, 196_608),
        (196_608, 250_000),
    ];
    // The mandatory fee at 0.5 % of the total output is 1_250 cells;
    // both vectors pay it in full. The recipient absorbs one cell of
    // difference between the two vectors so the packing stays exact.
    let outputs_for = |dev: u64| {
        // recipient total: 250_000 - 1_250 (fee) - dev.
        let to_total = 250_000 - 1_250 - dev;
        vec![
            Output {
                start: 0,
                end: 65_536,
                to: [9u8; 32],
                lock: None,
            },
            Output {
                start: 65_536,
                end: 131_072,
                to: [9u8; 32],
                lock: None,
            },
            Output {
                start: 131_072,
                end: 196_608,
                to: [9u8; 32],
                lock: None,
            },
            Output {
                start: 196_608,
                end: to_total,
                to: [9u8; 32],
                lock: None,
            },
            Output {
                start: to_total,
                end: to_total + dev,
                to: DEV_FUND,
                lock: None,
            },
            Output {
                start: to_total + dev,
                end: 250_000,
                to: FEE_VAULT,
                lock: None,
            },
        ]
    };
    // One cell below the exact share: 24 < max(250_000 * 1 / 10_000, 1)
    // = 25.
    let tx = make_tx(pk, &sk, &page_inputs, &outputs_for(24), &chain.chain_id);
    assert!(
        !chain.validate_tx(&tx),
        "one cell below the exact share must be rejected"
    );
    // The exact share: 25 cells to the dev fund, everything else
    // identical.
    let tx = make_tx(pk, &sk, &page_inputs, &outputs_for(25), &chain.chain_id);
    assert!(
        chain.validate_tx(&tx),
        "the exact basis-point share must satisfy the rule"
    );
}
