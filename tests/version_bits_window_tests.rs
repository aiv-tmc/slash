//! Regression tests for windowed version-bit signaling (BIP-9 style).
//!
//! Signaling is counted per retarget window: the counter resets when a block
//! opens a new window and lock-in is decided only at the last block of a
//! window whose tally reached the threshold. The tests drive the public
//! `Chain::update_version_bits` entry point with synthetic blocks whose
//! header fields carry the signaling bit, which exercises the state machine
//! without proof-of-work or state transitions.

use slash::chain::{Chain, DeploymentStatus, DIFFICULTY_ADJUSTMENT_INTERVAL};
use slash::state::TREASURY;
use slash::test_fixtures::*;
use std::collections::BTreeMap;

/// Build a synthetic block whose only meaningful fields for the state machine
/// are `height` and `version`. Every other field takes a neutral value: the
/// block never executes, so only the header matters.
fn synthetic_block(height: u64, version: u32) -> slash::chain::Block {
    slash::chain::Block {
        version,
        prev: [0u8; 32],
        time: 1_700_000_000 + height,
        height,
        nonce: 0,
        miner: TREASURY,
        mined_cell: 0,
        txs: Vec::new(),
        fee_claims: Vec::new(),
        difficulty: 1,
        page_roots: BTreeMap::new(),
    }
}

/// Configure the first deployment (`min_tx_v2`, bit 0) for direct state
/// machine testing: signaling starts at the first window boundary and the
/// timeout sits far beyond every tested height. Returns a chain whose only
/// relevant state is the deployment vector.
fn signaling_chain(start_height: u64) -> Chain {
    reset_testnet();
    let mut chain = Chain::genesis();
    let d = &mut chain.version_bits.deployments[0];
    d.start_height = start_height;
    d.timeout_height = 10_000_000;
    d.threshold = 1_512;
    d.status = DeploymentStatus::Defined;
    d.signals = 0;
    d.activation_height = None;
    chain
}

/// Feed one synthetic block per height in `heights`, carrying the signaling
/// bit when `signal` is true, through the version-bit state machine.
fn run_window(chain: &mut Chain, heights: std::ops::Range<u64>, signal: bool) {
    for h in heights {
        let version = if signal { 1 } else { 0 };
        let b = synthetic_block(h, version);
        chain.update_version_bits(&b);
    }
}

#[test]
fn built_in_start_heights_align_with_retarget_windows() {
    // Every built-in deployment starts exactly on a retarget window
    // boundary, so the first signaling window of each deployment is
    // complete from its first block onward and the boundary reset can
    // never discard a partial window's honest signals.
    let chain = signaling_chain(DIFFICULTY_ADJUSTMENT_INTERVAL);
    for d in &chain.version_bits.deployments {
        assert_eq!(
            d.start_height % DIFFICULTY_ADJUSTMENT_INTERVAL,
            0,
            "deployment {} must start on a retarget window boundary",
            d.name
        );
    }
}

#[test]
fn signals_below_threshold_across_windows_never_lock_in() {
    let mut chain = signaling_chain(DIFFICULTY_ADJUSTMENT_INTERVAL);
    // Three consecutive windows each signal 800 blocks: 2400 cumulative
    // signals exceed the 1512 threshold, but no single window reaches it, so
    // the deployment must stay Started with only the last window's tally.
    let w = DIFFICULTY_ADJUSTMENT_INTERVAL;
    for window in 0..3u64 {
        let start = w + window * w;
        run_window(&mut chain, start..start + 800, true);
        run_window(&mut chain, start + 800..start + w, false);
    }
    let d = &chain.version_bits.deployments[0];
    assert_eq!(d.status, DeploymentStatus::Started);
    assert_eq!(d.signals, 800);
    assert!(d.activation_height.is_none());
}

#[test]
fn single_window_reaching_threshold_locks_in_at_window_end() {
    let mut chain = signaling_chain(DIFFICULTY_ADJUSTMENT_INTERVAL);
    let w = DIFFICULTY_ADJUSTMENT_INTERVAL;
    let window_start = w;
    // Exactly `threshold` signaling blocks, all inside one window.
    run_window(&mut chain, window_start..window_start + 1_512, true);
    // Lock-in must not happen early: the block that completes the threshold
    // sits mid-window and the status must still be Started.
    let d = &chain.version_bits.deployments[0];
    assert_eq!(d.status, DeploymentStatus::Started);
    assert_eq!(d.signals, 1_512);
    // Non-signaling blocks carry the window to its end, where lock-in fires.
    run_window(&mut chain, window_start + 1_512..window_start + w, false);
    let d = &chain.version_bits.deployments[0];
    assert_eq!(d.status, DeploymentStatus::LockedIn);
    assert_eq!(
        d.activation_height,
        Some(window_start + w - 1 + DIFFICULTY_ADJUSTMENT_INTERVAL)
    );
}

#[test]
fn locked_in_deployment_activates_at_scheduled_height() {
    let mut chain = signaling_chain(DIFFICULTY_ADJUSTMENT_INTERVAL);
    let w = DIFFICULTY_ADJUSTMENT_INTERVAL;
    run_window(&mut chain, w..w + 1_512, true);
    run_window(&mut chain, w + 1_512..w + w, false);
    let activation = chain.version_bits.deployments[0]
        .activation_height
        .expect("lock-in scheduled an activation height");
    // One block before the scheduled height stays LockedIn.
    let just_before = synthetic_block(activation - 1, 0);
    chain.update_version_bits(&just_before);
    assert_eq!(
        chain.version_bits.deployments[0].status,
        DeploymentStatus::LockedIn
    );
    // The block at the scheduled height flips the deployment to Active.
    let at_activation = synthetic_block(activation, 0);
    chain.update_version_bits(&at_activation);
    assert_eq!(
        chain.version_bits.deployments[0].status,
        DeploymentStatus::Active
    );
}

#[test]
fn counter_resets_at_window_boundary_even_without_threshold() {
    let mut chain = signaling_chain(DIFFICULTY_ADJUSTMENT_INTERVAL);
    let w = DIFFICULTY_ADJUSTMENT_INTERVAL;
    // Fill the first counted window almost to the threshold, then cross the
    // boundary: the fresh window must start from zero, proving the reset
    // happens regardless of lock-in.
    run_window(&mut chain, w..w + 1_500, true);
    assert_eq!(chain.version_bits.deployments[0].signals, 1_500);
    let first_of_next = synthetic_block(w + w, 1);
    chain.update_version_bits(&first_of_next);
    let d = &chain.version_bits.deployments[0];
    assert_eq!(d.signals, 1);
    assert_eq!(d.status, DeploymentStatus::Started);
}
