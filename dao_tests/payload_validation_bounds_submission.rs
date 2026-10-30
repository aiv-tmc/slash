//! `payload_validation_bounds_submission` — one case of the `dao_tests` suite.

use super::*;

#[test]
fn payload_validation_bounds_submission() {
    // Structural bounds at submission: below-minimum amounts, zero
    // addresses, and out-of-range basis points are rejected; a valid
    // payload is accepted. The quorum denominator rides every submission
    // — here it is the locked weight of a genesis state, which is zero,
    // and none of the accept/refuse outcomes below depend on it.
    let state = slash::state::State::genesis();
    let total = slash::dao::total_locked_weight(&state, 1);
    let mut dao = DaoState::genesis();
    assert!(dao
        .submit(
            DaoPayload::TreasuryMint {
                amount: 10,
                to: [1u8; 32]
            },
            [1u8; 32],
            1,
            total
        )
        .is_none());
    assert!(dao
        .submit(
            DaoPayload::TreasuryMint {
                amount: 500,
                to: [0u8; 32]
            },
            [1u8; 32],
            1,
            total
        )
        .is_none());
    assert!(dao
        .submit(DaoPayload::SetDevFee { bps: 101 }, [1u8; 32], 1, total)
        .is_none());
    assert!(dao
        .submit(
            DaoPayload::RotateTreasurySigner { public: [0u8; 32] },
            [1u8; 32],
            1,
            total
        )
        .is_none());
    assert!(dao
        .submit(DaoPayload::SetDevFee { bps: 50 }, [1u8; 32], 1, total)
        .is_some());
}

#[test]
fn pause_horizon_is_bounded_at_submission() {
    // A pause may not reach past the submission-bound horizon: beyond it
    // the freeze could outlive any plausible quorum, and inside it a
    // class can always be re-paused by a fresh proposal. The bound is
    // relative to the height at which the proposal is submitted.
    let state = slash::state::State::genesis();
    let total = slash::dao::total_locked_weight(&state, 1_000);
    let mut dao = DaoState::genesis();
    let horizon = 1_000 + slash::dao::DAO_MAX_PAUSE_BLOCKS;
    assert!(dao
        .submit(
            DaoPayload::EmergencyPause {
                flags: 0b01,
                until_height: horizon + 1,
            },
            [1u8; 32],
            1_000,
            total
        )
        .is_none());
    assert!(dao
        .submit(
            DaoPayload::EmergencyPause {
                flags: 0b01,
                until_height: horizon,
            },
            [1u8; 32],
            1_000,
            total
        )
        .is_some());
    assert!(dao
        .submit(
            DaoPayload::EmergencyPause {
                flags: 0b01,
                until_height: horizon,
            },
            [1u8; 32],
            999,
            total
        )
        .is_none());
    assert!(dao
        .submit(
            DaoPayload::EmergencyPause {
                flags: 0b01,
                until_height: 1,
            },
            [1u8; 32],
            1_000,
            total
        )
        .is_none());
}
