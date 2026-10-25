//! Regression tests for reorganization safety and restart persistence.
//!
//! These tests pin the three consensus-critical guarantees: a reorg that
//! would roll back past the available state history is rejected without
//! touching the chain; a reorg within the available history succeeds and
//! replaces the tip; and a restarted node restores its deployments,
//! checkpoints, and exact reorg window.

use slash::chain::{Block, BlockError, BlockProcessResult, Chain, Deployment, DeploymentStatus};
use slash::state;
use slash::test_fixtures::*;

/// Build a block that extends `prev_hash` at `height` with a coinbase
/// computed against `pre_state`, grinding the nonce until the proof-of-work
/// slot lands on a treasury-owned cell outside the coinbase inputs. This
/// mirrors the consensus rule that a slot may never confiscate a user cell
/// or collide with the coinbase range.
fn fork_block(
    pre_state: &state::State,
    prev_hash: [u8; 32],
    height: u64,
    miner: [u8; 32],
    time: u64,
) -> Block {
    let coinbase = Chain::build_coinbase(pre_state, miner, height, 0);
    let range = coinbase.as_ref().and_then(|c| c.inputs.first().copied());
    let mut txs: Vec<slash::chain::Tx> = Vec::new();
    if let Some(cb) = coinbase {
        txs.push(cb);
    }
    let mut b = Block {
        version: 1,
        prev: prev_hash,
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs,
        fee_claims: vec![],
        difficulty: 1,
        page_roots: Default::default(),
    };
    loop {
        if let Some(cell) =
            slash::chain::verify_pow(b.prev, b.time, b.height, b.miner, b.nonce, b.difficulty)
        {
            let hits_coinbase = range.is_some_and(|(s, e)| cell >= s && cell < e);
            let treasury_owned = pre_state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == state::TREASURY);
            if !hits_coinbase && treasury_owned {
                b.mined_cell = cell;
                return b;
            }
        }
        b.nonce += 1;
    }
}

/// Advance a fresh treasury-signer chain by `n` empty blocks and return it.
fn chain_with_blocks(n: usize) -> (Chain, [u8; 32]) {
    let mut chain = treasury_genesis();
    let miner = [1u8; 32];
    for _ in 0..n {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    (chain, miner)
}

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod test_reorg_rejected_without_history;
mod test_reorg_succeeds_after_restart;
mod test_reorg_succeeds_with_history;
mod test_restart_persists_governance_and_history;
