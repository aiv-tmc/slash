//! `emergency_pause_freezes_without_halting` — one case of the `dao_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn emergency_pause_freezes_without_halting() {
    // An executed EmergencyPause freezes the flagged class from the next
    // block while the chain keeps moving: coinbase, slot, and fee claims
    // still execute, the paused class is refused in both validation
    // paths, the lifeline — an EmergencyResume submission — stays
    // admissible, and EmergencyResume lifts the freeze before the
    // scheduled expiry.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dao(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 1_000);
    chain.state.mine(2_000, pk, 0);
    chain.state.locks.insert(2_000, 1 + DAO_LOCK_MIN);
    chain.state.lock_created.insert(2_000, 0);

    // Block 1: submit the pause proposal.
    let submit_outputs = transfer_outputs(&[inputs], [8u8; 32], 100, pk);
    let submit_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[inputs],
        &submit_outputs,
        &DaoOp::SubmitProposal {
            payload: DaoPayload::EmergencyPause {
                flags: slash::dao::EMERGENCY_USER_TXS,
                until_height: 1_000,
            },
        },
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![submit_tx]);
    chain.apply(b).expect("submit block applies");
    let id = 1u64;

    // Fund and vote from the high page, shorten the windows, and let the
    // ordinary block path close and execute on schedule. The close
    // derives the scheduled execution height from the end height, so the
    // execution shortening happens after the closing block, not before.
    let inputs2 = (65_536, 66_036);
    for i in inputs2.0..inputs2.1 {
        chain.state.mine(i, pk, 1);
    }
    let vote_outputs = transfer_outputs(&[inputs2], [8u8; 32], 100, pk);
    let vote_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[inputs2],
        &vote_outputs,
        &DaoOp::Vote {
            proposal_id: id,
            approve: true,
        },
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![vote_tx]);
    chain.apply(b).expect("vote block applies");
    {
        let p = chain.state.dao.proposals.get_mut(&id).unwrap();
        p.end_height = 3;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    {
        let p = chain.state.dao.proposals.get_mut(&id).unwrap();
        p.exec_height = 4;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    assert_eq!(
        chain.state.dao.proposals[&id].status,
        ProposalStatus::Executed
    );

    // The freeze is live from the next block: a user transfer is refused
    // by the mempool path. The input is 100 cells wide and the helper
    // pays 90 of them onward with the one-cell mandatory fee, leaving a
    // valid change tail — the refusal must come from the freeze, not
    // from the shape.
    let tip_inputs = (66_036, 66_136);
    for i in tip_inputs.0..tip_inputs.1 {
        chain.state.mine(i, pk, 1);
    }
    let frozen_outputs = transfer_outputs(&[tip_inputs], [8u8; 32], 90, pk);
    let frozen_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[tip_inputs],
        &frozen_outputs,
        &DaoOp::Vote {
            proposal_id: id,
            approve: true,
        },
    );
    assert!(
        !chain.validate_tx(&frozen_tx),
        "the paused class is refused at the mempool"
    );
    // ...while the chain itself does not stop: an empty block applies
    // and pays the miner exactly the subsidy plus slot.
    let miner_before = chain.state.balance([7u8; 32]);
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain
        .apply(b)
        .expect("the chain keeps moving under the pause");
    assert_eq!(chain.state.balance([7u8; 32]), miner_before + 500 + 1);

    // EmergencyResume lifts the freeze before the scheduled expiry, and
    // the same class is admissible again. The resume submission is the
    // lifeline: it is the one class admitted while the freeze is live.
    let resume_outputs = transfer_outputs(&[(66_136, 66_236)], [8u8; 32], 90, pk);
    for i in 66_136..66_236 {
        chain.state.mine(i, pk, 1);
    }
    let resume_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[(66_136, 66_236)],
        &resume_outputs,
        &DaoOp::SubmitProposal {
            payload: DaoPayload::EmergencyResume,
        },
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![resume_tx]);
    chain
        .apply(b)
        .expect("the lifeline applies despite the pause");
    let resume_id = chain.state.dao.next_id - 1;
    let tip = chain.tip_height();
    {
        let p = chain.state.dao.proposals.get_mut(&resume_id).unwrap();
        p.end_height = tip + 1;
        p.yes_weight = 1;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    {
        let p = chain.state.dao.proposals.get_mut(&resume_id).unwrap();
        p.exec_height = tip + 2;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    // The resume proposal must have executed at its scheduled height:
    // this assert separates a close-time failure from an execute-time
    // failure before the flag check below.
    assert_eq!(
        chain.state.dao.proposals[&resume_id].status,
        ProposalStatus::Executed,
        "the resume proposal executed at its scheduled height"
    );
    assert_eq!(chain.state.emergency_flags, 0);
    assert!(
        chain.validate_tx(&frozen_tx),
        "the resumed class is admissible again"
    );
}
