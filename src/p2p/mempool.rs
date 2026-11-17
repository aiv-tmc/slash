//! Mempool policy: transaction identity, treasury-payload admission with
//! the consensus pairing order, block assembly from the mempool, cap
//! enforcement, and eviction of treasury payloads whose funding payment
//! has left the pool.
//!
//! Admission runs the same structural policy as block validation
//! (`Chain::validate_tx`) plus the treasury-pairing bookkeeping, so a
//! transaction admitted here is executable in the next block. Treasury
//! payloads are fully authenticated at admission: the treasury signer
//! signature is verified against the configured signer key before any
//! pairing logic runs, so a forged payload can never occupy the mempool
//! and poison the miner's template. The pairing mirror evaluates both
//! sides of the curve exactly as the block executor does: a mint prices
//! against the running curve position and pairs with one earlier vault
//! payment covering the price, and a payout matches one earlier
//! treasury-returning transfer at exactly the curve sell price of the
//! returned amount. A payload minting zero cells is rejected outright:
//! it has no payment to pair with and would kill every block it enters.
//! The cap is enforced by `evict_to_cap`, which every insertion path
//! calls before pushing, so the mempool never exceeds its bound between
//! housekeeping ticks and every downstream consumer that sized itself
//! against the cap is sized against reality.

use crate::chain::{Chain, Tx};
use crate::state::{FEE_VAULT, TREASURY, TREASURY_VAULT};
use std::collections::HashMap;

/// Build the miner's fee claims for a candidate block at `height`: sweep
/// every unlocked fee-vault cell in canonical order. An empty vault
/// yields no claims; locked cells are skipped by the selection, so the
/// assembled claims are always executable at the next height.
pub fn build_fee_claims(
    state: &crate::state::State,
    miner: [u8; 32],
    height: u64,
) -> Vec<crate::state::Output> {
    state
        .select_unlocked(FEE_VAULT, state.balance(FEE_VAULT), height)
        .into_iter()
        .map(|(s, e)| crate::state::Output {
            start: s,
            end: e,
            to: miner,
            lock: None,
        })
        .collect()
}

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

/// Enforce a mempool size bound: evict the lowest-value transactions
/// until the pool fits `cap` entries. The transactions with the smallest
/// fee-to-output ratio leave first, so a flood of cheap transactions
/// cannot displace paying traffic, and treasury payloads clean their
/// pairing-table entries on the way out so their funding payments become
/// pairable again. Every insertion path calls this before pushing, so
/// the bound holds at all times — not only at housekeeping tick
/// boundaries — and every downstream consumer that sized itself against
/// the cap (assembly, purge, eviction) is sized against the pool's real
/// reachable state.
pub(crate) fn evict_to_cap(
    mempool: &mut Vec<Tx>,
    pairs: &mut HashMap<Vec<u8>, (Vec<u8>, usize)>,
    cap: usize,
) {
    while mempool.len() > cap {
        let worst = mempool
            .iter()
            .enumerate()
            .min_by_key(|(_, tx)| {
                let out: u64 = tx.outputs.iter().map(|o| o.end - o.start).sum();
                let fee: u64 = tx
                    .outputs
                    .iter()
                    .filter(|o| o.to == FEE_VAULT)
                    .map(|o| o.end - o.start)
                    .sum();
                fee.saturating_mul(1_000_000).saturating_div(out.max(1))
            })
            .map(|(i, _)| i);
        if let Some(i) = worst {
            let tx = mempool.remove(i);
            if tx.from == TREASURY {
                evict_treasury_payload(&tx, pairs);
            }
        } else {
            break;
        }
    }
}

/// The transaction id: blake3 over the signature bytes concatenated with
/// the serialized transaction body. Unsigned transactions — the coinbase
/// and the treasury payloads — carry an empty signature, so an id derived
/// from the signature alone would collapse every unsigned transaction
/// onto a single value and silently drop distinct payloads during
/// assembly and purge; the body commitment keeps distinct unsigned
/// transactions distinct. The id is stable across serialization and cheap
/// relative to the serialization the hot path already performs.
pub fn tx_id(tx: &Tx) -> Vec<u8> {
    let body = bincode::serialize(tx).unwrap_or_default();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&tx.sig);
    hasher.update(&body);
    hasher.finalize().as_bytes().to_vec()
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

#[cfg(test)]
mod evict_tests {
    use super::*;

    /// One ordinary transaction of `width` output cells paying `fee`
    /// cells into the fee vault; the marker byte keeps identities
    /// distinguishable under eviction. The markers start at 0xA1 so no
    /// synthetic owner collides with the well-known addresses: marker
    /// 0x01 would equal FEE_VAULT and every output would be counted as
    /// fee, which corrupts the ratio the policy under test computes.
    fn ordinary(marker: u8, width: u64, fee: u64) -> Tx {
        let owner = [0xA0 | (marker & 0x0F); 32];
        Tx {
            from: owner,
            inputs: vec![(0, width)],
            outputs: vec![
                crate::chain::Output {
                    start: 0,
                    end: fee,
                    to: FEE_VAULT,
                    lock: None,
                },
                crate::chain::Output {
                    start: fee,
                    end: width,
                    to: owner,
                    lock: None,
                },
            ],
            sig: vec![marker],
            scheme: 0,
            memo: None,
        }
    }

    #[test]
    fn eviction_drops_the_lowest_fee_ratio_first() {
        let mut pool = vec![
            ordinary(1, 100, 1),
            ordinary(2, 100, 2),
            ordinary(3, 100, 3),
            ordinary(4, 100, 4),
        ];
        let mut pairs = HashMap::new();
        evict_to_cap(&mut pool, &mut pairs, 2);
        assert_eq!(pool.len(), 2, "cap 2 leaves exactly two entries");
        let remaining: Vec<u8> = pool.iter().map(|t| t.from[0]).collect();
        assert_eq!(
            remaining,
            vec![0xA3, 0xA4],
            "the two lowest fee-ratio transactions leave first, \
             the two highest survive"
        );
    }

    #[test]
    fn evicted_treasury_payload_cleans_its_pairing_entry() {
        let mut payload = ordinary(9, 100, 1);
        payload.from = TREASURY;
        let funding = ordinary(8, 100, 10);
        let mut pool = vec![funding.clone(), payload.clone()];
        let mut pairs = HashMap::new();
        pairs.insert(tx_id(&payload), (tx_id(&funding), 0));
        evict_to_cap(&mut pool, &mut pairs, 1);
        assert!(
            pairs.is_empty(),
            "the evicted payload releases its funding payment"
        );
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn pool_at_or_below_cap_is_untouched() {
        let mut pool = vec![ordinary(1, 100, 1), ordinary(2, 100, 2)];
        let mut pairs = HashMap::new();
        evict_to_cap(&mut pool, &mut pairs, 2);
        assert_eq!(pool.len(), 2);
        evict_to_cap(&mut pool, &mut pairs, 10);
        assert_eq!(pool.len(), 2);
    }
}
