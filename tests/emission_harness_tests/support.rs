//! Shared fixtures of the `emission_harness_tests` suite: the per-era
//! chain constructor and the prepared tail block. The suite crosses
//! every halving boundary and at least two difficulty retarget intervals
//! without paying for RandomX proof-of-work.

use slash::chain::{Block, Chain, Output, Tx};

/// Build a chain whose next expected height equals `next_height`: the
/// base height is `next_height - 1` and the block vector starts empty,
/// so the tip formula (`base_height + blocks.len()`) yields
/// `next_height - 1` and the prepared block lands exactly on
/// `next_height`. `next_height` must be at least one: height zero is
/// the genesis itself, which no prepared block can replace.
pub fn chain_with_next_height(next_height: u64) -> Chain {
    assert!(next_height >= 1, "height zero is the genesis itself");
    let mut chain = Chain::genesis();
    chain.base_height = next_height - 1;
    chain
}

/// Build a fully prepared block at the chain's next expected height
/// with an optional coinbase, mirroring the era-9 tail shape: the
/// subsidy is `subsidy_at(height)`, the coinbase (if any) is the
/// mandatory first transaction, and the header carries a valid nonce
/// at the next difficulty. The returned block is prepared against the
/// chain, so its page roots commit to the post-block state exactly as
/// an applied block's do.
pub fn prepared_tail_block(chain: &Chain, miner: [u8; 32], with_coinbase: bool) -> Block {
    // The block extends the tip: base plus the retained count plus one.
    let height = chain.blocks.len() as u64 + chain.base_height + 1;
    let mut block = Block {
        version: 1,
        prev: chain.tip_hash(),
        time: chrono::Utc::now().timestamp() as u64,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![],
        fee_claims: vec![],
        difficulty: chain.next_difficulty(),
        page_roots: std::collections::BTreeMap::new(),
    };
    if with_coinbase {
        let subsidy = slash::chain::subsidy_at(height);
        let inputs: Vec<(u64, u64)> = vec![chain
            .state
            .select_single(slash::state::TREASURY, subsidy)
            .expect("treasury must fund the subsidy")];
        let coinbase = Tx {
            from: slash::state::TREASURY,
            inputs: inputs.clone(),
            outputs: vec![Output {
                start: inputs[0].0,
                end: inputs[0].1,
                to: miner,
                lock: None,
            }],
            sig: vec![],
            scheme: 0,
            memo: None,
        };
        block.txs = vec![coinbase];
    }
    // RandomX over the template inputs until the nonce satisfies the
    // target and the slot stays treasury-owned and out of every
    // transaction input range of the block.
    let mut nonce = 0u64;
    loop {
        block.nonce = nonce;
        let out = slash::chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            block.nonce,
            block.difficulty,
        );
        if let Some(cell) = out {
            let slot_is_treasury = chain
                .state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == slash::state::TREASURY);
            if slot_is_treasury && !inputs_overlap(&block, cell) {
                block.mined_cell = cell;
                break;
            }
        }
        nonce += 1;
    }
    // Prepare against the chain: the header's page roots must commit to
    // the post-block state or application refuses the block with a root
    // mismatch.
    chain.prepare_block(block)
}

/// Whether any transaction input of the block already claims the cell.
fn inputs_overlap(block: &Block, cell: u64) -> bool {
    block
        .txs
        .iter()
        .any(|tx| tx.inputs.iter().any(|&(s, e)| cell >= s && cell < e))
}
