//! src/dao/payload.rs — payload structural validation (at submission
//! time) and payload execution (at the executable height). Bounds at
//! submission keep a passed proposal from requesting a transition that
//! would be guaranteed to expire; execution routes treasury movements
//! through the canonical selection so every node reproduces identical
//! ranges.

use super::types::DaoPayload;

/// Structural validation for a payload, evaluated at submission time.
/// Numeric bounds keep a passed proposal from requesting an absurd state
/// transition that would be guaranteed to expire at execution.
pub fn payload_valid(payload: &DaoPayload) -> bool {
    match payload {
        DaoPayload::TreasuryMint { amount, to } => *amount >= crate::state::MIN && *to != [0u8; 32],
        DaoPayload::TreasuryPayout { amount, to } => {
            *amount >= crate::state::MIN && *to != [0u8; 32]
        }
        DaoPayload::RotateDevFundSigner { public } => *public != [0u8; 32],
        DaoPayload::RotateTreasurySigner { public } => *public != [0u8; 32],
        DaoPayload::SetDevFee { bps } => *bps <= 100,
        DaoPayload::SetMineFee { bps } => *bps <= 1000,
        // A pause must request at least one class and must not set flag
        // bits nobody recognizes: unknown bits could never be cleared by
        // a future resume semantics and would freeze an undefined
        // surface forever.
        DaoPayload::EmergencyPause {
            flags,
            until_height,
        } => *flags != 0 && flags & !0b11 == 0 && *until_height > 0,
        // Resuming is always structurally valid; whether a pause is live
        // is a runtime question, not a submission bound.
        DaoPayload::EmergencyResume => true,
    }
}

/// Execute a passed payload against the state. Mint and payout flow through
/// the canonical treasury selection so every node reproduces identical
/// input ranges and page roots; rotations and fee adjustments are direct
/// field writes. Returns false when the transition cannot be applied, in
/// which case the caller marks the proposal expired.
pub fn execute_payload(state: &mut crate::state::State, payload: &DaoPayload, height: u64) -> bool {
    match payload {
        DaoPayload::TreasuryMint { amount, to } => {
            let Some((start, end)) = state.select_single(crate::state::TREASURY, *amount) else {
                return false;
            };
            state.mint(
                &[(start, end)],
                &[crate::state::Output {
                    start,
                    end,
                    to: *to,
                    lock: None,
                }],
                height,
            )
        }
        DaoPayload::TreasuryPayout { amount, to } => {
            let Some((start, end)) = state.select_single(crate::state::TREASURY_VAULT, *amount)
            else {
                return false;
            };
            state.payout(
                &[(start, end)],
                &[crate::state::Output {
                    start,
                    end,
                    to: *to,
                    lock: None,
                }],
                height,
            )
        }
        DaoPayload::RotateDevFundSigner { public } => {
            state.dev_fund_signer = *public;
            true
        }
        DaoPayload::RotateTreasurySigner { public } => {
            state.treasury_signer = *public;
            true
        }
        DaoPayload::SetDevFee { bps } => {
            state.dev_fee_bps = *bps;
            true
        }
        DaoPayload::SetMineFee { bps } => {
            state.mine_fee_bps = *bps;
            true
        }
        // A pause is a direct field write: the flags and the expiry
        // height land in state, so the freeze rides the ordinary state
        // transition. Validation paths read it from the pre-block state,
        // which makes the pause take effect from the next block and
        // reproduce exactly on every node — and, because execution is
        // deterministic, a reorg that re-executes the same proposal
        // re-establishes the pause on the surviving branch.
        DaoPayload::EmergencyPause {
            flags,
            until_height,
        } => {
            state.emergency_flags = *flags;
            state.emergency_until = *until_height;
            let _ = height;
            true
        }
        DaoPayload::EmergencyResume => {
            state.emergency_flags = 0;
            state.emergency_until = 0;
            true
        }
    }
}
