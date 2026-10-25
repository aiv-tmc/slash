//! `test_subsidy_at_exact_boundary_values` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_subsidy_at_exact_boundary_values() {
    // Assert the exact subsidy at the first block of every era and at the
    // last block of every era. A boundary error is a consensus fork in the
    // emission schedule.
    for (era, &subsidy) in ERA_SUBSIDIES.iter().enumerate() {
        let first = era as u64 * slash::chain::HALVING_INTERVAL;
        let last = first + slash::chain::HALVING_INTERVAL - 1;
        assert_eq!(
            slash::chain::subsidy_at(first),
            subsidy,
            "era {} first-block subsidy must be exactly {}",
            era,
            subsidy
        );
        assert_eq!(
            slash::chain::subsidy_at(last),
            subsidy,
            "era {} last-block subsidy must be exactly {}",
            era,
            subsidy
        );
    }
    assert_eq!(
        slash::chain::subsidy_at(9 * slash::chain::HALVING_INTERVAL),
        0
    );
}
