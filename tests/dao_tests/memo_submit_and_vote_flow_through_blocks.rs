//! `memo_submit_and_vote_flow_through_blocks` — one test case of the
//! `dao_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn memo_submit_and_vote_flow_through_blocks() {
    // A proposal submitted and voted through ordinary transaction memos
    // must open, close, and execute through the ordinary block path:
    // the memo operation applies only after the carrying transfer
    // executes, and the execution lands in the consensus state the next
    // query reads.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dao(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 1_000);
    chain.state.mine(2_000, pk, 0);
    chain.state.locks.insert(2_000, 1 + DAO_LOCK_MIN);
    chain.state.lock_created.insert(2_000, 0);

    // Block 1: submit the proposal through the memo.
    let submit_outputs = transfer_outputs(&[inputs], [8u8; 32], 100, pk);
    let submit_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[inputs],
        &submit_outputs,
        &DaoOp::SubmitProposal {
            payload: DaoPayload::SetDevFee { bps: 9 },
        },
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![submit_tx]);
    chain.apply(b).expect("submit block applies");
    let id = 1u64;
    assert_eq!(chain.state.dao.proposals[&id].snapshot_height, 1);
    assert_eq!(chain.state.dao.proposals[&id].locked_total, 1);

    // Block 2: vote through the memo; the snapshot-pinned weight of the
    // single locked cell lands on the proposal.
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
    assert_eq!(chain.state.dao.proposals[&id].yes_weight, 1);

    // Shorten the voting window directly in consensus state so the
    // proposal closes on the next block. The close itself stamps the
    // scheduled execution height — the close derives it from the end
    // height, so the execution window is shortened a second time, after
    // the close — and mining the two due blocks carries both transitions:
    // the close flips the proposal to Passed, the execution block runs
    // the payload.
    {
        let p = chain.state.dao.proposals.get_mut(&id).unwrap();
        p.end_height = 3;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    assert_eq!(
        chain.state.dao.proposals[&id].status,
        ProposalStatus::Passed
    );
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
    assert_eq!(chain.state.dev_fee_bps, 9);
}
