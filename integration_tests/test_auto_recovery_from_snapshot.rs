//! `test_auto_recovery_from_snapshot` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_auto_recovery_from_snapshot() {
    // Save and reload a five-block chain: the reload must reproduce the
    // identical tip state through the journal replay path.
    let _tmp = setup_test_dir("auto_recovery");
    reset_testnet();
    let mut chain = treasury_genesis();
    let miner = [1u8; 32];
    for _ in 0..5 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    slash::save(&chain);
    let loaded = slash::load();
    assert_eq!(loaded.tip_hash(), chain.tip_hash());
    assert_eq!(loaded.state.pages, chain.state.pages);
}
