//! `dev_fee_not_required_before_activation` — one test case of the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fee_not_required_before_activation() {
    // With the deployment in Defined state, a user transaction without a
    // dev-fund output must pass mempool validation and apply cleanly in a
    // block; this pins the zero-obligation window before signaling.
    reset_testnet();
    let mut chain = treasury_genesis();
    let (sk, pk) = slash::crypto::generate_keypair();
    let _ = fund(&mut chain, pk, 200);
    let outputs = vec![
        Output {
            start: 0,
            end: 150,
            to: [9u8; 32],
            lock: None,
        },
        Output {
            start: 150,
            end: 199,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: 199,
            end: 200,
            to: pk,
            lock: None,
        },
    ];
    let tx = make_tx(pk, &sk, &[(0, 200)], &outputs, &chain.chain_id);
    assert!(
        chain.validate_tx(&tx),
        "before activation the dev-fund output must not be required"
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![tx]);
    chain.apply(b).expect("inactive rule: block applies");
}
