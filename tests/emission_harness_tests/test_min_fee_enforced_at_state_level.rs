//! `test_min_fee_enforced_at_state_level` — one test case of the
//! `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_min_fee_enforced_at_state_level() {
    // The mandatory fee lives in the state engine: any transaction whose
    // fee-vault output totals less than total_out / 200 is refused, the
    // exact boundary is accepted, and more than the minimum is accepted.
    let mut st = State::genesis();
    let (sk, pk) = slash::crypto::generate_keypair();
    for i in 0..200 {
        st.mine(i, pk, 0);
    }
    // Boundary vectors over a 200-cell single-page input: total_out 200
    // owes exactly one cell, so fee = 0 is refused and fee = 1 is the
    // exact boundary. A second output returning the change keeps the
    // packing exact.
    let outputs_for = |fee: u64| {
        vec![
            Output {
                start: 0,
                end: 200 - fee,
                to: [9u8; 32],
                lock: None,
            },
            Output {
                start: 200 - fee,
                end: 200,
                to: FEE_VAULT,
                lock: None,
            },
        ]
    };
    let mk = |fee: u64| {
        let outputs = outputs_for(fee);
        let hash = slash::chain::Chain::tx_signature_hash(
            &pk,
            &[(0, 200)],
            &outputs,
            slash::chain::MAINNET_ID.as_bytes(),
            0,
            &None,
        );
        let sig = slash::crypto::sign(&sk, &hash);
        (outputs, sig)
    };
    // fee = 0: 0 * 200 < 200, refused.
    let (outputs, sig) = mk(0);
    assert!(!st.tx(
        pk,
        &[(0, 200)],
        &outputs,
        &sig,
        0,
        slash::chain::MAINNET_ID.as_bytes(),
        0,
        &None
    ));
    // fee = 1 with total_out = 200: 1 * 200 >= 200 exactly, accepted.
    // The state is reused, so this second attempt re-spends the same
    // cells: build a fresh state for the acceptance vector.
    let mut st = State::genesis();
    for i in 0..200 {
        st.mine(i, pk, 0);
    }
    let (outputs, sig) = mk(1);
    assert!(st.tx(
        pk,
        &[(0, 200)],
        &outputs,
        &sig,
        0,
        slash::chain::MAINNET_ID.as_bytes(),
        0,
        &None
    ));
}
