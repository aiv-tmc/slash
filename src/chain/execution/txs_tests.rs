//! The pipeline tests of the block transaction executor. The module is
//! wired into `txs.rs` through a path attribute, so it drives the private
//! lifeline predicate and the governance parser exactly as an inline
//! module would while keeping the executor itself within the line
//! budget.

use super::*;

/// The memo bytes the tests and the wire carry: a bincode-serialized
/// governance operation.
fn memo_of(op: &crate::dao::DaoOp) -> Option<Vec<u8>> {
    Some(bincode::serialize(op).expect("DaoOp serializes"))
}

#[test]
fn emergency_lifeline_admits_only_resume_submissions() {
    // The carve-out admits exactly one class — an EmergencyResume
    // submission — and refuses every other memo shape, including
    // other governance operations: under a user-transaction freeze
    // votes and fresh pause proposals stop, only the lifting
    // mechanism flows.
    let resume = crate::dao::DaoOp::SubmitProposal {
        payload: crate::dao::DaoPayload::EmergencyResume,
    };
    assert!(is_emergency_lifeline(&memo_of(&resume)));
    let pause = crate::dao::DaoOp::SubmitProposal {
        payload: crate::dao::DaoPayload::EmergencyPause {
            flags: crate::dao::EMERGENCY_USER_TXS,
            until_height: 100,
        },
    };
    assert!(!is_emergency_lifeline(&memo_of(&pause)));
    let vote = crate::dao::DaoOp::Vote {
        proposal_id: 1,
        approve: true,
    };
    assert!(!is_emergency_lifeline(&memo_of(&vote)));
    assert!(!is_emergency_lifeline(&None));
    assert!(!is_emergency_lifeline(&Some(b"garbage".to_vec())));
}

#[test]
fn governance_op_parse_is_shared_with_the_lifeline() {
    // The freeze carve-out and the operation application consume the
    // same parse: whatever the lifeline admits, the block applies.
    let resume = crate::dao::DaoOp::SubmitProposal {
        payload: crate::dao::DaoPayload::EmergencyResume,
    };
    let memo = memo_of(&resume);
    assert!(parse_governance_op(&memo).is_some());
    assert!(is_emergency_lifeline(&memo));
}
