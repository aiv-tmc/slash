//! Soak driver: runs the three-node simulation for `ops` operations,
//! injecting a divergence drill every `fork_len` operations, and checks
//! the conservation invariant on every node after every operation.
//!
//! A soak test must fail, never hang: the watchdog budgets the whole
//! run and panics with the live state dimensions (tip, dirty pages,
//! locks, side-fork pool) the moment the budget is exceeded, so a
//! regression surfaces as one localized, information-rich failure
//! instead of a silent CI stall.

use super::*;
use slash::chain::Chain;

/// The wall-clock budget of one soak run. A healthy run of the CI
/// parameters completes in seconds; exceeding the budget means one
/// operation is doing pathological work, and the panic names the
/// operation and the state that produced it.
const SOAK_BUDGET: std::time::Duration = std::time::Duration::from_secs(90);

/// A drill chain: a clone of the shared base with the reorg rewind map
/// and the historical page-root maps dropped. The soak drives blocks
/// exclusively through `apply_recovered` and wholesale chain
/// replacement; the rewind map exists for the reorg mechanism, which no
/// soak code path ever invokes, and the header hash commits only
/// version, parent, time, height, nonce, miner, mined cell, difficulty,
/// and the transaction body hash — never the page roots — so
/// historical roots are dead weight here, and fresh roots are
/// recomputed from the state for every new block. Carrying either
/// through every drill clone multiplies the drill cost without bound as
/// the chain grows.
fn drill_chain(base: &Chain) -> Chain {
    let mut chain = base.clone();
    chain.state_history.clear();
    for block in chain.blocks.iter_mut() {
        block.page_roots.clear();
    }
    chain
}

/// Divergence drill: every node rebuilds its chain as the shared prefix
/// cloned from node 0 extended by its own `fork_len`-block suffix. The
/// chains disagree on the suffix while the prefix — and therefore the
/// conservation invariant — stays identical, exercising the multi-chain
/// bookkeeping without a network transport.
///
/// The drill resets every node onto the base BEFORE building the fork
/// plans: between drills the nodes mine round-robin on their own
/// private chains, so the per-node views diverge, and a plan built
/// against a diverged view would reference cells the base chain does
/// not assign to that miner — the payload would be unexecutable by
/// construction. Building only after the reset keeps every plan a pure
/// function of the shared base.
///
/// The base's rewind map and historical page roots are dropped before
/// the drill: the drill mines hypothetical forks that never drive the
/// reorg mechanism, so both are dead weight on every clone, and clearing
/// them on the source keeps the base copy light.
fn reorg_drill(rng: &mut Rng, nodes: &mut [Node; 3], fork_len: u64) {
    nodes[0].chain.state_history.clear();
    for block in nodes[0].chain.blocks.iter_mut() {
        block.page_roots.clear();
    }
    let base = drill_chain(&nodes[0].chain);
    for node in nodes.iter_mut() {
        node.chain = drill_chain(&base);
    }
    let plans: Vec<Vec<slash::chain::Tx>> = (0..3).map(|i| random_payload(rng, nodes, i)).collect();
    for (i, node) in nodes.iter_mut().enumerate() {
        node.mine(&plans[i]);
        for _ in 1..fork_len {
            node.mine(&[]);
        }
    }
}

/// Run the full multi-node soak: `ops` payload rounds mined round-robin
/// across the three nodes, a divergence drill after every `fork_len`
/// rounds, and the conservation invariant checked on all nodes after
/// each round. The watchdog panics past the wall-clock budget with the
/// operation index and every node's live dimensions — tip, dirty pages,
/// live locks, side-fork pool — so a pathological operation identifies
/// itself and the state that produced it.
pub fn run_network_soak(ops: u64, fork_len: u64) {
    let mut rng = Rng(0x5EED_5EED_5EED_5EED);
    let mut nodes = [
        Node::new("node_a"),
        Node::new("node_b"),
        Node::new("node_c"),
    ];
    let started = std::time::Instant::now();
    for op in 0..ops {
        let miner = (op % 3) as usize;
        let txs = random_payload(&mut rng, &nodes, miner);
        nodes[miner].mine(&txs);
        if fork_len > 0 && op % fork_len == fork_len - 1 {
            reorg_drill(&mut rng, &mut nodes, fork_len);
        }
        for node in nodes.iter_mut() {
            assert_invariants(node);
        }
        assert!(
            started.elapsed() < SOAK_BUDGET,
            "soak exceeded the {:?} budget at op {op}/{ops}: {:?}",
            SOAK_BUDGET,
            nodes
                .iter()
                .map(|n| {
                    (
                        n.chain.tip_height(),
                        n.chain.state.pages.len(),
                        n.chain.state.locks.len(),
                        n.chain.side_forks.len(),
                    )
                })
                .collect::<Vec<_>>()
        );
    }
}
