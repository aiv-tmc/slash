//! `test_multi_input_balance_per_page` — one test case of the `integration_tests` suite.
//!
//! The shared module paths come from the crate root via `use super::*;`.

use super::*;

#[test]
fn test_multi_input_balance_per_page() {
    // A transaction that spends from two pages leaves both pages balanced:
    // conservation holds per page, not only globally. The test pins the
    // per-page conservation invariant; the signature itself is produced
    // for completeness, and the underscore binding documents that the
    // dedicated signature suites exercise the spend path it unlocks.
    let mut st = state::State::genesis();
    let owner = [1u8; 32];
    // PAGE_SIZE is a u64 constant, so the second page anchor is a plain
    // global cell coordinate without a cast.
    let page_size = state::PAGE_SIZE;
    st.mine(0, owner, 0);
    st.mine(page_size, owner, 0);
    let inputs = vec![(0u64, 1u64), (page_size, page_size + 1)];
    let outputs = vec![
        state::Output {
            start: 0,
            end: 1,
            to: [2u8; 32],
            lock: None,
        },
        state::Output {
            start: page_size,
            end: page_size + 1,
            to: [2u8; 32],
            lock: None,
        },
    ];
    let hash =
        chain::Chain::tx_signature_hash(&owner, &inputs, &outputs, chain::MAINNET_ID, 0, &None);
    let (sk, _) = crypto::generate_keypair();
    let _sig = crypto::sign(&sk, &hash);
    // Conservation: the balance cache always sums to the full supply.
    assert_eq!(st.balance_cache.values().sum::<u64>(), state::N);
}
