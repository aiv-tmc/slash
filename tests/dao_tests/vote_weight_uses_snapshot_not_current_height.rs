//! `vote_weight_uses_snapshot_not_current_height` — one case of the `dao_tests` suite; shared imports come from
//! the suite root via `use super::*;`.

use super::*;

#[test]
fn vote_weight_uses_snapshot_not_current_height() {
    // Weight is pinned to the proposal snapshot: a cell locked after the
    // snapshot earns nothing on the open proposal, and a cell locked
    // before it keeps its weight. The quorum denominator is captured at
    // submission. Weight requires both lock maps to agree on the cell.
    let _tmp = setup_test_dir("dao_snapshot_weight");
    reset_testnet();
    let mut state = slash::state::State::genesis();
    let early = [1u8; 32];
    let late = [2u8; 32];
    state.mine(10, early, 0);
    state.locks.insert(10, 10 + DAO_LOCK_MIN);
    // The early lock predates every snapshot in this test.
    state.lock_created.insert(10, 0);
    state.mine(20, late, 0);

    let mut dao = DaoState::genesis();
    let total = slash::dao::total_locked_weight(&state, 10);
    let id = dao
        .submit(DaoPayload::SetMineFee { bps: 5 }, early, 10, total)
        .expect("submit succeeds");
    // The late voter locks only after the snapshot: the raw lock entry
    // without a matching creation height never counts.
    state.locks.insert(20, 10 + DAO_LOCK_MIN);
    let w_early = slash::dao::locked_weight(&state, early, 10);
    let w_late = slash::dao::locked_weight(&state, late, 10);
    assert_eq!((w_early, w_late), (1, 0));
    // A vote with zero weight is dropped, so the late voter cannot
    // register either.
    assert!(!dao.vote(id, late, w_late, true, 20));
    assert!(dao.vote(id, early, w_early, true, 20));
}
