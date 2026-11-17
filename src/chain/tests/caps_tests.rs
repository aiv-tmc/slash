//! Amplification-cap tests: the per-transaction range, page, and field
//! caps reject shape attacks before any state work.

use crate::chain::*;

#[test]
fn tx_caps_reject_range_and_page_amplification() {
    // The amplification caps bound the shape of a transaction before
    // any state work: too many ranges in total, or ranges fanned out
    // across too many pages, are rejected structurally. Both cases are
    // built from one-cell ranges, the cheapest possible unit, so the
    // caps — not the amount — are what stops them.
    let chain = Chain::genesis();
    // Exactly MAX_RANGES_PER_TX ranges in total, split half inputs and
    // half outputs, all on one page: passes the caps.
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for i in 0..(MAX_RANGES_PER_TX / 2) as u64 {
        let start = i * 2;
        inputs.push((start, start + 1));
        outputs.push(Output {
            start,
            end: start + 1,
            to: [1u8; 32],
            lock: None,
        });
    }
    let at_cap = Tx {
        from: [1u8; 32],
        inputs,
        outputs,
        sig: vec![0u8; 64],
        scheme: 0,
        memo: None,
    };
    assert!(chain.tx_caps_valid(&at_cap));
    // One more range pair over the cap fails even though every range is
    // tiny.
    let last_end = at_cap.inputs.last().map(|(_, e)| *e).unwrap();
    let mut over = at_cap.clone();
    over.inputs.push((last_end, last_end + 1));
    over.outputs.push(Output {
        start: last_end,
        end: last_end + 1,
        to: [1u8; 32],
        lock: None,
    });
    assert!(!chain.tx_caps_valid(&over));
    // Ranges spread across MAX_PAGES_PER_TX + 1 pages fail the page cap
    // even though the range count stays small.
    let mut wide_inputs = Vec::new();
    let mut wide_outputs = Vec::new();
    for i in 0..=MAX_PAGES_PER_TX as u64 {
        // PAGE_SIZE is a u64 constant, so each page base is a plain
        // multiplication in global cell coordinates.
        let start = i * crate::state::PAGE_SIZE;
        wide_inputs.push((start, start + 1));
        wide_outputs.push(Output {
            start,
            end: start + 1,
            to: [1u8; 32],
            lock: None,
        });
    }
    let too_wide = Tx {
        from: [1u8; 32],
        inputs: wide_inputs,
        outputs: wide_outputs,
        sig: vec![0u8; 64],
        scheme: 0,
        memo: None,
    };
    assert!(!chain.tx_caps_valid(&too_wide));
}

#[test]
fn tx_field_caps_reject_oversized_sig_and_memo() {
    // Field caps refuse oversized variable-length fields before any
    // hashing or verification work: a signature longer than 64 bytes or
    // a memo longer than 4096 bytes is malformed on the wire whatever
    // its content, and the enclosing block size limit would otherwise
    // let a near-megabyte field reach every node's hash and parse paths
    // inside an otherwise valid block. The caps run inside the shared
    // structural policy, so mempool admission and block validation
    // refuse identically. The baseline transaction satisfies every other
    // structural rule — conservation, packing, the minimum, and the
    // mandatory fee — so only the field under test can fail it.
    let chain = Chain::genesis();
    let valid = Tx {
        from: [1u8; 32],
        inputs: vec![(0, 100)],
        outputs: vec![
            Output {
                start: 0,
                end: 95,
                to: [2u8; 32],
                lock: None,
            },
            Output {
                start: 95,
                end: 100,
                to: crate::state::FEE_VAULT,
                lock: None,
            },
        ],
        sig: vec![0u8; 64],
        scheme: 0,
        memo: None,
    };
    assert!(
        chain.tx_structure_valid(&valid),
        "baseline transaction satisfies every structural rule"
    );
    // One byte over the signature cap fails the structure even though
    // the signature would fail verification anyway: the cap refuses the
    // field before any parse attempt.
    let mut big_sig = valid.clone();
    big_sig.sig = vec![0u8; MAX_SIG_BYTES + 1];
    assert!(!chain.tx_structure_valid(&big_sig));
    // One byte over the memo cap fails the structure even though the
    // memo would fail DaoOp decoding anyway.
    let mut big_memo = valid.clone();
    big_memo.memo = Some(vec![0u8; MAX_MEMO_BYTES + 1]);
    assert!(!chain.tx_structure_valid(&big_memo));
    // Boundary values still pass: exactly 64 bytes and exactly 4096
    // bytes are within the caps.
    let mut at_sig_cap = valid.clone();
    at_sig_cap.memo = Some(vec![0u8; MAX_MEMO_BYTES]);
    assert!(chain.tx_structure_valid(&at_sig_cap));
}
