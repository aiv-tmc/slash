//! Block builders: the nonce search and the three block constructors every
//! suite drives. All searches run through the light verification path.
//!
//! The builders re-grind while a candidate block fails speculative
//! execution, but never for longer than a hard time budget: a block whose
//! transactions are invalid is rejected by consensus no matter how many
//! nonces the miner burns, so the builder must return it instead of
//! spinning. Execution failure within the budget means the proof slot
//! landed inside the coinbase's canonical treasury range (or another
//! consensus gate fired); the next nonce is a fresh, independent roll.

use super::{chain, state};

/// The total time the builder may spend re-rolling a non-executable
/// candidate before returning it for the caller's ordinary rejection path.
const EXECUTION_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);

/// Search a nonce satisfying the difficulty for the given header fields
/// and a treasury-owned PoW slot cell. The consensus input is
/// `prev || time || height || nonce || miner`; the slot is the first
/// eight bytes of the RandomX output taken modulo the cell count.
pub fn grind_nonce(chain: &chain::Chain, block: &mut chain::Block) {
    loop {
        if let Some(cell) = chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            block.nonce,
            block.difficulty,
        ) {
            let owned_by_treasury = chain
                .state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == state::TREASURY);
            if owned_by_treasury {
                block.mined_cell = cell;
                return;
            }
        }
        block.nonce += 1;
    }
}

/// Prepare a block, re-grinding the nonce while speculative execution
/// fails, bounded by the hard time budget. Blocks whose transactions
/// consensus rejects are returned with empty roots after the budget, so
/// the caller observes the rejection through the ordinary apply path
/// instead of the builder spinning forever.
fn prepare_executed(chain: &chain::Chain, mut block: chain::Block) -> chain::Block {
    let deadline = std::time::Instant::now() + EXECUTION_RETRY_BUDGET;
    loop {
        let prepared = chain.prepare_block(block);
        if !prepared.page_roots.is_empty() || std::time::Instant::now() >= deadline {
            return prepared;
        }
        block = prepared;
        block.nonce += 1;
        grind_nonce(chain, &mut block);
    }
}

/// Build a proof-of-work-satisfying block extending the chain tip with an
/// explicit timestamp and difficulty. The block carries no transactions
/// and is NOT applied; the caller amends it or applies it explicitly.
pub fn mine_valid_block(
    chain: &chain::Chain,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
) -> chain::Block {
    let height = chain.tip_height() + 1;
    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![],
        fee_claims: vec![],
        difficulty,
        page_roots: Default::default(),
    };
    grind_nonce(chain, &mut block);
    chain.prepare_block(block)
}

/// Build (but do not apply) the next block paying `miner`, carrying the
/// mandatory coinbase when the subsidy is positive plus the supplied
/// transactions, timestamped one second past the median-time window and
/// at the retargeted difficulty.
pub fn mine_next_block(chain: &chain::Chain, miner: [u8; 32], txs: Vec<chain::Tx>) -> chain::Block {
    let height = chain.tip_height() + 1;
    let time = chain.median_timestamp() + 1;
    let difficulty = chain.next_difficulty();
    let mut block = mine_valid_block(chain, time, miner, difficulty);
    let mut all = Vec::with_capacity(txs.len() + 1);
    if let Some(coinbase) = chain::Chain::build_coinbase(&chain.state, miner, height, 0) {
        all.push(coinbase);
    }
    all.extend(txs);
    block.txs = all;
    prepare_executed(chain, block)
}

/// Build (but do not apply) a block at an explicit height, timestamp, and
/// difficulty, carrying the mandatory coinbase when the subsidy is
/// positive plus the supplied transactions. The parent link follows the
/// retained ancestor at `height - 1` when it exists and the tip
/// otherwise, so tests can construct in-flight fork blocks and amend the
/// parent explicitly.
pub fn mine_block_at(
    chain: &chain::Chain,
    height: u64,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<chain::Tx>,
) -> chain::Block {
    let index = height.saturating_sub(1 + chain.base_height) as usize;
    let prev = if index < chain.blocks.len() {
        chain.block_hash(&chain.blocks[index])
    } else {
        chain.tip_hash()
    };
    let mut block = mine_valid_block(chain, time, miner, difficulty);
    block.height = height;
    block.prev = prev;
    grind_nonce(chain, &mut block);
    let mut all = Vec::with_capacity(txs.len() + 1);
    if let Some(coinbase) = chain::Chain::build_coinbase(&chain.state, miner, height, 0) {
        all.push(coinbase);
    }
    all.extend(txs);
    block.txs = all;
    prepare_executed(chain, block)
}

/// Assign one treasury-owned cell directly to `owner` in the state,
/// bypassing block production entirely. Fixtures name the exact cell so
/// input sets stay deterministic and disjoint across owners.
pub fn mine_cell_for(state: &mut state::State, cell: u64, owner: [u8; 32]) {
    state.mine(cell, owner, 0);
}
