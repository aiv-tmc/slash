//! `memo_submit_and_vote_flow_through_blocks` — one case of the `dao_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn memo_submit_and_vote_flow_through_blocks() {
    // Submit and vote ride ordinary transfer memos, execute through real
    // block application, and follow the full lifecycle: weight pinned
    // at the snapshot height, first-vote-wins, close at the window end,
    // execution at the scheduled height.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dao(&mut chain);
    let (sk, pk) = slash::crypto::generate_keypair();
    let inputs = fund(&mut chain, pk, 1_000);
    // The governance weight cell, locked past the snapshot horizon.
    chain.state_mut().mine(2_000, pk, 0);
    chain.state_mut().locks.insert(2_000, 1 + DAO_LOCK_MIN);
    chain.state_mut().lock_created.insert(2_000, 0);

    // Block 1: submit the payout proposal.
    let submit_outputs = transfer_outputs(&[inputs], [8u8; 32], 100, pk);
    let submit_tx = governance_tx(
        &chain,
        &sk,
        pk,
        &[inputs],
        &submit_outputs,
        &DaoOp::SubmitProposal {
            payload: DaoPayload::TreasuryPayout {
                amount: 50,
                to: slash::state::TREASURY_VAULT,
            },
        },
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![submit_tx]);
    chain.apply(b).expect("submit block applies");
    let id = 1u64;
    assert_eq!(
        chain.state.dao.proposals[&id].status,
        ProposalStatus::Voting
    );

    // Block 2: fund and vote from a fresh range on another page.
    let inputs2 = (65_536, 66_036);
    for i in inputs2.0..inputs2.1 {
        chain.state_mut().mine(i, pk, 1);
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
    // Shorten the windows so the ordinary block path closes and executes
    // on schedule within the test's runtime.
    {
        let p = chain.state_mut().dao.proposals.get_mut(&id).unwrap();
        p.end_height = 3;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();
    {
        let p = chain.state_mut().dao.proposals.get_mut(&id).unwrap();
        p.exec_height = 4;
    }
    let b = mine_next_block(&chain, [7u8; 32], vec![]);
    chain.apply(b).unwrap();

    assert_eq!(
        chain.state.dao.proposals[&id].status,
        ProposalStatus::Executed
    );
    // The first-vote-wins rule rejected the second vote from the same
    // address: the tally still carries exactly one vote.
    assert_eq!(chain.state.dao.proposals[&id].yes_weight, 1);
    assert_eq!(chain.state.dao.proposals[&id].no_weight, 0);
}
