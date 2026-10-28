//! The economic simulations: the emission-schedule sweep and the fee-only
//! tail sweep. Both drive the consensus engine directly — the schedule
//! sweep asserts the published per-era supply table, and the tail sweep
//! drives real signed fee payments and miner vault sweeps through live
//! block application, asserting miner revenue, vault drain, and cache
//! parity on every block.

use super::SimulationConfig;
use crate::chain::{Chain, Tx};

/// Economic report emitted by the emission-schedule simulation: the
/// per-era issued supply and the cumulative total.
pub struct EmissionReport {
    /// Issued supply at every era boundary, era 0 first.
    pub era_subsidies: Vec<u64>,
    /// Cumulative issued supply after the last computed era.
    pub total_issued: u64,
    /// Last era index included in the report.
    pub final_era: u64,
}

/// Sweep the emission schedule era by era and return the issued supply of
/// each era. The numbers must match the published emission table exactly;
/// any drift is a consensus bug.
pub fn simulate_emission_schedule() -> EmissionReport {
    let mut era_subsidies = Vec::new();
    let mut total_issued = 0u64;
    let mut era = 0u64;
    loop {
        let subsidy = crate::chain::subsidy_at(era * crate::chain::HALVING_INTERVAL);
        if subsidy == 0 {
            break;
        }
        let era_supply = subsidy * crate::chain::HALVING_INTERVAL;
        total_issued += era_supply;
        era_subsidies.push(era_supply);
        era += 1;
    }
    EmissionReport {
        era_subsidies,
        total_issued,
        final_era: era,
    }
}

/// Run `run_blocks` blocks of fee-funded mining on top of
/// `config.initial_blocks` synthetic pre-mined blocks and report whether
/// every per-block invariant held. Every block carries one user
/// transaction paying the mandatory fee into the vault, and the miner
/// claims the whole unlocked vault in the same block, so the scenario
/// exercises the same fee-claim machinery the perpetual tail depends on.
/// Per block the sweep asserts:
///
/// - the miner's balance grew by exactly subsidy + claimed fees + the
///   one-cell slot mint, so the revenue accounting is exact;
/// - the fee vault ends the block empty, so the sweep drained it;
/// - the balance cache still sums to N, so the cache mirrors the ledger.
///
/// Returns true when every block applied and every invariant held; the
/// first violation aborts the run with a descriptive assertion.
pub fn simulate_fee_only_tail(config: &SimulationConfig, run_blocks: u64) -> bool {
    let mut chain = Chain::genesis();
    for i in 0..config.initial_blocks {
        let miner = config.miners[i as usize % config.miners.len()];
        let time = 1_700_000_000u64 + i * config.target_block_time_secs;
        let block = mine_block(&chain, time, miner, 1, vec![]);
        chain.apply(block).expect("simulation block applies");
    }
    // Fund one user with a page-local range on a pristine page far from
    // the canonical coinbase walk: the engine stages every range on the
    // single page it belongs to, so a range crossing a page boundary
    // never executes. Every block spends the range, returns the change,
    // and pays the mandatory fee to the vault. The transaction is signed
    // for real — execution verifies signatures, so the simulation pays
    // the full consensus cost of the fee path.
    let (user_sk, user) = crate::crypto::generate_keypair();
    let mut range = (65_536u64, 131_072u64);
    for cell in range.0..range.1 {
        chain.state.mine(cell, user, 1);
    }
    // The mandatory fee is ceil(total_out / 200): a truncated division
    // pays one cell short whenever the width is not a multiple of 200,
    // and the fee gate refuses the block from the second iteration on.
    // The run stays well-formed while the range stays above the
    // transaction minimum.
    let max_run = (range.1 - range.0 - crate::state::MIN) * 200 / (range.1 - range.0);
    let run_blocks = run_blocks.min(max_run);
    for i in 0..run_blocks {
        let miner = config.miners[i as usize % config.miners.len()];
        let prev_time = chain
            .get_block(chain.tip_height())
            .map(|b| b.time)
            .unwrap_or(1_700_000_000);
        let time = prev_time + config.target_block_time_secs;
        let height = chain.tip_height() + 1;
        let width = range.1 - range.0;
        let fee = width.div_ceil(crate::state::MIN_FEE_DENOMINATOR);
        let engine_outputs = [
            crate::state::Output {
                start: range.0,
                end: range.0 + fee,
                to: crate::state::FEE_VAULT,
                lock: None,
            },
            crate::state::Output {
                start: range.0 + fee,
                end: range.1,
                to: user,
                lock: None,
            },
        ];
        let hash = Chain::tx_signature_hash(
            &user,
            &[range],
            &engine_outputs,
            crate::chain::MAINNET_ID,
            0,
            &None,
        );
        let sig = crate::crypto::sign(&user_sk, &hash);
        // The miner's sweep must reference cells the vault actually owns
        // when the claims execute — after the fee lands. Stage the
        // transaction on a scratch copy and sweep the scratch vault, so
        // the claims cover the freshly paid fee together with every
        // earlier one and the vault ends the block empty.
        let mut scratch = chain.state.clone();
        scratch.tx(
            user,
            &[range],
            &engine_outputs,
            &sig,
            height,
            crate::chain::MAINNET_ID.as_bytes(),
            0,
            &None,
        );
        let vault_after = scratch.balance(crate::state::FEE_VAULT);
        let claims: Vec<crate::chain::Output> = scratch
            .select_unlocked(crate::state::FEE_VAULT, vault_after, height)
            .into_iter()
            .map(|(s, e)| crate::chain::Output {
                start: s,
                end: e,
                to: miner,
                lock: None,
            })
            .collect();
        let tx = Tx {
            from: user,
            inputs: vec![range],
            outputs: vec![
                crate::chain::Output {
                    start: range.0,
                    end: range.0 + fee,
                    to: crate::state::FEE_VAULT,
                    lock: None,
                },
                crate::chain::Output {
                    start: range.0 + fee,
                    end: range.1,
                    to: user,
                    lock: None,
                },
            ],
            sig,
            scheme: 0,
            memo: None,
        };
        let subsidy = crate::chain::subsidy_at(height);
        let before_miner = chain.state.balance(miner);
        let mut block = mine_block(&chain, time, miner, 1, vec![tx]);
        block.fee_claims = claims;
        // The claims extend the block: re-prepare so the header commits
        // to the post-claim state before applying.
        block = chain.prepare_block(block);
        chain
            .apply(block)
            .expect("fee-funded simulation block applies");
        let revenue = chain.state.balance(miner) - before_miner;
        assert_eq!(
            revenue,
            subsidy + vault_after + 1,
            "miner revenue = subsidy + claimed fees + slot mint"
        );
        assert_eq!(
            chain.state.balance(crate::state::FEE_VAULT),
            0,
            "vault drained by the miner's sweep"
        );
        let supply: u64 = chain.state.balance_cache.values().sum();
        assert_eq!(supply, crate::state::N, "cache parity per block");
        range = (range.0 + fee, range.1);
    }
    true
}

/// Build one block against the chain: a mandatory coinbase when the
/// subsidy is positive, the caller's transactions after it, a nonce
/// search over a light verification context, and a single preparation
/// pass. The search uses the light RandomX context (the same input the
/// verifier hashes) rather than the mining entry point: the full
/// dataset path pins multi-gigabyte contexts per distinct parent hash,
/// and a sweep of many blocks would exhaust node memory for no
/// consensus benefit — the simulation mines at difficulty one, where
/// the first nonce satisfies the target. The slot predicate keeps the
/// slot treasury-owned and out of every transaction input range. Returns
/// the prepared block whether or not the template executed: callers
/// that expect a valid block assert on application, callers that expect
/// a rejection inspect the failure themselves. Crate-visible because
/// the sibling simulation entry points re-export it for their own
/// scenarios.
pub(crate) fn mine_block(
    chain: &Chain,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<Tx>,
) -> crate::chain::Block {
    let height = chain.tip_height() + 1;
    let prev = chain.tip_hash();
    // Mandatory coinbase while the subsidy is positive: the block is
    // invalid without it, in the simulation exactly as in consensus.
    let subsidy = crate::chain::subsidy_at(height);
    let mut all: Vec<Tx> = Vec::new();
    if subsidy > 0 {
        let (s0, e0) = chain
            .state
            .select_single(crate::state::TREASURY, subsidy)
            .expect("treasury funds the coinbase");
        all.push(Tx {
            from: crate::state::TREASURY,
            inputs: vec![(s0, e0)],
            outputs: vec![crate::chain::Output {
                start: s0,
                end: e0,
                to: miner,
                lock: None,
            }],
            sig: vec![],
            scheme: 0,
            memo: None,
        });
    }
    all.extend(txs);
    let input_spans: Vec<(u64, u64)> = all.iter().flat_map(|t| t.inputs.clone()).collect();
    let slot_ok = |slot: u64| {
        chain
            .state
            .owns(crate::state::TREASURY, slot, slot + 1, height)
            && !input_spans.iter().any(|&(s, e)| slot >= s && slot < e)
    };
    // Difficulty one accepts the first nonce; later difficulties scan
    // forward. The verifier rebuilds the input for every candidate, so
    // miner and verifier hash identical buffers by construction.
    let mut nonce = 0u64;
    let (slot, time) = loop {
        match crate::chain::verify_pow(prev, time, height, miner, nonce, difficulty) {
            Some(cell) if slot_ok(cell) => break (cell, time),
            _ => nonce += 1,
        }
    };
    let block = crate::chain::Block {
        version: 1,
        prev,
        time,
        height,
        nonce,
        miner,
        mined_cell: slot,
        txs: all,
        fee_claims: vec![],
        difficulty,
        page_roots: std::collections::BTreeMap::new(),
    };
    chain.prepare_block(block)
}
