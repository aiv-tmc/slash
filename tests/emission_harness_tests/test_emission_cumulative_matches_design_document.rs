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
    //
    // Each mint is chunked to one page: the canonical selection serves at
    // most one pristine page per call, so a whole era supply is funded the
    // way real funding works — many page-sized inputs, never one giant
    // range.
    let mut state = State::genesis();
    let mut minted = 0u64;
    let miner = [7u8; 32];
    for &subsidy in &ERA_SUBSIDIES {
        let supply = subsidy * slash::chain::HALVING_INTERVAL;
        let mut remaining = supply;
        while remaining > 0 {
            let chunk = remaining.min(PAGE_SIZE);
            let (s, e) = state
                .select_single(TREASURY, chunk)
                .expect("treasury must fund the era chunk");
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
            assert!(ok, "era supply chunk of {} cells must mint", chunk);
            remaining -= chunk;
        }
        minted += supply;
    }
    assert_eq!(minted, EMISSION_CAP);
    assert_eq!(state.balance(TREASURY), N - EMISSION_CAP);
    assert_eq!(state.balance(miner), EMISSION_CAP);
}
