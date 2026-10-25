//! The unlocked sweep, the page-surgery primitive, interior locks inside
//! merged ranges, and the fee-claim validation: selection mechanics that
//! are not part of the golden vectors but pin the surrounding
//! invariants.

use super::*;

#[test]
fn select_unlocked_skips_locked_spans() {
    // The unlocked sweep splits owner ranges around locked runs and
    // keeps going, so a griefing lock can delay only its own cells and
    // never invalidates a whole claim sweep. Expired locks (until <=
    // height) no longer block the sweep.
    let mut state = State::genesis();
    {
        let page = state.get_page_mut(0);
        page.ranges.insert(
            0,
            R {
                e: 200,
                o: FEE_VAULT,
            },
        );
        page.ranges.insert(
            200,
            R {
                e: page_size(0),
                o: TREASURY,
            },
        );
    }
    // Lock two runs inside the vault range: [50, 60) and [150, 160).
    for cell in 50..60 {
        state.locks.insert(cell, 100);
    }
    for cell in 150..160 {
        state.locks.insert(cell, 100);
    }
    // At height 50 the locked runs are skipped and the sweep continues
    // past them in canonical order.
    let ranges = state.select_unlocked(FEE_VAULT, 200, 50);
    assert_eq!(ranges, vec![(0, 50), (60, 150), (160, 200)]);
    // A partial amount truncates at the first locked span.
    let ranges = state.select_unlocked(FEE_VAULT, 55, 50);
    assert_eq!(ranges, vec![(0, 50), (60, 65)]);
    // At the expiry height the cells are spendable again.
    let ranges = state.select_unlocked(FEE_VAULT, 200, 100);
    assert_eq!(ranges, vec![(0, 200)]);
}

#[test]
fn remove_range_spans_adjacent_same_owner_ranges() {
    // Regression guard: adjacent ranges of the same owner occur around
    // proof-of-work slot cells and must be crossed cleanly by a removal.
    // Removing a span that covers the head of the second range must drop
    // the second range's original key; leaving it in place would create
    // overlapping ranges and break the page invariant.
    let mut page = Page {
        ranges: BTreeMap::new(),
        ..Default::default()
    };
    page.ranges.insert(0, R { e: 10, o: TREASURY });
    page.ranges.insert(10, R { e: 20, o: TREASURY });
    page.ranges.insert(
        20,
        R {
            e: 30,
            o: [7u8; 32],
        },
    );

    // Remove [3, 12): crosses from the first treasury range into the
    // second, covering the head of the second range.
    assert!(page.remove_range(3, 12));
    let expected: BTreeMap<u64, R> = [
        (0, R { e: 3, o: TREASURY }),
        (12, R { e: 20, o: TREASURY }),
        (
            20,
            R {
                e: 30,
                o: [7u8; 32],
            },
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(page.ranges, expected);

    // Full removal across both treasury ranges plus the foreign range's
    // head is refused (foreign ownership), leaving the map untouched.
    assert!(!page.remove_range(0, 25));
    assert_eq!(page.ranges, expected);

    // Exact removal of the remaining second treasury range.
    assert!(page.remove_range(12, 20));
    let expected2: BTreeMap<u64, R> = [
        (0, R { e: 3, o: TREASURY }),
        (
            20,
            R {
                e: 30,
                o: [7u8; 32],
            },
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(page.ranges, expected2);
}

#[test]
fn lock_inside_merged_owner_range_blocks_spend() {
    // A lock registered in the interior of a merged owner range must
    // still block a spend that covers it. The ownership map compacts
    // the ten single-cell assignments into one range; the lock check
    // scans the full queried span, so the interior lock is visible.
    let mut state = State::genesis();
    let owner = [7u8; 32];
    for cell in 0..10 {
        state.mine(cell, owner, 0);
    }
    state.locks.insert(5, 100);
    assert!(!state.owns(owner, 0, 10, 50));
    assert!(state.owns(owner, 0, 10, 100));
    assert!(state.owns(owner, 6, 10, 50));
}

#[test]
fn empty_fee_claims_are_valid_and_claims_pay_miner_only() {
    // An empty claim set is a valid no-op: unclaimed fees stay in the
    // vault for a later sweep, and the block carrying it is not
    // penalized.
    let mut state = State::genesis();
    assert!(state.claim_fees([1u8; 32], &[], 10));

    // Claims paying anyone other than the block miner are refused. The
    // miner address [1u8; 32] below is the fee vault itself, so the
    // accepted claim exercises the vault-to-vault round trip: the sweep
    // validates ownership and lock state at the executing height and
    // then deliberately moves nothing. Debiting the balance cache
    // without a matching credit, or removing and re-inserting the same
    // range, would only push the cached balance away from the page map
    // it mirrors, so both the page map and the cache stay untouched.
    for cell in 0..100 {
        state.mine(cell, FEE_VAULT, 0);
    }
    let wrong = Output {
        start: 0,
        end: 10,
        to: [2u8; 32],
        lock: None,
    };
    assert!(!state.claim_fees([1u8; 32], &[wrong], 10));
    let right = Output {
        start: 0,
        end: 10,
        to: [1u8; 32],
        lock: None,
    };
    assert!(state.claim_fees([1u8; 32], &[right], 10));
    assert_eq!(state.balance(FEE_VAULT), 100);
    // The conservation invariant holds after the no-op round trip: the
    // cache mirrors the ledger exactly, with no drift in either
    // direction.
    let supply: u64 = state.balance_cache.values().sum();
    assert_eq!(supply, N);
}
