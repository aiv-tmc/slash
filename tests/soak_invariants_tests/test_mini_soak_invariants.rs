//! `test_mini_soak_invariants` — CI profile of the deterministic soak.
//! Runs 1,500 pseudo-random operations with full invariant checks after
//! every mutation; shared helpers come from the crate root.

use super::*;

#[test]
fn test_mini_soak_invariants() {
    run_soak(1_500);
}
