//! `terminal_proposals_prune_after_retention` — one case of the `dao_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn terminal_proposals_prune_after_retention() {
    // Terminal proposals — executed, rejected, expired — survive
    // DAO_TERMINAL_RETENTION blocks past their voting end so their
    // outcome stays observable and auditable on-chain, and are then
    // pruned. Open proposals are never touched by the sweep, votes for a
    // pruned id find no proposal and are dropped silently, and ids are
    // never reused.
    let _tmp = setup_test_dir("dao_prune");
    reset_testnet();
    let mut state = slash::state::State::genesis();
    let holder = [5u8; 32];
    state.mine(10, holder, 0);
    state.locks.insert(10, 1 + DAO_LOCK_MIN);
    state.lock_created.insert(10, 0);
    let total = slash::dao::total_locked_weight(&state, 1);

    let mut dao = DaoState::genesis();
    // A rejected proposal: never voted on, closed without reaching
    // quorum's positive outcome.
    let dead_id = dao
        .submit(DaoPayload::SetDevFee { bps: 11 }, [1u8; 32], 1, total)
        .unwrap();
    dao.tally_at(dao.proposals[&dead_id].end_height);
    assert_eq!(dao.proposals[&dead_id].status, ProposalStatus::Rejected);
    // An executed proposal rides the same lifecycle to completion.
    let live_id = dao
        .submit(DaoPayload::SetMineFee { bps: 7 }, [1u8; 32], 1, total)
        .unwrap();
    dao.vote(live_id, [1u8; 32], 1, true, 2);
    let end = dao.proposals[&live_id].end_height;
    dao.tally_at(end);
    dao.execute_due(&mut state, dao.proposals[&live_id].exec_height);
    assert_eq!(dao.proposals[&live_id].status, ProposalStatus::Executed);
    // A still-open proposal with a far-future end height: pruning must
    // never touch it no matter how far the sweep runs.
    let open_id = dao
        .submit(DaoPayload::SetDevFee { bps: 13 }, [1u8; 32], 1, total)
        .unwrap();
    dao.proposals.get_mut(&open_id).unwrap().end_height = u64::MAX / 2;

    // Inside the retention window every record survives a sweep.
    let dead_end = dao.proposals[&dead_id].end_height;
    let still_inside = dead_end + slash::dao::DAO_TERMINAL_RETENTION;
    dao.close_due(still_inside);
    assert!(dao.proposals.contains_key(&dead_id));
    assert!(dao.proposals.contains_key(&live_id));
    assert!(dao.proposals.contains_key(&open_id));

    // One block past the window the terminal records are gone and the
    // open proposal remains.
    dao.close_due(still_inside + 1);
    assert!(!dao.proposals.contains_key(&dead_id));
    assert!(!dao.proposals.contains_key(&live_id));
    assert!(dao.proposals.contains_key(&open_id));

    // A vote for a pruned id is dropped silently, exactly like a vote
    // for an unknown id.
    assert!(!dao.vote(dead_id, [1u8; 32], 1, true, still_inside + 2));
}
