//! Tests for reorg persistence behaviors: governance and checkpoint
//! persistence across restarts, and the reorg fail-closed posture when
//! the history is unavailable.

use slash::chain::{Block, BlockError, BlockProcessResult, Chain, Deployment, DeploymentStatus};
use slash::state::State;
use slash::test_fixtures::*;

mod test_reorg_rejected_without_history;
mod test_reorg_succeeds_after_restart;
mod test_reorg_succeeds_with_history;
mod test_restart_persists_governance_and_history;

/// Convert chain transaction outputs to engine outputs.
fn state_outputs(outs: &[slash::chain::Output]) -> Vec<slash::state::Output> {
    outs.iter()
        .map(|o| slash::state::Output {
            start: o.start,
            end: o.end,
            to: o.to,
            lock: o.lock,
        })
        .collect()
}

/// Build a chain with `n` main blocks above genesis.
fn chain_with_blocks(n: u64) -> (Chain, [u8; 32]) {
    let main_miner = [1u8; 32];
    let mut chain = treasury_genesis();
    for _ in 0..n {
        let b = mine_next_block(&chain, main_miner, vec![]);
        chain.apply(b).unwrap();
    }
    (chain, main_miner)
}

/// Build one block extending `prev_hash` at `height` against `state`,
/// mirroring the consensus-internal fork path: coinbase selected against
/// `state`, nonce ground until the PoW slot is treasury-owned in `state`,
/// and the page roots left empty — the caller roots the block against the
/// state it was built on.
fn fork_block(
    state: &State,
    prev_hash: [u8; 32],
    height: u64,
    miner: [u8; 32],
    timestamp: u64,
) -> Block {
    let difficulty = 1;
    let subsidy = slash::chain::subsidy_at(height);
    let (s, e) = state
        .select_single(slash::state::TREASURY, subsidy)
        .expect("treasury must fund the fork coinbase");
    let coinbase_inputs: Vec<(u64, u64)> = vec![(s, e)];
    let coinbase = slash::chain::Tx {
        from: slash::state::TREASURY,
        inputs: coinbase_inputs.clone(),
        outputs: vec![slash::chain::Output {
            start: s,
            end: e,
            to: miner,
            lock: None,
        }],
        sig: Vec::new(),
        scheme: 0,
        memo: None,
    };
    let mut b = Block {
        version: 1,
        prev: prev_hash,
        time: timestamp,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![coinbase],
        fee_claims: Vec::new(),
        difficulty,
        page_roots: std::collections::BTreeMap::new(),
    };
    loop {
        if let Some(cell) =
            slash::chain::verify_pow(b.prev, b.time, b.height, b.miner, b.nonce, b.difficulty)
        {
            let treasury_owned = state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == slash::state::TREASURY);
            let in_tx_inputs = b
                .txs
                .iter()
                .any(|tx| tx.inputs.iter().any(|&(s, e)| cell >= s && cell < e));
            if treasury_owned && !in_tx_inputs {
                b.mined_cell = cell;
                break;
            }
        }
        b.nonce += 1;
    }
    b
}

/// Fill a block's page roots by executing it against the state it was
/// built on: a scratch chain with the given state installed prepares the
/// block, so the header commits to the post-block state of that fork
/// point. The node's own prepare path cannot serve here — it executes
/// against the tip state, where a fork block's coinbase inputs are
/// already spent. The installed state re-wraps the value behind a fresh
/// `Arc` for the scratch chain's exclusive use.
fn rooted(chain: &Chain, state: &State, block: Block) -> Block {
    let mut scratch = chain.clone();
    scratch.state = std::sync::Arc::new(state.clone());
    scratch.prepare_block(block)
}
