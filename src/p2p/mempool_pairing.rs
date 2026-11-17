//! Treasury-payload admission and block assembly for the mempool.
//!
//! The pairing mirror evaluates both sides of the bonding curve exactly
//! as the block executor does: a mint prices against the running curve
//! position and pairs with one earlier vault payment covering the price,
//! and a payout matches one earlier treasury-returning transfer at
//! exactly the curve sell price of the returned amount. Every payload is
//! authenticated against the configured treasury signer key before any
//! pairing logic runs, and its id is stored in the pairing table keyed by
//! the funding payment's id, so no payment ever funds two payloads.

use super::tx_id;
use crate::chain::{Chain, Tx};
use crate::state::{TREASURY, TREASURY_VAULT};
use std::collections::HashMap;

/// Admit a treasury payload (curve mint or vault payout) to the mempool,
/// maintaining the pairing table exactly as consensus does inside a
/// block: sells recorded by earlier ordinary transactions can fund a
/// payout, and a payment recorded by an earlier ordinary transaction can
/// fund a mint. The payload is rejected when no compatible earlier
/// transaction exists in the pool. `running_total` tracks the curve
/// position the block executor will see: it rises by every admitted
/// minted amount and falls by every admitted sold amount, so successive
/// payloads price against the same running position consensus evaluates.
/// It is left untouched on rejection.
///
/// The payload's transaction id is stored in the pairing table keyed by
/// the funding payment's transaction id, so no payment ever funds two
/// payloads.
pub(crate) fn admit_treasury_payload(
    chain: &Chain,
    tx: &Tx,
    mempool: &[Tx],
    pairs: &mut HashMap<Vec<u8>, (Vec<u8>, usize)>,
    running_total: &mut u64,
) -> bool {
    // Authenticate the payload before any pairing logic: the treasury
    // signer key must be configured and the payload's signature must
    // verify against it, exactly as the consensus engine verifies the
    // same hash over the same fields. An unauthenticated payload would
    // be consensus-rejected inside the block it enters, killing the
    // miner's template and every honest transaction in it, so admission
    // refuses it here and the caller records a strike against the
    // sender.
    if chain.state.treasury_signer == [0u8; 32] {
        return false;
    }
    let engine_outputs: Vec<crate::state::Output> = tx
        .outputs
        .iter()
        .map(|o| crate::state::Output {
            start: o.start,
            end: o.end,
            to: o.to,
            lock: o.lock,
        })
        .collect();
    let hash = Chain::tx_signature_hash(
        &tx.from,
        &tx.inputs,
        &engine_outputs,
        chain.chain_id.as_bytes(),
        tx.scheme,
        &tx.memo,
    );
    if !crate::crypto::verify(&chain.state.treasury_signer, &hash, &tx.sig) {
        return false;
    }
    let height = chain.tip_height() + 1;
    // Classify the payload by its funding source: treasury-owned inputs
    // make it a curve mint; vault-owned inputs make it a sell payout.
    let mut treasury_owned = true;
    let mut vault_owned = true;
    for &(s, e) in &tx.inputs {
        if !chain.state.owns(TREASURY, s, e, height) {
            treasury_owned = false;
        }
        if !chain.state.owns(TREASURY_VAULT, s, e, height) {
            vault_owned = false;
        }
    }
    if treasury_owned == vault_owned {
        return false;
    }
    if vault_owned {
        // Sell side. The payout amount must equal the curve sell price
        // of exactly one earlier ordinary transaction's payment back to
        // the treasury; a payment already consumed by an earlier payload
        // is skipped. Matching mirrors the block executor: the price is
        // a function of the returned amount, never of the payout.
        let Some(payout) = width_sum(&tx.outputs) else {
            return false;
        };
        for (j, earlier) in mempool.iter().enumerate() {
            if earlier.from == TREASURY || pairs.values().any(|(t, _)| *t == tx_id(earlier)) {
                continue;
            }
            let Some(sold) = returned_to_treasury(earlier) else {
                continue;
            };
            if sold > 0 && crate::treasury::sell_price(sold, *running_total) == payout {
                pairs.insert(tx_id(tx), (tx_id(earlier), j));
                *running_total = running_total.saturating_sub(sold);
                return true;
            }
        }
        false
    } else {
        // Treasury-owned inputs. A sell payout whose funding sell is
        // still pending spends treasury-owned cells in the current state
        // as well: the sell has not executed, so its returned cells are
        // not in the vault and the payout cannot read vault funds. Sell
        // pairing is therefore attempted first — an earlier ordinary
        // transaction paying exactly sell_price back to the treasury; a
        // payload with no matching sell falls through to ordinary buy
        // pairing against a treasury-vault payment.
        let Some(payout) = width_sum(&tx.outputs) else {
            return false;
        };
        for (j, earlier) in mempool.iter().enumerate() {
            if earlier.from == TREASURY || pairs.values().any(|(t, _)| *t == tx_id(earlier)) {
                continue;
            }
            let Some(sold) = returned_to_treasury(earlier) else {
                continue;
            };
            if sold > 0 && crate::treasury::sell_price(sold, *running_total) == payout {
                pairs.insert(tx_id(tx), (tx_id(earlier), j));
                *running_total = running_total.saturating_sub(sold);
                return true;
            }
        }
        // Buy side: the minted amount prices against the running curve
        // position and must be funded by an earlier ordinary transaction
        // paying at least the price into the treasury vault. A payload
        // minting zero cells carries no payment at all and cannot
        // satisfy the exact-packing rule inside a block, so it is
        // rejected outright instead of being admitted as a guaranteed
        // template-killer.
        let minted = payout;
        if minted == 0 {
            return false;
        }
        let price = crate::treasury::buy_price(minted, *running_total);
        for (j, earlier) in mempool.iter().enumerate() {
            if earlier.from == TREASURY || pairs.values().any(|(t, _)| *t == tx_id(earlier)) {
                continue;
            }
            let Some(paid) = paid_to_vault(earlier) else {
                continue;
            };
            if paid >= price {
                pairs.insert(tx_id(tx), (tx_id(earlier), j));
                *running_total = running_total.saturating_add(minted);
                return true;
            }
        }
        false
    }
}

/// Assemble the block transaction list from the mempool: ordinary
/// transactions first in admission order, treasury payloads after them.
/// The pairing table orders each payload right after its funding payment,
/// so the assembled block executes in exactly the order the consensus
/// pairing rules require.
pub(crate) fn assemble_block_txs(
    chain: &Option<&Chain>,
    mempool: &[Tx],
    _height: u64,
    pairs: &HashMap<Vec<u8>, (Vec<u8>, usize)>,
) -> Vec<Tx> {
    let mut out = Vec::with_capacity(mempool.len());
    if let Some(chain) = chain {
        let _ = chain.tip_height();
    }
    // Map each payload id to its funding payment's mempool index, ordered
    // by that index so pairs land in payment order.
    let mut by_payment: Vec<(usize, Vec<u8>)> = pairs
        .iter()
        .map(|(payload, (_, idx))| (*idx, payload.clone()))
        .collect();
    by_payment.sort_by_key(|(idx, _)| *idx);
    let mut inserted: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    for (idx, payload_id) in &by_payment {
        // Insert the funding payment if it is not already out.
        if let Some(payment) = mempool.get(*idx) {
            if !inserted.contains(&tx_id(payment)) {
                out.push(payment.clone());
                inserted.insert(tx_id(payment));
            }
        }
        // Then the payload itself.
        if let Some(payload) = mempool.iter().find(|t| tx_id(t) == *payload_id) {
            if !inserted.contains(payload_id) {
                out.push(payload.clone());
                inserted.insert(payload_id.clone());
            }
        }
    }
    for tx in mempool {
        if !inserted.contains(&tx_id(tx)) {
            out.push(tx.clone());
        }
    }
    out
}

/// Drop a treasury payload from the pairing table when it leaves the
/// mempool (eviction or inclusion in a block), so its funding payment or
/// sell becomes pairable again.
pub(crate) fn evict_treasury_payload(tx: &Tx, pairs: &mut HashMap<Vec<u8>, (Vec<u8>, usize)>) {
    pairs.remove(&tx_id(tx));
}

/// Sum the widths of an output list with checked arithmetic. A range
/// whose end does not exceed its start, or a sum that overflows u64,
/// fails closed as None and the caller rejects the transaction.
fn width_sum(outputs: &[crate::chain::Output]) -> Option<u64> {
    outputs.iter().try_fold(0u64, |acc, o| {
        let width = o.end.checked_sub(o.start)?;
        acc.checked_add(width)
    })
}

/// The total payment one ordinary transaction sends into the treasury
/// vault. Treasury senders contribute nothing: only ordinary transactions
/// can fund a mint.
fn paid_to_vault(tx: &Tx) -> Option<u64> {
    if tx.from == TREASURY {
        return Some(0);
    }
    tx.outputs
        .iter()
        .filter(|o| o.to == TREASURY_VAULT)
        .try_fold(0u64, |acc, o| {
            let width = o.end.checked_sub(o.start)?;
            acc.checked_add(width)
        })
}

/// The total payment one ordinary transaction sends back to the treasury
/// address. Treasury senders contribute nothing: only ordinary
/// transactions can be sells.
fn returned_to_treasury(tx: &Tx) -> Option<u64> {
    if tx.from == TREASURY {
        return Some(0);
    }
    tx.outputs
        .iter()
        .filter(|o| o.to == TREASURY)
        .try_fold(0u64, |acc, o| {
            let width = o.end.checked_sub(o.start)?;
            acc.checked_add(width)
        })
}
