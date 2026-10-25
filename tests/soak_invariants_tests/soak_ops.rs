//! One deterministic soak operation: the fixed-seed generator selects
//! among a user transfer, a self-lock, a mock-clock advance with lock
//! pruning, a treasury buy pair, a treasury sell pair, and a treasury
//! mine. Early-outs before anything is applied return false so the
//! driver skips the invariant block for that iteration, exactly as the
//! inline loop's `continue` did; a completed arm returns true. The
//! draw order against the seeded generator is unchanged.

use super::*;

/// Draw one operation against the seeded generator and apply it. The
/// clock advance mutates the driver-owned height through the mutable
/// reference so the operation order — and therefore the deterministic
/// sequence — matches the replay exactly.
pub fn soak_op(
    state: &mut State,
    rng: &mut Rng,
    owners: &[([u8; 32], [u8; 32])],
    bases: &[u64],
    current_height: &mut u64,
    treasury_sk: &[u8; 32],
) -> bool {
    let op = rng.range(6);
    match op {
        // Transfer a chunk between two owners, paying the mandatory fee.
        0 => {
            let idx = rng.range(owners.len() as u64) as usize;
            let (sk, from) = owners[idx];
            let base = bases[idx];
            let len = 50 + rng.range(200);
            if len >= 5_000 {
                return false;
            }
            let start = base + rng.range(5_000 - len);
            let recipient = owners[rng.range(owners.len() as u64) as usize].1;
            let fee = len / MIN_FEE_DENOMINATOR + 1;
            let outputs = vec![
                Output {
                    start,
                    end: start + len - fee,
                    to: recipient,
                    lock: None,
                },
                Output {
                    start: start + len - fee,
                    end: start + len,
                    to: FEE_VAULT,
                    lock: None,
                },
            ];
            let inputs = vec![(start, start + len)];
            let hash = chain::Chain::tx_signature_hash(
                &from,
                &inputs,
                &outputs,
                chain::MAINNET_ID,
                0,
                &None,
            );
            let sig = crypto::sign(&sk, &hash);
            // A rejection is a valid outcome; the invariants below must
            // hold either way.
            let _ = state.tx(
                from,
                &inputs,
                &outputs,
                &sig,
                *current_height,
                chain::MAINNET_ID.as_bytes(),
                0,
                &None,
            );
        }
        // Lock a chunk of own cells until a future height.
        1 => {
            let idx = rng.range(owners.len() as u64) as usize;
            let (sk, from) = owners[idx];
            let base = bases[idx];
            let len = 50 + rng.range(200);
            if len >= 5_000 {
                return false;
            }
            let start = base + rng.range(5_000 - len);
            let lock_height = *current_height + 10 + rng.range(100);
            let fee = len / MIN_FEE_DENOMINATOR + 1;
            let outputs = vec![
                Output {
                    start,
                    end: start + len - fee,
                    to: from,
                    lock: Some(lock_height),
                },
                Output {
                    start: start + len - fee,
                    end: start + len,
                    to: FEE_VAULT,
                    lock: None,
                },
            ];
            let inputs = vec![(start, start + len)];
            let hash = chain::Chain::tx_signature_hash(
                &from,
                &inputs,
                &outputs,
                chain::MAINNET_ID,
                0,
                &None,
            );
            let sig = crypto::sign(&sk, &hash);
            let _ = state.tx(
                from,
                &inputs,
                &outputs,
                &sig,
                *current_height,
                chain::MAINNET_ID.as_bytes(),
                0,
                &None,
            );
        }
        // Advance the mock clock and prune expired locks.
        2 => {
            *current_height += 1 + rng.range(5);
            state.prune_locks(*current_height);
        }
        // Treasury buy pair: pay TREASURY_VAULT, then mint treasury cells
        // to the buyer and advance total_sold exactly as block execution
        // does for a paired T_mint.
        3 => {
            let idx = rng.range(owners.len() as u64) as usize;
            let (buyer_sk, buyer) = owners[idx];
            let base = bases[idx];
            let amount = 50 + rng.range(200);
            let cost = treasury::buy_price(amount, state.total_sold) + 10;
            let fee = cost / MIN_FEE_DENOMINATOR + 2;
            let total_out = cost + fee + rng.range(20).max(1);
            if total_out >= 5_000 {
                return false;
            }
            let start = base + rng.range(5_000 - total_out);
            let buy_outputs = vec![
                Output {
                    start,
                    end: start + cost,
                    to: TREASURY_VAULT,
                    lock: None,
                },
                Output {
                    start: start + cost,
                    end: start + cost + fee,
                    to: FEE_VAULT,
                    lock: None,
                },
                Output {
                    start: start + cost + fee,
                    end: start + total_out,
                    to: buyer,
                    lock: None,
                },
            ];
            let buy_inputs = vec![(start, start + total_out)];
            let buy_hash = chain::Chain::tx_signature_hash(
                &buyer,
                &buy_inputs,
                &buy_outputs,
                chain::MAINNET_ID,
                0,
                &None,
            );
            let buy_sig = crypto::sign(&buyer_sk, &buy_hash);
            if !state.tx(
                buyer,
                &buy_inputs,
                &buy_outputs,
                &buy_sig,
                *current_height,
                chain::MAINNET_ID.as_bytes(),
                0,
                &None,
            ) {
                return false;
            }
            let mint_inputs = match state.select_single(TREASURY, amount) {
                Some(r) => vec![r],
                None => return false,
            };
            let mint_outputs = vec![Output {
                start: mint_inputs[0].0,
                end: mint_inputs[0].1,
                to: buyer,
                lock: None,
            }];
            let mint_hash = chain::Chain::tx_signature_hash(
                &TREASURY,
                &mint_inputs,
                &mint_outputs,
                chain::MAINNET_ID,
                0,
                &None,
            );
            let _mint_sig = crypto::sign(treasury_sk, &mint_hash);
            // State::mint applies the transfer; the signature itself is
            // enforced by Chain::execute_block_txs in consensus, so the
            // soak exercises the state transition and the bookkeeping.
            if state.mint(&mint_inputs, &mint_outputs, *current_height) {
                state.total_sold += amount;
            }
        }
        // Treasury sell pair: transfer cells to TREASURY, then pay the
        // seller out of TREASURY_VAULT and advance total_bought_back.
        4 => {
            let idx = rng.range(owners.len() as u64) as usize;
            let (seller_sk, seller) = owners[idx];
            let base = bases[idx];
            let amount = 50 + rng.range(200);
            let fee = amount / MIN_FEE_DENOMINATOR + 1;
            let total_out = amount + fee;
            if total_out >= 5_000 {
                return false;
            }
            let start = base + rng.range(5_000 - total_out);
            let sell_outputs = vec![
                Output {
                    start,
                    end: start + amount,
                    to: TREASURY,
                    lock: None,
                },
                Output {
                    start: start + amount,
                    end: start + total_out,
                    to: FEE_VAULT,
                    lock: None,
                },
            ];
            let sell_inputs = vec![(start, start + total_out)];
            let sell_hash = chain::Chain::tx_signature_hash(
                &seller,
                &sell_inputs,
                &sell_outputs,
                chain::MAINNET_ID,
                0,
                &None,
            );
            let sell_sig = crypto::sign(&seller_sk, &sell_hash);
            if !state.tx(
                seller,
                &sell_inputs,
                &sell_outputs,
                &sell_sig,
                *current_height,
                chain::MAINNET_ID.as_bytes(),
                0,
                &None,
            ) {
                return false;
            }
            let payout = treasury::sell_price(amount, state.total_sold);
            let payout_inputs = match state.select_single(TREASURY_VAULT, payout) {
                Some(r) => vec![r],
                None => return false,
            };
            let payout_outputs = vec![Output {
                start: payout_inputs[0].0,
                end: payout_inputs[0].1,
                to: seller,
                lock: None,
            }];
            if state.payout(&payout_inputs, &payout_outputs, *current_height) {
                state.total_bought_back += amount;
            }
        }
        // Mine a random unissued cell for a random owner.
        _ => {
            let cell = rng.range(N);
            if let Some((_, _, owner)) = state.get(cell) {
                if owner == TREASURY {
                    let recipient = owners[rng.range(owners.len() as u64) as usize].1;
                    state.mine(cell, recipient, *current_height);
                }
            }
        }
    }
    true
}
