//! `era_nine_unsigned_treasury_tx_at_position_zero_is_rejected` — one test case of the `era_nine_boundary_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn era_nine_unsigned_treasury_tx_at_position_zero_is_rejected() {
    let _tmp = setup_test_dir("era_nine_unsigned_pos0");
    let mut chain = chain_with_next_height(ERA_NINE_START);
    let user = [9u8; 32];
    let tx = unsigned_treasury_tx(&chain.state, user);
    let b = build_pow_block(&chain, vec![tx]);
    assert!(
        chain.apply(b).is_err(),
        "an unsigned treasury transaction at position 0 of an era-9 block must be rejected"
    );
}
