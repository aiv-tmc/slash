//! `test_restart_persists_governance_and_history` — one test case of the
//! `reorg_persistence_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_restart_persists_governance_and_history() {
    // Deployments and checkpoints must survive a save/load cycle, and the
    // reorganization window must be rebuilt exactly from genesis replay.
    let _tmp = setup_test_dir("restart_persist");
    reset_testnet();
    let (mut chain, _miner) = chain_with_blocks(5);
    // The probe name must not collide with any built-in deployment: the
    // table already carries a "scheme1_ready" record, and a name lookup
    // would otherwise return the built-in instead of the pushed one.
    chain.version_bits.deployments.push(Deployment {
        name: "reorg_probe".to_string(),
        bit: 6,
        start_height: 1,
        timeout_height: 1000,
        threshold: 900,
        status: DeploymentStatus::Started,
        activation_height: None,
        signals: 3,
    });
    // The checkpoint pins the retained block at its absolute height: the
    // block vector holds heights 1..=5, so height 3 lives at index 2.
    let cp_hash = chain.block_hash(&chain.blocks[2]);
    chain.checkpoints.insert(3, cp_hash);

    slash::save(&chain);
    let loaded = slash::load_pruned();

    assert_eq!(loaded.checkpoints.get(&3), Some(&cp_hash));
    // Genesis carries the six built-in deployments; the pushed record is
    // the seventh and must round-trip intact.
    assert_eq!(loaded.version_bits.deployments.len(), 7);
    let d = loaded
        .version_bits
        .deployments
        .iter()
        .find(|d| d.name == "reorg_probe")
        .expect("the pushed deployment survives the restart");
    assert_eq!(d.status, DeploymentStatus::Started);
    assert_eq!(d.signals, 3);
    // Genesis replay reconstructs every height of the short chain.
    assert_eq!(loaded.state_history.len(), 6);
    assert!(loaded.state_history.contains_key(&4));
    // Tip state and tip hash reproduce the persisted chain exactly.
    assert_eq!(loaded.state.pages, chain.state.pages);
    assert_eq!(loaded.tip_hash(), chain.tip_hash());
}
