//! `rotation_payload_replaces_signer_at_execution` — one case of the `dao_tests` suite; shared imports come from
//! the suite root via `use super::*;`.

use super::*;

#[test]
fn rotation_payload_replaces_signer_at_execution() {
    // A passed RotateTreasurySigner proposal replaces the treasury signer
    // field at the scheduled execution height, and only then: the old
    // signer keeps authorizing pairs until the rotation lands.
    let _tmp = setup_test_dir("dao_rotation");
    reset_testnet();
    let mut state = slash::state::State::genesis();
    let old_signer = [3u8; 32];
    state.treasury_signer = old_signer;
    // Quorum needs a nonzero snapshot denominator: one cell locked by a
    // third party before the snapshot supplies it. Both lock maps are
    // written so the weight counts.
    let holder = [5u8; 32];
    state.mine(10, holder, 0);
    state.locks.insert(10, 1 + DAO_LOCK_MIN);
    state.lock_created.insert(10, 0);

    let mut dao = DaoState::genesis();
    let new_signer = [7u8; 32];
    let total = slash::dao::total_locked_weight(&state, 1);
    let id = dao
        .submit(
            DaoPayload::RotateTreasurySigner { public: new_signer },
            [1u8; 32],
            1,
            total,
        )
        .expect("submit succeeds");
    dao.vote(id, [1u8; 32], 1, true, 2);
    let end = dao.proposals[&id].end_height;
    dao.tally_at(end);
    assert_eq!(dao.proposals[&id].status, ProposalStatus::Passed);
    // Before the scheduled height the rotation has not landed.
    assert_eq!(state.treasury_signer, old_signer);
    let exec = dao.proposals[&id].exec_height;
    dao.execute_due(&mut state, exec);
    assert_eq!(dao.proposals[&id].status, ProposalStatus::Executed);
    assert_eq!(state.treasury_signer, new_signer);
}
