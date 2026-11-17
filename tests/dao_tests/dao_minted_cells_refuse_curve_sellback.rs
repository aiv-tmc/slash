//! `dao_minted_cells_refuse_curve_sellback` — one case of the `dao_tests`
//! suite; shared imports come from the suite root via `use super::*;`.

use super::*;

#[test]
fn dao_minted_cells_refuse_curve_sellback() {
    // A governance mint moves cells to a recipient without any payment
    // into the curve vault, so the minted cells must never acquire
    // buy-back rights: selling them back would pay vault funds against
    // no purchase. The transfer engine refuses any treasury-bound output
    // whose cells carry mint provenance, on the mempool path and the
    // block path alike, because both enter through the single `State::tx`
    // gate.
    let _tmp = setup_test_dir("dao_minted_sellback");
    reset_testnet();
    let mut state = slash::state::State::genesis();
    let (to_sk, to_pk) = slash::crypto::generate_keypair();
    let (other_sk, other) = slash::crypto::generate_keypair();

    // Quorum denominator: one cell locked before the snapshot, with both
    // lock maps written.
    let holder = [5u8; 32];
    state.mine(10, holder, 0);
    state.locks.insert(10, 1 + DAO_LOCK_MIN);
    state.lock_created.insert(10, 0);
    let total = slash::dao::total_locked_weight(&state, 1);

    let mut dao = DaoState::genesis();
    // Reserve the exact range the mint will consume: canonical selection
    // is deterministic, so the pre-execution reservation equals the
    // funded range and gives the test the exact cells to probe.
    let (s, e) = state
        .select_single(TREASURY, 300)
        .expect("treasury funds the mint");
    let mint_id = dao
        .submit(
            DaoPayload::TreasuryMint {
                amount: 300,
                to: to_pk,
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
    assert_eq!(state.balance(to_pk), 300);
    assert_eq!(
        dao.proposals[&mint_id].status,
        ProposalStatus::Executed,
        "the mint payload executed"
    );
    assert!(
        dao.minted.contains(s, e),
        "executed governance mint pins its funded range as provenance"
    );

    // Attach the proposal tables to the state: the transfer engine
    // consults `state.dao.minted`, and the test drove the mint through a
    // standalone table. From here on every engine read — the sell-back
    // refusal, the ordinary move, the follow-up refusal from the new
    // owner, and the control below — sees the pinned provenance exactly
    // as block execution would.
    state.dao = dao;

    // The recipient sells the minted range back to the treasury with a
    // fee output: refused by the engine.
    let sell_outputs = vec![
        Output {
            start: s,
            end: s + 295,
            to: TREASURY,
            lock: None,
        },
        Output {
            start: s + 295,
            end: e,
            to: FEE_VAULT,
            lock: None,
        },
    ];
    let sell_hash = slash::chain::Chain::tx_signature_hash(
        &to_pk,
        &[(s, e)],
        &sell_outputs,
        slash::chain::TESTNET_ID.as_bytes(),
        0,
        &None,
    );
    let sell_sig = slash::crypto::sign(&to_sk, &sell_hash);
    assert!(
        !state.tx(
            to_pk,
            &[(s, e)],
            &sell_outputs,
            &sell_sig,
            5,
            slash::chain::TESTNET_ID.as_bytes(),
            0,
            &None
        ),
        "selling governance-minted cells back to the treasury is refused"
    );
    // The refusal leaves the recipient's balance exactly as it was.
    assert_eq!(state.balance(to_pk), 300);

    // Transferring the minted cells to another address is an ordinary
    // transfer and succeeds; provenance follows the cells, so the new
    // owner cannot sell them back either.
    let move_outputs = vec![
        Output {
            start: s,
            end: s + 250,
            to: other,
            lock: None,
        },
        Output {
            start: s + 250,
            end: e,
            to: FEE_VAULT,
            lock: None,
        },
    ];
    let move_hash = slash::chain::Chain::tx_signature_hash(
        &to_pk,
        &[(s, e)],
        &move_outputs,
        slash::chain::TESTNET_ID.as_bytes(),
        0,
        &None,
    );
    let move_sig = slash::crypto::sign(&to_sk, &move_hash);
    assert!(
        state.tx(
            to_pk,
            &[(s, e)],
            &move_outputs,
            &move_sig,
            6,
            slash::chain::TESTNET_ID.as_bytes(),
            0,
            &None
        ),
        "transferring minted cells to another address is an ordinary transfer"
    );
    let tainted_outputs = vec![
        Output {
            start: s,
            end: s + 95,
            to: TREASURY,
            lock: None,
        },
        Output {
            start: s + 95,
            end: s + 100,
            to: FEE_VAULT,
            lock: None,
        },
    ];
    let tainted_hash = slash::chain::Chain::tx_signature_hash(
        &other,
        &[(s, s + 100)],
        &tainted_outputs,
        slash::chain::TESTNET_ID.as_bytes(),
        0,
        &None,
    );
    let tainted_sig = slash::crypto::sign(&other_sk, &tainted_hash);
    assert!(
        !state.tx(
            other,
            &[(s, s + 100)],
            &tainted_outputs,
            &tainted_sig,
            7,
            slash::chain::TESTNET_ID.as_bytes(),
            0,
            &None
        ),
        "provenance follows the cells: the new owner cannot sell them back either"
    );

    // Control: cells issued by ordinary mining on a pristine page carry
    // no provenance, so a regular sell-back to the treasury passes the
    // engine. The control uses a real keypair: the degenerate
    // address-equals-secret pair the lock section used would fail
    // signature verification, because the public key is the derived
    // Edwards point, never the raw secret bytes. The range starts on
    // page 5 (base 5 * 65 536), far outside the minted span and every
    // other owner range, and the balance assert proves every mint
    // landed — mine returns unit, so a silent no-op would otherwise
    // hide here and masquerade as an engine refusal downstream.
    let (control_sk, control_pk) = slash::crypto::generate_keypair();
    let control_base = 5 * 65_536;
    for cell in control_base..control_base + 50 {
        state.mine(cell, control_pk, 0);
    }
    assert_eq!(
        state.balance(control_pk),
        50,
        "control setup: every mint landed on the pristine page"
    );
    assert!(!state.dao.minted.contains(control_base, control_base + 50));
    let clean_outputs = vec![
        Output {
            start: control_base,
            end: control_base + 4,
            to: TREASURY,
            lock: None,
        },
        Output {
            start: control_base + 4,
            end: control_base + 5,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: control_base + 5,
            end: control_base + 50,
            to: control_pk,
            lock: None,
        },
    ];
    let clean_hash = slash::chain::Chain::tx_signature_hash(
        &control_pk,
        &[(control_base, control_base + 50)],
        &clean_outputs,
        slash::chain::TESTNET_ID.as_bytes(),
        0,
        &None,
    );
    let clean_sig = slash::crypto::sign(&control_sk, &clean_hash);
    assert!(
        state.tx(
            control_pk,
            &[(control_base, control_base + 50)],
            &clean_outputs,
            &clean_sig,
            8,
            slash::chain::TESTNET_ID.as_bytes(),
            0,
            &None
        ),
        "ordinary mined cells sell back to the treasury without provenance"
    );
}
