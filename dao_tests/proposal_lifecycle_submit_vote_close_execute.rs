//! `proposal_lifecycle_submit_vote_close_execute` — one case of the `dao_tests` suite; shared imports come from
//! the suite root via `use super::*;`.

use super::*;

#[test]
fn proposal_lifecycle_submit_vote_close_execute() {
    // Pure state machine: submit at height 1, vote with snapshot weight,
    // close at end_height, execute at exec_height. The quorum denominator
    // is captured at submission: the locked supply at the snapshot height
    // is measured once, when the proposal opens, and never recomputed.
    let mut state = slash::state::State::genesis();
    let voter = [1u8; 32];
    state.mine(10, voter, 0);
    state.locks.insert(10, 1 + DAO_LOCK_MIN);
    // Weight requires both lock maps to agree on the cell: the expiry
    // above and the creation height below. A creation at height 0 is
    // never above any snapshot, so the cell counts from its snapshot on.
    state.lock_created.insert(10, 0);
    let total = slash::dao::total_locked_weight(&state, 1);
    let mut dao = DaoState::genesis();
    let id = dao
        .submit(DaoPayload::SetDevFee { bps: 7 }, voter, 1, total)
        .expect("submit succeeds");
    assert!(dao.vote(id, voter, 1, true, 5));
    // Second vote from the same address is rejected.
    assert!(!dao.vote(id, voter, 1, false, 6));
    // Close the proposal: 1 vote of 1 locked cell meets quorum and majority.
    let end = dao.proposals[&id].end_height;
    dao.tally_at(end);
    assert_eq!(dao.proposals[&id].status, ProposalStatus::Passed);
    assert_eq!(dao.proposals[&id].exec_height, end + DAO_EXEC_DELAY);
    // Execute at the scheduled height.
    dao.execute_due(&mut state, end + DAO_EXEC_DELAY);
    assert_eq!(dao.proposals[&id].status, ProposalStatus::Executed);
    assert_eq!(state.dev_fee_bps, 7);
}
