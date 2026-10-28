//! `test_network_soak_probe` — the soak localizer: the same machinery as
//! the CI soak with per-operation timing lines on stderr, so a stall
//! identifies its operation, the phase (payload, mine, drill), and the
//! state dimensions at the moment it happened. The probe runs a short
//! ladder: plain rounds first, then rounds with drills, so a drill-phase
//! stall separates from a mining-phase stall in one run. It is a normal
//! test: fast when healthy, precise when not.

use super::*;

#[test]
fn test_network_soak_probe() {
    let _tmp = setup_test_dir("network_soak_probe");
    reset_testnet();
    let mut rng = Rng(0x5EED_5EED_5EED_5EED);
    let mut nodes = [
        Node::new("node_a"),
        Node::new("node_b"),
        Node::new("node_c"),
    ];
    // Phase 1: plain rounds, no drills. A stall here is in the mining
    // path itself (payload build, template, or recovery apply).
    for op in 0..10u64 {
        let t = std::time::Instant::now();
        let miner = (op % 3) as usize;
        let txs = random_payload(&mut rng, &nodes, miner);
        nodes[miner].mine(&txs);
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
        eprintln!(
            "[probe] op {op} phase=plain miner={miner} took {:?} tips {:?} pages {:?}",
            t.elapsed(),
            nodes.iter().map(|n| n.chain.tip_height()).collect::<Vec<_>>(),
            nodes.iter().map(|n| n.chain.state.pages.len()).collect::<Vec<_>>()
        );
    }
    // Phase 2: rounds with a drill every three. A stall here is in the
    // drill path (clone, plan, or fork mining).
    for op in 0..10u64 {
        let t = std::time::Instant::now();
        let miner = (op % 3) as usize;
        let txs = random_payload(&mut rng, &nodes, miner);
        nodes[miner].mine(&txs);
        reorg_drill(&mut rng, &mut nodes, 3);
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
        eprintln!(
            "[probe] op {op} phase=drill miner={miner} took {:?} tips {:?} pages {:?}",
            t.elapsed(),
            nodes.iter().map(|n| n.chain.tip_height()).collect::<Vec<_>>(),
            nodes.iter().map(|n| n.chain.state.pages.len()).collect::<Vec<_>>()
        );
    }
}
