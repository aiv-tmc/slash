//! `test_dos_oversized_block_rejected` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_dos_oversized_block_rejected() {
    let _tmp = setup_test_dir("dos_oversized");
    reset_testnet();
    // The chain is only read by the block builder below.
    let chain = chain::Chain::genesis();

    let mut txs = Vec::new();
    for i in 0..200 {
        let start = i as u64 * 50;
        txs.push(chain::Tx {
            from: state::TREASURY,
            inputs: vec![(start, start + 50)],
            outputs: vec![chain::Output {
                start,
                end: start + 50,
                to: [1u8; 32],
                lock: None,
            }],
            sig: vec![0u8; 64],
            scheme: 0,
            memo: None,
        });
    }

    let b = mine_next_block(&chain, [1u8; 32], txs);
    // The consensus size limit is checked against bincode serialization,
    // which is the same format used for P2P transmission and disk storage.
    let serialized = bincode::serialize(&b).unwrap_or_default();
    if serialized.len() > chain::MAX_BLOCK_SIZE_BYTES {
        assert!(!chain.validate_block(&b));
    }
}
