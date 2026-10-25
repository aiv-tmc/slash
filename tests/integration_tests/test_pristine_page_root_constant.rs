//! `test_pristine_page_root_constant` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_pristine_page_root_constant() {
    // A pristine page contributes the implicit treasury commitment to the
    // global root; the commitment is stable across calls.
    let st = state::State::genesis();
    let r1 = st.global_root();
    let r2 = st.global_root();
    assert_eq!(r1, r2);
    assert_ne!(r1, [0u8; 32]);
}
