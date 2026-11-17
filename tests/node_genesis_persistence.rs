//! Regression test for the genesis persistence path used by the node
//! binary: a freshly created genesis chain must round-trip through the
//! manifest save and the pruned load without tripping the sanity
//! package. The balance cache is derived data, but the load-time sanity
//! check requires its sum to equal the full supply, so an empty cache
//! saved at genesis made every node restart fall back to snapshot
//! recovery.

use slash::storage;
use slash::test_fixtures::setup_test_dir;
use slash::TESTNET_MODE;
use std::sync::atomic::Ordering;

#[test]
fn genesis_manifest_round_trips_the_balance_cache() {
    let _tmp = setup_test_dir("node_genesis_persist");
    TESTNET_MODE.store(true, Ordering::Relaxed);

    // Node startup on an empty data directory: genesis chain.
    let chain = slash::load();
    assert_eq!(chain.tip_height(), 0);
    let sum: u64 = chain.state.balance_cache.values().sum();
    eprintln!("genesis cache sum: {sum}");
    assert_eq!(sum, slash::state::N);

    // Housekeeping save, exactly like the node event loop performs it.
    storage::save_manifest(
        &chain.blocks,
        &chain.state,
        chain.base_height,
        &chain.checkpoints,
        &chain.version_bits.deployments,
        chain.chain_id.as_bytes(),
    )
    .unwrap();

    // Restart: the manifest triple must pass the sanity package and
    // restore the identical cache.
    let reloaded = slash::load();
    assert_eq!(reloaded.tip_height(), 0);
    let reloaded_sum: u64 = reloaded.state.balance_cache.values().sum();
    eprintln!("reloaded cache sum: {reloaded_sum}");
    assert_eq!(reloaded_sum, slash::state::N);
}
