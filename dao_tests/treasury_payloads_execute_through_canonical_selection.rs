//! `treasury_payloads_execute_through_canonical_selection` — one case of the `dao_tests` suite; shared imports come from
//! the suite root via `use super::*;`.

use super::*;

#[test]
fn treasury_payloads_execute_through_canonical_selection() {
    // TreasuryMint debits the unissued treasury supply through canonical
    // selection, and TreasuryPayout pays out of the vault; both ride the
    // same proposal lifecycle and both amounts land exactly.
    let _tmp = setup_test_dir("dao_treasury_payloads");
    reset_testnet();
    let mut state = slash::state::State::genesis();
    let target = [9u8; 32];
    // Quorum needs a nonzero snapshot denominator: one cell locked before
    // the snapshot supplies it, with both lock maps written.
    let holder = [5u8; 32];
    state.mine(10, holder, 0);
    state.locks.insert(10, 1 + DAO_LOCK_MIN);
    state.lock_created.insert(10, 0);
    let mut dao = DaoState::genesis();
    let total = slash::dao::total_locked_weight(&state, 1);

    let mint_id = dao
        .submit(
            DaoPayload::TreasuryMint {
                amount: 300,
                to: target,
            },
            [1u8; 32],
            1,
            total,
        )
        .expect("submit succeeds");
    dao.vote(mint_id, [1u8; 32], 1, true, 2);
    let end = dao.proposals[&mint_id].end_height;
    dao.tally_at(end);
    dao.execute_due(&mut state, dao.proposals[&mint_id].exec_height);
    assert_eq!(state.balance(target), 300);

    // Fund the vault with one hundred contiguous cells on a pristine
    // page: canonical vault selection needs a single unbroken range, and
    // page 1 starts untouched, so the whole block of cells is one range.
    for cell in 65_536..65_636 {
        state.mine(cell, slash::state::TREASURY_VAULT, 1);
    }

    let payout_id = dao
        .submit(
            DaoPayload::TreasuryPayout {
                amount: 100,
                to: target,
            },
            [1u8; 32],
            3,
            total,
        )
        .expect("submit succeeds");
    dao.vote(payout_id, [1u8; 32], 1, true, 4);
    let end2 = dao.proposals[&payout_id].end_height;
    dao.tally_at(end2);
    dao.execute_due(&mut state, dao.proposals[&payout_id].exec_height);
    assert_eq!(state.balance(target), 400);
}
