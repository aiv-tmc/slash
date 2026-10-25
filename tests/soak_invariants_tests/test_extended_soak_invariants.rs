//! `test_extended_soak_invariants` — extended profile of the deterministic
//! soak for manual runs (`cargo test -- --ignored`): 100,000 operations
//! with full invariant checks after every mutation.

use super::*;

#[test]
#[ignore = "extended soak: run manually with cargo test --release -- --ignored"]
fn test_extended_soak_invariants() {
    run_soak(100_000);
}
