//! `era_nine_empty_block_without_coinbase_is_valid` — one test case of the `era_nine_boundary_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn era_nine_empty_block_without_coinbase_is_valid() {
    let _tmp = setup_test_dir("era_nine_empty");
    let mut chain = chain_with_next_height(ERA_NINE_START);
    assert_eq!(chain::subsidy_at(ERA_NINE_START), 0);
    let b = build_pow_block(&chain, vec![]);
    chain.apply(b).expect("an empty era-9 block must apply");
    assert_eq!(chain.tip_height(), ERA_NINE_START);
}
