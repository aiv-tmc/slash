//! `dev_fee_required_by_mempool_when_active` — one test case of the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fee_required_by_mempool_when_active() {
    // While the deployment is active, a structurally valid transaction with
    // a correct fee-vault share but no dev-fund output is rejected by
    // mempool admission, and adding the exact share makes it valid.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 10_000);
    let without_dev = vec![
        Output {
            start: 0,
            end: 9_000,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: 9_000,
            end: 9_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 9_999,
            end: 10_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &without_dev, &chain.chain_id);
    assert!(!chain.validate_tx(&tx));
    let with_dev = vec![
        Output {
            start: 0,
            end: 8_999,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: 8_999,
            end: 9_000,
            to: DEV_FUND,
            lock: None,
        },
        Output {
            start: 9_000,
            end: 9_999,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 9_999,
            end: 10_000,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[inputs], &with_dev, &chain.chain_id);
    assert!(chain.validate_tx(&tx));
}
