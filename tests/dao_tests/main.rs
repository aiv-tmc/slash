//! Phase 2 regression tests: the DAO governance layer.
//!
//! Pure state-machine tests pin proposal lifecycle, snapshot weight,
//! double-vote rejection, quorum, and payload execution; chain-level tests
//! pin the memo wiring, the treasury sunset, and execution through real
//! block application. Governance transactions are ordinary transfers with
//! a memo: they pay the standard fee and dev-fund shares.

use slash::chain::{Chain, DeploymentStatus, Tx};
use slash::dao::{DaoOp, DaoPayload, DaoState, ProposalStatus, DAO_EXEC_DELAY, DAO_LOCK_MIN};
use slash::state::{Output, FEE_VAULT, TREASURY};
use slash::test_fixtures::*;

/// Activate the dao_v1 deployment at height 0.
fn activate_dao(chain: &mut Chain) {
    let d = chain
        .version_bits
        .deployments
        .iter_mut()
        .find(|d| d.name == "dao_v1")
        .expect("dao_v1 deployment exists");
    d.status = DeploymentStatus::Active;
    d.activation_height = Some(0);
}

/// Fund an address with `cells` contiguous cells.
fn fund(chain: &mut Chain, owner: [u8; 32], cells: u64) -> (u64, u64) {
    for i in 0..cells {
        chain.state.mine(i, owner, 0);
    }
    (0, cells)
}

/// Build a signed user transaction whose memo carries a governance
/// operation. The memo is committed into the signature hash, so the
/// transaction cannot be re-purposed after signing.
fn governance_tx(
    chain: &Chain,
    sk: &[u8; 32],
    pk: [u8; 32],
    inputs: &[(u64, u64)],
    outputs: &[Output],
    op: &DaoOp,
) -> Tx {
    let memo = bincode::serialize(op).expect("DaoOp serializes");
    // The memo is hashed by reference and also stored in the transaction,
    // so the hashed copy is cloned: the commitment binds the exact bytes
    // that travel with the transaction.
    let hash = slash::chain::Chain::tx_signature_hash(
        &pk,
        inputs,
        outputs,
        &chain.chain_id,
        0,
        &Some(memo.clone()),
    );
    Tx {
        from: pk,
        inputs: inputs.to_vec(),
        outputs: outputs
            .iter()
            .map(|o| slash::chain::Output {
                start: o.start,
                end: o.end,
                to: o.to,
                lock: o.lock,
            })
            .collect(),
        sig: slash::crypto::sign(sk, &hash),
        scheme: 0,
        memo: Some(memo),
    }
}

/// Standard transfer outputs covering `inputs`: `pay` cells to `to`, the
/// mandatory fee to the fee vault, the rest back to the owner. The fee is
/// the ceiling of the full input width divided by the fee denominator,
/// because every output cell counts toward `total_out` and the consensus
/// rule requires `fee * 200 >= total_out`: flooring the division
/// underpays on any width not an exact multiple of 200 (a width of 500
/// would pay 2 where 3 is required), so the fee rounds up and the change
/// tail takes the remainder.
fn transfer_outputs(inputs: &[(u64, u64)], to: [u8; 32], pay: u64, owner: [u8; 32]) -> Vec<Output> {
    let start = inputs[0].0;
    let end = inputs[0].1;
    let width = end - start;
    let fee = width.div_ceil(slash::state::MIN_FEE_DENOMINATOR).max(1);
    vec![
        Output {
            start,
            end: start + pay,
            to,
            lock: None,
        },
        Output {
            start: start + pay,
            end: start + pay + fee,
            to: FEE_VAULT,
            lock: None,
        },
        Output {
            start: start + pay + fee,
            end,
            to: owner,
            lock: None,
        },
    ]
}

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod dao_minted_cells_refuse_curve_sellback;
mod emergency_pause_freezes_without_halting;
mod memo_submit_and_vote_flow_through_blocks;
mod payload_validation_bounds_submission;
mod proposal_lifecycle_submit_vote_close_execute;
mod quorum_failure_rejects_proposal;
mod rotation_payload_replaces_signer_at_execution;
mod terminal_proposals_prune_after_retention;
mod treasury_payloads_execute_through_canonical_selection;
mod treasury_transactions_rejected_while_dao_active;
mod vote_weight_uses_snapshot_not_current_height;
