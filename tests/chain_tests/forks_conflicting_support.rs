//! Shared fixtures for the conflicting-reorg tests: the funded keypair,
//! the spend constructors, the stage-tagged balance dump, and the fork
//! block builder that mirrors the heavier-fork reorg path. Kept beside
//! the tests so the test bodies read as scenarios rather than setup.

use super::*;

pub(crate) const STAGE_MAIN: &str = "[forks_conflicting:main-chain]";
pub(crate) const STAGE_BUILD: &str = "[forks_conflicting:build-fork]";
pub(crate) const STAGE_ADOPT: &str = "[forks_conflicting:adoption]";
pub(crate) const STAGE_ASSERT: &str = "[forks_conflicting:assertions]";

/// Deterministic fixture secret for Alice; the public half derives
/// through the ordinary Ed25519 key derivation so signatures verify
/// through the consensus path.
pub(crate) const ALICE_SK: [u8; 32] = [0xA1; 32];

/// Alice's address, derived once per test from the fixture secret.
pub(crate) fn alice_pk() -> [u8; 32] {
    slash::crypto::public_from_secret(&ALICE_SK)
}

/// The cells Alice is funded with on every branch of these tests: 200
/// contiguous cells on page 0, above the canonical coinbase ranges that
/// start at the page base.
pub(crate) const ALICE_CELLS: (u64, u64) = (1000, 1200);

/// Sign a transaction spending Alice's funded cells over the given
/// engine-level outputs through the consensus signature hash.
pub(crate) fn alice_spend(outputs: &[slash::state::Output], chain_id: &[u8]) -> chain::Tx {
    make_tx(alice_pk(), &ALICE_SK, &[ALICE_CELLS], outputs, chain_id)
}

/// An engine output vector paying 180 cells to `recipient` and a 20-cell
/// tail to the fee vault: exactly Alice's 200 funded cells, satisfying
/// the mandatory fee with room to spare.
pub(crate) fn spend_outputs(recipient: [u8; 32]) -> Vec<slash::state::Output> {
    vec![
        slash::state::Output {
            start: ALICE_CELLS.0,
            end: ALICE_CELLS.0 + 180,
            to: recipient,
            lock: None,
        },
        slash::state::Output {
            start: ALICE_CELLS.0 + 180,
            end: ALICE_CELLS.1,
            to: slash::state::FEE_VAULT,
            lock: None,
        },
    ]
}

/// Fund Alice with `ALICE_CELLS` through the test backfill primitive.
pub(crate) fn fund_alice(state: &mut slash::state::State) {
    for cell in ALICE_CELLS.0..ALICE_CELLS.1 {
        state.mine(cell, alice_pk(), 0);
    }
}

/// The serialized form of the retained block window, used to compare
/// chain contents exactly: two block lists are identical precisely when
/// their canonical serializations agree byte for byte.
pub(crate) fn blocks_fingerprint(blocks: &[chain::Block]) -> Vec<u8> {
    bincode::serialize(blocks).expect("block lists serialize")
}

/// Dump every balance the test reasons about, tagged with the stage, so
/// a failure output alone localizes where a divergence appears.
pub(crate) fn dump_balances(
    chain: &chain::Chain,
    tag: &str,
    miner_a: [u8; 32],
    miner_b: [u8; 32],
    bob: [u8; 32],
    carol: [u8; 32],
) {
    eprintln!(
        "{tag} tip={} miner_a={} miner_b={} bob={} carol={} alice={} vault={}",
        chain.tip_height(),
        chain.state.balance(miner_a),
        chain.state.balance(miner_b),
        chain.state.balance(bob),
        chain.state.balance(carol),
        chain.state.balance(alice_pk()),
        chain.state.balance(slash::state::FEE_VAULT),
    );
}

/// Build one fork block at `height` on top of `prev` against the fork
/// base state `scratch`, mirroring the heavier-fork reorg test: the
/// template is mined against the main chain, its height and parent are
/// rewritten to the fork, the fork base state is installed so the nonce
/// ownership check and speculative execution run against exactly the
/// state fork validation replays, and the scratch state is advanced
/// through each valid sibling. Blocks whose transactions cannot execute
/// on the fork branch set `execute_on_scratch` to false: they are ground
/// but not prepared, so the pipeline observes and rejects them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_fork_block(
    chain: &mut chain::Chain,
    scratch: &mut slash::state::State,
    height: u64,
    time: u64,
    prev: [u8; 32],
    miner: [u8; 32],
    txs: Vec<chain::Tx>,
    execute_on_scratch: bool,
    stage: &str,
) -> chain::Block {
    let mut b = mine_valid_block(chain, time, miner, 1);
    b.height = height;
    b.prev = prev;
    // Coinbase selected against the fork base state, not the tip.
    let coinbase = Chain::build_coinbase(scratch, miner, height, 0)
        .unwrap_or_else(|| panic!("{stage} build: coinbase at height {height}"));
    let coinbase_range = coinbase.inputs.first().copied();
    b.txs = std::iter::once(coinbase).chain(txs).collect();
    // Install the fork base state so the nonce ownership check and
    // preparation execute against exactly the state the fork validation
    // replays, then restore the tip state.
    let saved = std::mem::replace(&mut chain.state, scratch.clone());
    grind_nonce(chain, &mut b);
    if let Some((s, e)) = coinbase_range {
        assert!(
            !(b.mined_cell >= s && b.mined_cell < e),
            "{stage} build: slot {} collides with coinbase range {s}..{e}",
            b.mined_cell
        );
    }
    if execute_on_scratch {
        b = chain.prepare_block(b);
        assert!(
            !b.page_roots.is_empty(),
            "{stage} build: preparation left empty page roots at height {height}"
        );
    }
    chain.state = saved;
    if execute_on_scratch {
        // Advance the scratch state through the block for the next
        // sibling before the block is handed to the pipeline.
        *scratch = chain
            .execute_block_state(scratch, &b)
            .unwrap_or_else(|_| panic!("{stage} build: fork block does not execute at {height}"));
    }
    b
}
