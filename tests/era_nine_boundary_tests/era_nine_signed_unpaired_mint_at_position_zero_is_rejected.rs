//! `era_nine_signed_unpaired_mint_at_position_zero_is_rejected` — one test case of the `era_nine_boundary_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn era_nine_signed_unpaired_mint_at_position_zero_is_rejected() {
    let _tmp = setup_test_dir("era_nine_signed_unpaired");
    let mut chain = chain_with_next_height(ERA_NINE_START);
    let buyer = [9u8; 32];
    let (s, e) = chain
        .state
        .select_single(TREASURY, 50)
        .expect("treasury owns the supply");
    // A properly signed mint still needs an earlier buy payment in the same
    // block; at position 0 no earlier transaction can exist, so pairing must
    // fail even though the signature verifies.
    let mint = make_tx(
        TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &[Output {
            start: s,
            end: e,
            to: buyer,
            lock: None,
        }],
        &chain.chain_id,
    );
    let b = build_pow_block(&chain, vec![mint]);
    assert!(
        chain.apply(b).is_err(),
        "a signed but unpaired treasury mint must be rejected in every era"
    );
}
