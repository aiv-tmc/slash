//! Block fixtures: coinbase construction, deterministic mining, arbitrary
//! branch templates, synthetic retarget headers, and template preparation
//! through the public execution pipeline.
//!
//! These helpers are the canonical way for tests to obtain executable
//! blocks: executable fixture paths are prepared through
//! `Chain::prepare_block`, while unchecked paths intentionally return raw
//! templates so negative consensus tests can feed malformed blocks into
//! `apply` and observe the rejection.

use super::{chain, state};

/// Backfill one cell for a test owner. The helper is a thin, searchable
/// name around the state-engine mine operation so test setup reads as
/// scenario data instead of engine mechanics.
pub fn mine_cell_for(state: &mut state::State, cell: u64, owner: [u8; 32]) {
    state.mine(cell, owner, 0);
}

/// Build a genesis block whose coinbase pays the miner the full era-0
/// subsidy. Used by the fixture `Chain::genesis()` so every test starts
/// from an ordinary executable block rather than a hand-crafted partial
/// one.
pub fn build_genesis_coinbase(miner: [u8; 32]) -> chain::Tx {
    let outputs = vec![
        chain::Output {
            start: 0,
            end: 250,
            to: miner,
            lock: None,
        },
        chain::Output {
            start: 250,
            end: 500,
            to: state::TREASURY,
            lock: None,
        },
    ];
    chain::Tx {
        from: state::TREASURY,
        inputs: vec![(0, 500)],
        outputs,
        sig: Vec::new(),
        scheme: 0,
        memo: None,
    }
}

/// Build `count` ordinary transactions, each spending a fresh disjoint
/// input range and paying exactly the mandatory fee. The transaction
/// builder comes from the shared transaction fixture module; this file
/// only fixes the deterministic input ranges and recipients.
pub fn make_txs_for(chain: &chain::Chain, count: usize) -> Vec<chain::Tx> {
    let (sk, owner) = super::test_keypair(9);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = i as u64 * 100;
        let end = start + 100;
        let outputs = [
            (start, end - 1, [7u8; 32]),
            (end - 1, end, state::FEE_VAULT),
        ];
        out.push(super::make_tx_for(
            chain,
            owner,
            &sk,
            &[(start, end)],
            &outputs,
            0,
        ));
    }
    out
}

/// Mine a valid raw template for the next main-chain block. The template
/// carries the canonical coinbase selected against the current tip state,
/// the given time and difficulty, and a nonce whose proof-of-work slot is
/// treasury-owned and outside every transaction input in the block,
/// including the coinbase inputs. The returned block is intentionally not
/// prepared: callers that need executed page roots use `mine_next_block`
/// or `mine_block_at`, while negative tests can mutate and apply the raw
/// template directly.
pub fn mine_valid_block(
    chain: &chain::Chain,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
) -> chain::Block {
    assert!(difficulty > 0, "difficulty must be positive");
    let height = chain.tip_height() + 1;
    let coinbase = chain::Chain::build_coinbase(&chain.state, miner, height, 0)
        .unwrap_or_else(|| panic!("coinbase at height {height}"));
    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![coinbase],
        fee_claims: Vec::new(),
        difficulty,
        page_roots: std::collections::BTreeMap::new(),
    };
    grind_nonce(chain, &mut block);
    block
}

/// Prepare an already-mined template. The nonce is re-ground after the
/// final transaction list is known, because appending transactions can
/// introduce new input spans the slot must avoid; preparation then runs
/// the full block through speculative execution and fills page roots.
fn prepare_executed(chain: &chain::Chain, block: &chain::Block) -> chain::Block {
    let mut block = block.clone();
    grind_nonce(chain, &mut block);
    let prepared = chain::Chain::prepare_block(chain, block);
    if prepared.page_roots.is_empty() {
        let cb = &prepared.txs[0];
        let canonical =
            chain::Chain::build_coinbase(&chain.state, prepared.miner, prepared.height, 0)
                .map(|cb| cb.inputs.first().copied());
        panic!(
            "mine fixture could not produce an executable block: InvalidTx (height={}, subsidy={}, canonical={:?}, coinbase_inputs={:?}, txs={})",
            prepared.height,
            crate::chain::subsidy_at(prepared.height),
            canonical,
            cb.inputs.first().copied(),
            prepared
                .txs
                .iter()
                .enumerate()
                .map(|(i, tx)| format!(
                    "tx[{i}] from={} inputs={:?} outputs={:?} sig_len={}",
                    hex::encode(tx.from),
                    tx.inputs,
                    tx.outputs,
                    tx.sig.len()
                ))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    prepared
}

/// Build and fully prepare the next block. Preparation executes the block
/// through the consensus pipeline and fills the page roots; a failure
/// panics with enough context to diagnose the fixture.
pub fn mine_next_block(chain: &chain::Chain, miner: [u8; 32], txs: Vec<chain::Tx>) -> chain::Block {
    let height = chain.tip_height() + 1;
    let template =
        mine_block_at_unchecked(chain, height, height, miner, chain.next_difficulty(), txs);
    prepare_executed(chain, &template)
}

/// Build a prepared block at the next main-chain height. The height check
/// is explicit so accidental off-by-one branch setup fails at the fixture
/// boundary instead of surfacing as a consensus error.
pub fn mine_block_at(
    chain: &chain::Chain,
    height: u64,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<chain::Tx>,
) -> chain::Block {
    assert_eq!(
        height,
        chain.tip_height() + 1,
        "mine_block_at builds the next main-chain block"
    );
    prepare_executed(
        chain,
        &mine_block_at_unchecked(chain, height, time, miner, difficulty, txs),
    )
}

/// Build a raw mined template without asserting that `height` is the next
/// main-chain height and without executing it. Negative consensus tests
/// use this helper to inject structurally unusual blocks; `apply` or
/// `validate_block` observes and rejects invalid content.
pub fn mine_block_at_unchecked(
    chain: &chain::Chain,
    height: u64,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<chain::Tx>,
) -> chain::Block {
    assert!(difficulty > 0, "difficulty must be positive");
    let coinbase = chain::Chain::build_coinbase(&chain.state, miner, height, 0)
        .unwrap_or_else(|| panic!("coinbase at height {height}"));
    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: std::iter::once(coinbase).chain(txs).collect(),
        fee_claims: Vec::new(),
        difficulty,
        page_roots: std::collections::BTreeMap::new(),
    };
    grind_nonce(chain, &mut block);
    block
}

/// Append `count` synthetic header blocks with monotonically increasing
/// timestamps spaced `step` seconds apart. The helper updates only the
/// block vector and the header fields difficulty arithmetic reads: it
/// deliberately does not execute transactions, update state, or record
/// history, which keeps long retarget windows cheap to construct.
pub fn push_synthetic_headers(chain: &mut chain::Chain, count: u64, step: u64) {
    let mut time = chain.blocks.last().map(|block| block.time).unwrap_or(0);
    for _ in 0..count {
        time += step;
        let height = chain.tip_height() + 1;
        let difficulty = chain.difficulty_for_height(height);
        chain.blocks.push(chain::Block {
            version: 1,
            prev: chain.tip_hash(),
            time,
            height,
            nonce: 0,
            miner: [0u8; 32],
            mined_cell: 0,
            txs: Vec::new(),
            fee_claims: Vec::new(),
            difficulty,
            page_roots: std::collections::BTreeMap::new(),
        });
    }
}

/// Grind a nonce whose proof-of-work slot is treasury-owned and outside
/// every transaction input span in the block. Including the coinbase is
/// essential: the coinbase debits a canonical treasury range, and a slot
/// inside that range would reassign one of its cells before the coinbase
/// mint runs.
pub fn grind_nonce(chain: &chain::Chain, block: &mut chain::Block) {
    let input_spans: Vec<(u64, u64)> = block
        .txs
        .iter()
        .flat_map(|tx| tx.inputs.iter().copied())
        .collect();
    loop {
        if let Some(cell) = chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            block.nonce,
            block.difficulty,
        ) {
            let treasury_owned = cell < state::N
                && chain
                    .state
                    .get(cell)
                    .is_some_and(|(_, _, owner)| owner == state::TREASURY);
            let collides_with_inputs = input_spans
                .iter()
                .any(|&(start, end)| cell >= start && cell < end);
            if treasury_owned && !collides_with_inputs {
                block.mined_cell = cell;
                break;
            }
        }
        block.nonce = block.nonce.wrapping_add(1);
    }
}

/// Convert engine outputs into wire outputs.
pub fn wire_outputs(engine: &[state::Output]) -> Vec<chain::Output> {
    engine
        .iter()
        .map(|o| chain::Output {
            start: o.start,
            end: o.end,
            to: o.to,
            lock: o.lock,
        })
        .collect()
}

/// Convert wire outputs into engine outputs. The shared `state_outputs`
/// helper in the transaction fixture module is the canonical name for
/// this conversion; this alias exists for fixture call sites that use
/// the older `engine_outputs` spelling.
pub fn engine_outputs(wire: &[chain::Output]) -> Vec<state::Output> {
    super::state_outputs(wire)
}
