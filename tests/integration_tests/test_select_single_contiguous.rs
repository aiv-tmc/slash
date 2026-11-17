//! `test_select_single_contiguous` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` and `state` paths are imported explicitly so this file
//! compiles on its own.

use super::*;
use slash::chain::Chain;
use slash::state;

#[test]
fn test_select_single_contiguous() {
    let _tmp = setup_test_dir("select_single_contiguous");
    reset_testnet();
    let mut chain = treasury_genesis();

    // A treasury selection of width 100 lands inside one pristine page,
    // so the returned range is a single contiguous run.
    let (start, end) = chain
        .state
        .select_single(state::TREASURY, 100)
        .expect("treasury must fund the selection");
    assert_eq!(end - start, 100);

    // Ownership is observable through the public engine path: a properly
    // packed, correctly signed spend over the selected range executes.
    let (sk, owner) = test_keypair(7);
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, owner);
    }
    let (s, e) = chain
        .state
        .select_single(owner, 50)
        .expect("owner balance must cover the selection");
    assert_eq!(e - s, 50);
    let outputs = vec![
        state::Output {
            start: s,
            end: e - 1,
            to: owner,
            lock: None,
        },
        state::Output {
            start: e - 1,
            end: e,
            to: state::FEE_VAULT,
            lock: None,
        },
    ];
    let hash = Chain::tx_signature_hash(
        &owner,
        &[(s, e)],
        &outputs,
        chain.chain_id.as_bytes(),
        0,
        &None,
    );
    let sig = slash::crypto::sign(&sk, &hash);
    let height = chain.tip_height() + 1;
    let chain_id = chain.chain_id.clone();
    assert!(chain.state_mut().tx(
        owner,
        &[(s, e)],
        &outputs,
        &sig,
        height,
        chain_id.as_bytes(),
        0,
        &None,
    ));
}
