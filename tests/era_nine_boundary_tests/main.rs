//! Boundary tests for the first zero-subsidy block (era 9, height 3_784_320).
//!
//! From era 9 onward the coinbase is omitted and the transaction list may be
//! empty, so the coinbase carve-out no longer shields position 0. Every
//! treasury transaction at any position must carry a valid treasury signer
//! signature, and a signed mint must still be paired with an earlier payment
//! in the same block. An ordinary user transaction at position 0 executes
//! through the full transaction pipeline exactly like any other transaction.

use slash::chain::{self, verify_pow, Chain};
use slash::state::{Output, State, TREASURY};
use slash::test_fixtures::*;
use std::collections::BTreeMap;

/// Absolute height of the first era-9 block: the first height whose subsidy
/// is zero and whose coinbase is therefore omitted.
const ERA_NINE_START: u64 = 3_784_320;

/// Build a chain whose expected next height equals `next_height` without
/// replaying millions of blocks: the retained window is moved by setting
/// `base_height` while the block vector starts empty, so the tip formula
/// (`base_height + blocks.len()`) lands on `next_height - 1` and the next
/// block lands exactly on the requested height. Every consensus rule that
/// reads the absolute height (subsidy era, coinbase requirement) evaluates
/// exactly this sum.
fn chain_with_next_height(next_height: u64) -> Chain {
    reset_testnet();
    let mut chain = treasury_genesis();
    chain.base_height = next_height - 1;
    chain
}

/// Build a valid proof-of-work block extending the current tip. The nonce is
/// ground until the proof-of-work slot is treasury-owned in the pre-block
/// state and outside every transaction input, mirroring the consensus rule
/// that a slot may never confiscate a user cell. The returned block is
/// prepared against the chain, so its page roots commit to the post-block
/// state and the block applies without a root mismatch; callers that
/// assemble an intentionally invalid block overwrite the transaction list
/// afterwards and observe the rejection through the ordinary validation
/// path.
fn build_pow_block(chain: &Chain, txs: Vec<chain::Tx>) -> chain::Block {
    let height = chain.tip_height() + 1;
    let mut b = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time: 1_700_001_000 + height,
        height,
        nonce: 0,
        miner: [7u8; 32],
        mined_cell: 0,
        txs,
        fee_claims: Vec::new(),
        difficulty: chain.next_difficulty(),
        page_roots: BTreeMap::new(),
    };
    loop {
        if let Some(cell) = verify_pow(b.prev, b.time, b.height, b.miner, b.nonce, b.difficulty) {
            let treasury_owned = chain
                .state
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
    chain.prepare_block(b)
}

/// A structurally plausible unsigned treasury transaction minting 50 cells
/// from the canonical treasury range. Its signature is empty, so it is valid
/// only if the consensus layer incorrectly treats a non-coinbase position as
/// a minting carve-out.
fn unsigned_treasury_tx(state: &State, to: [u8; 32]) -> chain::Tx {
    let (s, e) = state
        .select_single(TREASURY, 50)
        .expect("treasury owns the supply");
    chain::Tx {
        from: TREASURY,
        inputs: vec![(s, e)],
        outputs: vec![slash::chain::Output {
            start: s,
            end: e,
            to,
            lock: None,
        }],
        sig: Vec::new(),
        scheme: 0,
        memo: None,
    }
}

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod era_nine_empty_block_without_coinbase_is_valid;
mod era_nine_signed_unpaired_mint_at_position_zero_is_rejected;
mod era_nine_unsigned_treasury_tx_at_position_zero_is_rejected;
mod era_nine_user_tx_at_position_zero_executes;
