//! Soak driver: runs the three-node simulation for `ops` operations,
//! injecting a divergence drill every `fork_len` operations, and checks
//! the conservation invariant on every node after every operation.

use super::*;

/// Divergence drill: every node rebuilds its chain as the shared prefix
/// cloned from node 0 extended by its own `fork_len`-block suffix. The
/// chains disagree on the suffix while the prefix — and therefore the
/// conservation invariant — stays identical, exercising the multi-chain
/// bookkeeping without a network transport.
fn reorg_drill(rng: &mut Rng, nodes: &mut [Node; 3], fork_len: u64) {
    let base = nodes[0].chain.clone();
    let plans: Vec<Vec<slash::chain::Tx>> = (0..3).map(|i| random_payload(rng, nodes, i)).collect();
    for (i, node) in nodes.iter_mut().enumerate() {
        node.chain = base.clone();
        node.mine(plans[i].clone());
        for _ in 1..fork_len {
            node.mine(vec![]);
        }
    }
}

/// Run the full multi-node soak: `ops` payload rounds mined round-robin
/// across the three nodes, a divergence drill after every `fork_len`
/// rounds, and the conservation invariant checked on all nodes after each
/// round.
pub fn run_network_soak(ops: u64, fork_len: u64) {
    let mut rng = Rng(0x5EED_5EED_5EED_5EED);
    let mut nodes = [
        Node::new("node_a"),
        Node::new("node_b"),
        Node::new("node_c"),
    ];
    for op in 0..ops {
        let miner = (op % 3) as usize;
        let txs = random_payload(&mut rng, &nodes, miner);
        nodes[miner].mine(txs);
        if fork_len > 0 && op % fork_len == fork_len - 1 {
            reorg_drill(&mut rng, &mut nodes, fork_len);
        }
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
    }
}
