//! Emission-schedule sweeps: the full era table against `subsidy_at`, and
//! the fee-funded tail. Also the shared synthetic block-mining helper
//! every scenario drives.
//!
//! The tail scenario is the evidence behind the perpetual-tail decision
//! record: a subsidy-less miner's security budget is fees plus the one-cell
//! slot, and the sweep below exercises that accounting end to end — real
//! fee-paying transactions, a real vault sweep claimed by the miner, and
//! the conservation and balance-cache invariants checked on every block.

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
/// - the block applies cleanly through the ordinary consensus path;
/// - the miner's balance grows by exactly the coinbase subsidy (zero from
///   era 9 on), the claimed fees, and the one-cell proof-of-work slot;
/// - the vault ends the block empty and its balance-cache entry is zero;
/// - the balance cache still mirrors the ledger: `sum(cache) == N`.
///
/// The era-9 boundary itself (coinbase omitted, empty payload valid,
/// unsigned treasury tx rejected at zero subsidy) is pinned by the
/// consensus test `test_era_eight_to_nine_transition`; this sweep is the
/// economic twin: it proves the fee path that funds security after the
/// subsidy ends actually moves value to the miner.
pub fn simulate_fee_only_tail(config: &SimulationConfig, run_blocks: u64) -> bool {
    let mut chain = Chain::genesis();
    for i in 0..config.initial_blocks {
        let miner = config.miners[i as usize % config.miners.len()];
        let time = 1_700_000_000u64 + i * config.target_block_time_secs;
        let block = mine_block(&chain, time, miner, 1, vec![]);
        chain.apply(block).expect("simulation block applies");
    }
    // Fund one user with a wide contiguous range on a pristine high page,
    // far from the canonical coinbase walk: every block spends the range,
    // returns the change, and pays the mandatory fee to the vault. The
    // transaction is signed for real: execution verifies signatures, so
    // the simulation pays the full consensus cost of the fee path.
    let (user_sk, user) = crate::crypto::generate_keypair();
    let mut range = (65_536u64, 165_536u64);
    for cell in range.0..range.1 {
        chain.state.mine(cell, user, 1);
    }
    // The mandatory fee is total_out / 200; each block shrinks the range
    // by exactly one fee, so the run stays well-formed only while the
    // range stays above the transaction minimum.
    let max_run = (range.1 - range.0 - crate::state::MIN) * 200 / (range.1 - range.0);
    let run_blocks = run_blocks.min(max_run);
    for i in 0..run_blocks {
        let miner = config.miners[i as usize % config.miners.len()];
        let prev_time = chain
            .get_block(chain.tip_height())
            .map(|b| b.time)
            .unwrap_or(1_700_000_000);
        let time = prev_time + config.target_block_time_secs;
        let width = range.1 - range.0;
        let fee = width / crate::state::MIN_FEE_DENOMINATOR;
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
        let hash =
            Chain::tx_signature_hash(&user, &[range], &engine_outputs, &chain.chain_id, 0, &None);
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
            sig: crate::crypto::sign(&user_sk, &hash),
            scheme: 0,
            memo: None,
        };
        let mut block = mine_block(&chain, time, miner, 1, vec![tx]);
        // The miner claims the whole vault in the same block: the fee
        // lands in the vault when the transaction executes, and the sweep
        // executes after the transaction list, so the claim covers it.
        let vault_after = chain.state.balance(crate::state::FEE_VAULT) + fee;
        if vault_after > 0 {
            block.fee_claims = vec![crate::chain::Output {
                start: 0,
                end: vault_after,
                to: miner,
                lock: None,
            }];
            block = chain.prepare_block(block);
        }
        let subsidy = crate::chain::subsidy_at(block.height);
        let miner_before = chain.state.balance(miner);
        chain
            .apply(block.clone())
            .expect("fee-funded simulation block applies");
        range = (range.0 + fee, range.1);
        let revenue = chain.state.balance(miner) - miner_before;
        if revenue != subsidy + vault_after + 1 {
            return false;
        }
        if chain.state.balance(crate::state::FEE_VAULT) != 0 {
            return false;
        }
        let supply: u64 = chain.state.balance_cache.values().sum();
        if supply != crate::state::N {
            return false;
        }
    }
    chain.tip_height() == config.initial_blocks + run_blocks
}

/// Mine a single block on top of the given chain at the given time with
/// the given transactions, using difficulty 1 and a synthetic coinbase.
pub fn mine_block(
    chain: &Chain,
    time: u64,
    miner: [u8; 32],
    difficulty: u64,
    txs: Vec<Tx>,
) -> crate::chain::Block {
    let height = chain.tip_height() + 1;
    let mut full_txs = Vec::new();
    if let Some(coinbase) = Chain::build_coinbase(&chain.state, miner, height, 0) {
        full_txs.push(coinbase);
    }
    full_txs.extend(txs);
    let mut block = crate::chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: full_txs,
        fee_claims: Vec::new(),
        difficulty,
        page_roots: Default::default(),
    };
    // Grind the nonce until the PoW slot cell is treasury-owned. The
    // light verification context keeps the search cheap; the full mining
    // dataset is never allocated inside a simulation.
    loop {
        if let Some(cell) = crate::chain::verify_pow(
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
                .is_some_and(|(_, _, owner)| owner == crate::state::TREASURY);
            if owned_by_treasury {
                block.mined_cell = cell;
                break;
            }
        }
        block.nonce += 1;
    }
    chain.prepare_block(block)
}
