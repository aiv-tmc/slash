//! `test_emission_cumulative_matches_design_document` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_emission_cumulative_matches_design_document() {
    // Walk all nine eras at state level, minting every era supply through
    // the same primitive the coinbase uses (canonical select_single inputs
    // plus State::mint), and assert that the cumulative minted amount equals
    // the design-document cap of 417_957_120 cells and that the treasury
    // balance equals N minus the minted amount afterwards.
    let mut state = State::genesis();
    let mut minted = 0u64;
    let miner = [7u8; 32];
    for &subsidy in &ERA_SUBSIDIES {
        let supply = subsidy * slash::chain::HALVING_INTERVAL;
        minted += supply;
        let (s, e) = state
            .select_single(TREASURY, supply)
            .expect("treasury must fund the era");
        let ok = state.mint(
            &[(s, e)],
            &[Output {
                start: s,
                end: e,
                to: miner,
                lock: None,
            }],
            0,
        );
        assert!(ok, "era supply of {} cells must mint", supply);
    }
    assert_eq!(minted, EMISSION_CAP);
    assert_eq!(state.balance(TREASURY), N - EMISSION_CAP);
    assert_eq!(state.balance(miner), EMISSION_CAP);
}
