//! src/test_fixtures/blocks.rs — the block builders.
//!
//! All searches run through the light verification path (the same
//! RandomX input the verifier hashes), so a fixture block is valid
//! consensus-wide by construction. All times are deterministic (a fixed
//! base plus the height), so a failing test can be replayed bit for bit.

use crate::chain::{self, Chain};

/// The deterministic base timestamp every fixture block builds on: fixed
/// so a failing test can be replayed bit for bit.
pub const BASE_TIME: u64 = 1_700_000_000;

/// Advance the nonce until the proof-of-work slot is treasury-owned in
/// the pre-block state and outside every transaction input range of the
/// block — the slot mint executes before the transactions, so a slot
/// landing inside an input would confiscate that spend and the template
/// could never be valid, exactly as on a real miner.
pub fn grind_nonce(chain: &Chain, block: &mut chain::Block) {
    let mut nonce = 0u64;
    loop {
        if let Some(cell) = chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            nonce,
            block.difficulty,
        ) {
            let treasury_owned = chain
                .state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == crate::state::TREASURY);
            let in_tx_inputs = block
                .txs
                .iter()
                .any(|tx| tx.inputs.iter().any(|&(s, e)| cell >= s && cell < e));
            if treasury_owned && !in_tx_inputs {
                block.nonce = nonce;
                block.mined_cell = cell;
                return;
            }
        }
        nonce += 1;
    }
}

/// Build a valid block template at the next height without preparation:
/// the coinbase is attached, the nonce is ground, and the caller may
/// rewrite height, parent, or transactions before preparing. Fork tests
/// use this to mirror the consensus-internal fork path; ordinary tests
/// prefer `mine_next_block`.
pub fn mine_valid_block(
    chain: &Chain,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
) -> chain::Block {
    let height = chain.tip_height() + 1;
    let coinbase = Chain::build_coinbase(&chain.state, miner, height, 0)
        .unwrap_or_else(|| panic!("coinbase at height {height}"));
    let coinbase_range = coinbase.inputs.first().copied();
    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![coinbase],
        fee_claims: vec![],
        difficulty,
        page_roots: std::collections::BTreeMap::new(),
    };
    grind_nonce(chain, &mut block);
    if let Some((s, e)) = coinbase_range {
        assert!(
            !(block.mined_cell >= s && block.mined_cell < e),
            "slot {} collides with coinbase range {s}..{e}",
            block.mined_cell
        );
    }
    block
}

/// Run the template through preparation, re-grinding the nonce whenever
/// the template fails to execute, until the block carries its page roots
/// or the attempt budget runs out. Panics with the last execution error
/// and a dump of every transaction in the block when no executable block
/// could be produced — the dump identifies the failing transaction
/// without any environment setup. Callers that expect a rejection build
/// the invalid block themselves.
fn prepare_executed(chain: &Chain, mut block: chain::Block) -> chain::Block {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut attempt = 0;
    loop {
        attempt += 1;
        grind_nonce(chain, &mut block);
        let prepared = chain.prepare_block(block);
        block = prepared;
        if block.page_roots.is_empty() && std::time::Instant::now() < deadline && attempt < 200 {
            continue;
        }
        if let Err(e) = chain.execute_block_state(&chain.state, &block) {
            let tx_dump: Vec<String> = block
                .txs
                .iter()
                .enumerate()
                .map(|(i, tx)| {
                    format!(
                        "tx[{i}] from={} inputs={:?} outputs={:?} sig_len={}",
                        hex::encode(tx.from),
                        tx.inputs,
                        tx.outputs
                            .iter()
                            .map(|o| (o.start, o.end, hex::encode(o.to)))
                            .collect::<Vec<_>>(),
                        tx.sig.len()
                    )
                })
                .collect();
            panic!(
                "mine fixture could not produce an executable block: {:?} (height={}, subsidy={}, canonical={:?}, coinbase_inputs={:?}, txs={})",
                e,
                block.height,
                chain::subsidy_at(block.height),
                chain
                    .state
                    .select_single(crate::state::TREASURY, chain::subsidy_at(block.height)),
                block.txs.first().map(|tx| tx.inputs.clone()),
                tx_dump.join(" | ")
            );
        }
        break;
    }
    block
}

/// Mine one block extending the chain tip with the given transactions,
/// mirroring the node template path: coinbase first, then the caller's
/// transactions, and the difficulty taken from the chain's retargeted
/// next difficulty — exactly what a live template declares, so a block
/// built across a retarget boundary automatically carries the rescaled
/// target and validates against it.
pub fn mine_next_block(chain: &Chain, miner: [u8; 32], txs: Vec<chain::Tx>) -> chain::Block {
    let difficulty = chain.next_difficulty();
    let mut block = mine_valid_block(chain, chain.median_timestamp() + 1, miner, difficulty);
    let coinbase = block.txs.clone();
    let all = [coinbase, txs].concat();
    block.txs = all;
    prepare_executed(chain, block)
}

/// Mine one block extending the chain tip at a specific height with the
/// given transactions. The caller names the difficulty explicitly; on
/// retarget boundaries that must be `chain.next_difficulty()` for the
/// block to validate.
pub fn mine_block_at(
    chain: &Chain,
    height: u64,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<chain::Tx>,
) -> chain::Block {
    assert_eq!(height, chain.tip_height() + 1);
    let mut block = mine_valid_block(chain, time, miner, difficulty);
    let coinbase = block.txs.clone();
    let all = [coinbase, txs].concat();
    block.txs = all;
    prepare_executed(chain, block)
}

/// Mine one block without asserting the prepared result: callers that
/// expect an invalid block inspect the failure themselves.
pub fn mine_block_at_unchecked(
    chain: &Chain,
    height: u64,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<chain::Tx>,
) -> chain::Block {
    let mut block = mine_valid_block(chain, time, miner, difficulty);
    block.height = height;
    let coinbase = block.txs.clone();
    let all = [coinbase, txs].concat();
    block.txs = all;
    grind_nonce(chain, &mut block);
    chain.prepare_block(block)
}

/// Push `count` synthetic headers onto the block vector without mining:
/// every header declares difficulty one and advances a mock clock by
/// `time_step` seconds, starting from the tip's time (or the fixture base
/// time for an empty history). Tests that read only timestamps,
/// difficulties, and heights — retarget arithmetic, height advancement, lock
/// spendability — use this instead of paying for thousands of RandomX
/// context initializations, since nothing in those code paths reads the
/// proof, the state transition, or the header linkage.
pub fn push_synthetic_headers(chain: &mut Chain, count: u64, time_step: u64) {
    let mut time = chain.blocks.last().map(|b| b.time).unwrap_or(BASE_TIME);
    for _ in 0..count {
        time += time_step;
        let height = chain.tip_height() + 1;
        chain.blocks.push(chain::Block {
            version: 1,
            prev: [0u8; 32],
            time,
            height,
            nonce: 0,
            miner: [1u8; 32],
            mined_cell: 0,
            txs: Vec::new(),
            fee_claims: Vec::new(),
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        });
    }
}

/// Mine a single cell directly into the state, bypassing block machinery.
pub fn mine_cell_for(state: &mut crate::state::State, cell: u64, owner: [u8; 32]) {
    state.mine(cell, owner, 0);
}
