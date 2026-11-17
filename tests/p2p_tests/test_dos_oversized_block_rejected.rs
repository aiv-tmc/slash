//! `test_dos_oversized_block_rejected` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_dos_oversized_block_rejected() {
    let _tmp = setup_test_dir("dos_oversized");
    reset_testnet();
    let chain = chain::Chain::genesis();

    // A block past the serialized size limit is refused before any
    // transaction work: the limit is checked against the bincode
    // serialization, the same format used for P2P transmission and disk
    // storage. The block is a valid empty template with one absurdly wide
    // transaction injected after mining — the size gate runs before the
    // structural gates, so the wide transaction never needs to be valid.
    let mut b = mine_next_block(&chain, [1u8; 32], vec![]);
    let wide: Vec<(u64, u64)> = (0..80_000u64).map(|i| (i, i + 1)).collect();
    b.txs.push(chain::Tx {
        from: [9u8; 32],
        inputs: wide.clone(),
        outputs: wide
            .iter()
            .map(|&(s, e)| chain::Output {
                start: s,
                end: e,
                to: [1u8; 32],
                lock: None,
            })
            .collect(),
        sig: vec![1u8; 64],
        scheme: 0,
        memo: None,
    });
    let serialized = bincode::serialize(&b).unwrap_or_default();
    assert!(
        serialized.len() > chain::MAX_BLOCK_SIZE_BYTES,
        "the injected transaction must push the block past the size limit"
    );
    assert!(
        !chain.validate_block(&b),
        "an oversized block must be rejected"
    );
}
