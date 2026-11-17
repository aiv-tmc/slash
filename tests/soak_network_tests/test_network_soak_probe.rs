//! `test_network_soak_probe` — the soak localizer: the same machinery as
//! the CI soak with per-operation timing lines on stderr, so a stall
//! identifies its operation, the phase (payload, mine, drill), and the
//! state dimensions at the moment it happened. The probe runs a short
//! ladder: plain rounds first, then rounds with drills, so a drill-phase
//! stall separates from a mining-phase stall in one run. It is a normal
//! test: fast when healthy, precise when not.
//!
//! Every phase boundary is announced on stderr immediately (stderr is
//! unbuffered), and a watchdog aborts the process past 120 seconds of
//! phase silence, so a stall always leaves the exact phase line as the
//! last output. Set `SLASH_SOAK_TRACE=1` for sub-step traces inside
//! `Node::mine`.

use super::*;

/// The wall-clock budget of a single probe phase. Every phase must
/// print its boundary within this window; the watchdog aborts the
/// process with the last heartbeat otherwise.
const PER_OP_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(120);

#[test]
fn test_network_soak_probe() {
    let _tmp = setup_test_dir("network_soak_probe");
    reset_testnet();
    let watchdog = Watchdog::start(PER_OP_WATCHDOG);
    let mut rng = Rng(0x5EED_5EED_5EED_5EED);
    eprintln!("[probe] setup: building three genesis nodes");
    let mut nodes = [
        Node::new("node_a"),
        Node::new("node_b"),
        Node::new("node_c"),
    ];
    eprintln!("[probe] setup: nodes ready");
    // Phase 1: plain rounds, no drills. A stall here is in the mining
    // path itself (payload build, template, or recovery apply).
    for op in 0..10u64 {
        watchdog.beat(format!("probe op {op} phase=plain: mining"));
        let miner = (op % 3) as usize;
        eprintln!("[probe] op {op} phase=plain miner={miner}: payload");
        let t_payload = std::time::Instant::now();
        let txs = random_payload(&mut rng, &nodes, miner);
        let payload_ms = t_payload.elapsed();
        eprintln!(
            "[probe] op {op} phase=plain miner={miner}: mining {} txs (payload {:?})",
            txs.len(),
            payload_ms
        );
        let t = std::time::Instant::now();
        nodes[miner].mine(&txs);
        eprintln!("[probe] op {op} phase=plain miner={miner}: mined, checking invariants");
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
        eprintln!(
            "[probe] op {op} phase=plain miner={miner} took {:?} (payload {:?}) tips {:?} pages {:?}",
            t.elapsed(),
            payload_ms,
            nodes
                .iter()
                .map(|n| n.chain.tip_height())
                .collect::<Vec<_>>(),
            nodes
                .iter()
                .map(|n| n.chain.state.pages.len())
                .collect::<Vec<_>>()
        );
    }
    // Phase 2: rounds with a drill every round. A stall here is in the
    // drill path (clone, plan, or fork mining).
    for op in 0..10u64 {
        watchdog.beat(format!("probe op {op} phase=drill: mining"));
        let miner = (op % 3) as usize;
        eprintln!("[probe] op {op} phase=drill miner={miner}: payload");
        let t_payload = std::time::Instant::now();
        let txs = random_payload(&mut rng, &nodes, miner);
        let payload_ms = t_payload.elapsed();
        eprintln!(
            "[probe] op {op} phase=drill miner={miner}: mining {} txs (payload {:?})",
            txs.len(),
            payload_ms
        );
        let t = std::time::Instant::now();
        nodes[miner].mine(&txs);
        eprintln!("[probe] op {op} phase=drill miner={miner}: drilling");
        reorg_drill(&mut rng, &mut nodes, 3);
        eprintln!("[probe] op {op} phase=drill miner={miner}: drilled, checking invariants");
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
        eprintln!(
            "[probe] op {op} phase=drill miner={miner} took {:?} (payload {:?}) tips {:?} pages {:?}",
            t.elapsed(),
            payload_ms,
            nodes
                .iter()
                .map(|n| n.chain.tip_height())
                .collect::<Vec<_>>(),
            nodes
                .iter()
                .map(|n| n.chain.state.pages.len())
                .collect::<Vec<_>>()
        );
    }
}
