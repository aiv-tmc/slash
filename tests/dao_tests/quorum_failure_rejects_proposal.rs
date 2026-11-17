//! `quorum_failure_rejects_proposal` — one case of the `dao_tests` suite; shared imports come from
//! the suite root via `use super::*;`.

use super::*;

#[test]
fn quorum_failure_rejects_proposal() {
    // A majority of votes without quorum still rejects: quorum is measured
    // against the total locked supply captured at submission, not the cast
    // weight, so a whale that abstains keeps raising the bar for everyone
    // else until the whale's own weight is spent voting.
    let mut state = slash::state::State::genesis();
    let whale = [1u8; 32];
    let voter = [2u8; 32];
    state.mine(100, whale, 0);
    state.locks.insert(100, 1 + DAO_LOCK_MIN);
    state.mine(200, voter, 0);
    state.locks.insert(200, 1 + DAO_LOCK_MIN);
    let total = slash::dao::total_locked_weight(&state, 1);
    let mut dao = DaoState::genesis();
    let id = dao
        .submit(DaoPayload::SetDevFee { bps: 3 }, voter, 1, total)
        .unwrap();
    assert!(dao.vote(id, voter, 1, true, 5));
    let end = dao.proposals[&id].end_height;
    dao.tally_at(end);
    assert_eq!(dao.proposals[&id].status, ProposalStatus::Rejected);
}
