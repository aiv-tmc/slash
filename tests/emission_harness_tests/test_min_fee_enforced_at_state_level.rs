//! `test_min_fee_enforced_at_state_level` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_min_fee_enforced_at_state_level() {
    // The mandatory fee rule fee * 200 >= total_out holds at the state
    // level (the single source of truth shared by mempool and block
    // execution). Boundary: total_out = 200 with fee = 1 passes; fee = 0
    // fails.
    let mut st = State::genesis();
    let owner = [1u8; 32];
    st.mine(0, owner, 0);
    st.mine(1, owner, 0);
    st.mine(2, owner, 0);
    st.mine(3, owner, 0);
    let pk = owner;
    let (s, _sig) = {
        let (sk, pk) = crypto::generate_keypair();
        for i in 0..200 {
            st.mine(i, pk, 0);
        }
        (sk, pk)
    };
    let _ = s;
    let mk_outputs = |fee: u64| -> Vec<Output> {
        if fee == 0 {
            vec![Output {
                start: 0,
                end: 200,
                to: pk,
                lock: None,
            }]
        } else {
            vec![
                Output {
                    start: 0,
                    end: 199,
                    to: pk,
                    lock: None,
                },
                Output {
                    start: 199,
                    end: 200,
                    to: FEE_VAULT,
                    lock: None,
                },
            ]
        }
    };
    let inputs = vec![(0u64, 200u64)];
    let (sk, pk) = crypto::generate_keypair();
    for i in 0..200 {
        st.mine(i, pk, 0);
    }
    let outputs_ok = mk_outputs(1);
    let hash =
        chain::Chain::tx_signature_hash(&pk, &inputs, &outputs_ok, chain::MAINNET_ID, 0, &None);
    let sig = crypto::sign(&sk, &hash);
    assert!(
        st.tx(
            pk,
            &inputs,
            &outputs_ok,
            &sig,
            0,
            chain::MAINNET_ID.as_bytes(),
            0,
            &None
        ),
        "fee = 1 with total_out = 200 must satisfy fee * 200 >= total_out"
    );
    let outputs_bad = mk_outputs(0);
    let hash =
        chain::Chain::tx_signature_hash(&pk, &inputs, &outputs_bad, chain::MAINNET_ID, 0, &None);
    let sig = crypto::sign(&sk, &hash);
    assert!(
        !st.tx(
            pk,
            &inputs,
            &outputs_bad,
            &sig,
            0,
            chain::MAINNET_ID.as_bytes(),
            0,
            &None
        ),
        "fee = 0 with total_out = 200 must fail the mandatory fee rule"
    );
}
