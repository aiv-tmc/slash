//! `test_reorg_state_consistency` — one test case of the `integration_tests` suite.
//!
//! The shared fixtures (`setup_test_dir`, `reset_testnet`,
//! `treasury_genesis`, `mine_next_block`) come from the crate root via
//! `use super::*;`.

use super::*;

#[test]
fn test_reorg_state_consistency() {
    // After a save/load roundtrip the history window is rebuilt by genesis
    // replay, so a previously recorded height is restorable. The loaded
    // chain is only inspected below, so the binding is immutable.
    let _tmp = setup_test_dir("reorg_consistency");
    reset_testnet();
    let mut chain = treasury_genesis();
    let miner = [1u8; 32];
    for _ in 0..5 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    slash::save(&chain);
    let loaded = slash::load();
    assert_eq!(loaded.state_history.len(), 6);
    let restored = loaded.state_history.get(&4).cloned().unwrap();
    assert_eq!(restored.pages, chain.state_history[&4].pages);
}
