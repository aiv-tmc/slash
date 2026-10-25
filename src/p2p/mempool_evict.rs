//! Mempool cap enforcement.
//!
//! The cap holds at all times — not only at housekeeping tick
//! boundaries — because every insertion path calls `evict_to_cap` before
//! pushing. The lowest fee-to-output-ratio transactions leave first, so
//! a flood of cheap transactions cannot displace paying traffic, and
//! treasury payloads clean their pairing-table entries on the way out so
//! their funding payments become pairable again.

use super::pairing::evict_treasury_payload;
use super::tx_id;
use crate::chain::Tx;
use crate::state::{FEE_VAULT, TREASURY};
use std::collections::HashMap;

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
