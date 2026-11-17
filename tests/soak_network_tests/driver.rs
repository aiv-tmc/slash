//! Soak driver: runs the three-node simulation for `ops` operations,
//! injecting a divergence drill every `fork_len` operations, and checks
//! the conservation invariant on every node after every operation.
//!
//! A soak test must fail, never hang: a per-operation watchdog aborts
//! the process with the last heartbeat context (operation index and
//! every node's live dimensions) the moment a single operation stalls
//! past its wall-clock budget, so a regression surfaces as one
//! localized, information-rich failure instead of a silent CI stall.
//! The between-operations budget assert remains as a backstop against
//! aggregate slowness across the whole run.

use super::*;
use slash::chain::Chain;

/// Extra wall-clock slack for one soak run, added on top of the
/// per-operation allowance. A healthy run of the CI parameters completes
/// in well under a minute; exceeding the budget means the aggregate
/// per-operation cost has degraded, and the panic names the operation
/// and the state that produced it.
const SOAK_BUDGET_SLACK_SECS: u64 = 60;

/// Per-operation allowance inside the whole-run budget. Real mining
/// (nonce grinding plus template preparation and recovery application)
/// runs several full state transitions and RandomX verifications per
/// operation, so the allowance reflects a slow shared CI runner, not
/// the average development machine.
const SOAK_BUDGET_PER_OP_SECS: u64 = 12;

/// The wall-clock budget of a single soak operation. Every operation —
/// a payload round or a divergence drill — must report a heartbeat
/// within this window; the watchdog aborts the process with the last
/// heartbeat detail otherwise. The budget is sized for the slowest
/// legitimate operation, not for the average case.
const PER_OP_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(120);

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
/// cloned from node 0 extended by its own suffix. The chains disagree
/// on the suffix while the prefix — and therefore the conservation
/// invariant — stays identical, exercising the multi-chain bookkeeping
/// without a network transport. The leader node builds the full
/// `fork_len`-block suffix so the shared base keeps growing across
/// drills; the challengers stop after their payload block, because the
/// drill replaces chains wholesale and never drives the reorg mechanism —
/// a deeper challenger fork would exercise nothing extra while every
/// suffix block pays for a fresh RandomX context, which dominates the
/// drill wall time.
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
pub(crate) fn reorg_drill(rng: &mut Rng, nodes: &mut [Node; 3], fork_len: u64) {
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
        let depth = if i == 0 { fork_len } else { 1 };
        node.mine(&plans[i]);
        for _ in 1..depth {
            node.mine(&[]);
        }
    }
}

/// Run the full multi-node soak: `ops` payload rounds mined round-robin
/// across the three nodes, a divergence drill after every `fork_len`
/// rounds, and the conservation invariant checked on all nodes after
/// each round. The per-operation watchdog aborts the process past its
/// wall-clock budget with the last heartbeat context, and the
/// whole-run budget assert panics with the operation index and every
/// node's live dimensions — tip, dirty pages, live locks, side-fork
/// pool — so a pathological operation identifies itself and the state
/// that produced it.
pub fn run_network_soak(ops: u64, fork_len: u64) {
    let soak_budget = std::time::Duration::from_secs(
        SOAK_BUDGET_SLACK_SECS + SOAK_BUDGET_PER_OP_SECS.saturating_mul(ops),
    );
    let watchdog = Watchdog::start(PER_OP_WATCHDOG);
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
        let dims: Vec<_> = nodes
            .iter()
            .map(|n| {
                (
                    n.chain.tip_height(),
                    n.chain.state.pages.len(),
                    n.chain.state.locks.len(),
                    n.chain.side_forks.len(),
                )
            })
            .collect();
        eprintln!(
            "[soak] op {op}/{ops} miner={miner} tips/pages/locks/forks per node: \
             {dims:?} elapsed {:?}",
            started.elapsed()
        );
        watchdog.beat(format!(
            "op {op}/{ops} miner={miner} tips/pages/locks/forks per node: {dims:?}"
        ));
        assert!(
            started.elapsed() < soak_budget,
            "soak exceeded the {soak_budget:?} budget at op {op}/{ops}: {:?}",
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
