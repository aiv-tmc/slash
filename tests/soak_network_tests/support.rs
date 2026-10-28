//! Node fixture of the network soak: an independent miner with its own
//! chain, standing in for one node of the simulated network, plus the
//! deterministic generator the builders draw from.

use super::*;
use slash::chain::Chain;

/// Deterministic generator over a wrapped u64 state: every draw mixes
/// the state and takes it modulo the bound, so a soak run is a pure
/// function of its seed and replays bit for bit.
pub struct Rng(pub u64);

impl Rng {
    /// The next draw in `[0, bound)`.
    pub fn below(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound.max(1)
    }

    /// The next draw in `[0, bound]`, both ends inclusive.
    pub fn below_inclusive(&mut self, bound: u64) -> u64 {
        self.below(bound.saturating_add(1))
    }
}

/// One soak node: an independent miner with its own private chain. The
/// simulation never opens sockets; the node's private chain stands in
/// for its view of the network, and the driver asserts the per-node
/// invariants after every operation.
pub struct Node {
    pub id: usize,
    pub miner_sk: [u8; 32],
    pub miner_pk: [u8; 32],
    pub chain: Chain,
}

impl Node {
    /// Fresh node: a random keypair and a genesis chain.
    pub fn new(label: &str) -> Self {
        let _tmp = setup_test_dir(label);
        reset_testnet();
        let (miner_sk, miner_pk) = slash::crypto::generate_keypair();
        Self {
            id: 0,
            miner_sk,
            miner_pk,
            chain: Chain::genesis(),
        }
    }

    /// Mine one block carrying `payload` onto the node's private chain.
    ///
    /// The soak exercises the multi-chain bookkeeping and the consensus
    /// state transitions — coinbase, slot mint, transactions, pairing —
    /// and never the proof of work, so the block is built without
    /// RandomX: the nonce stays zero and the block applies through the
    /// recovery path, which executes the identical state transition and
    /// skips only the header checks. The PoW slot is the first
    /// treasury-owned cell at or after the coinbase range that no
    /// transaction input claims: the slot mint executes before the
    /// transactions, so it must not confiscate any claimed cell. The
    /// scan is bounded by the total cell supply — the slot selection is
    /// a pure function of the block and the pre-block state, so the
    /// bound can only be hit by a construction defect, and it panics
    /// with the frontier instead of scanning forever.
    pub fn mine(&mut self, payload: &[slash::chain::Tx]) -> bool {
        let height = self.chain.tip_height() + 1;
        let miner = self.miner_pk;
        let mut txs: Vec<slash::chain::Tx> = Vec::new();
        let mut frontier = 0u64;
        if slash::chain::subsidy_at(height) > 0 {
            let coinbase = slash::chain::Chain::build_coinbase(&self.chain.state, miner, height, 0)
                .expect("coinbase while the subsidy is positive");
            frontier = coinbase.inputs.first().map(|&(_, e)| e).unwrap_or(0);
            txs.push(coinbase);
        }
        txs.extend_from_slice(payload);
        let slot = {
            let mut candidate = frontier;
            loop {
                let treasury = self
                    .chain
                    .state
                    .get(candidate)
                    .is_some_and(|(_, _, owner)| owner == slash::state::TREASURY);
                let claimed = txs.iter().any(|tx| {
                    tx.inputs
                        .iter()
                        .any(|&(s, e)| candidate >= s && candidate < e)
                });
                if treasury && !claimed {
                    break candidate;
                }
                candidate += 1;
                assert!(
                    candidate < slash::state::N,
                    "slot search exhausted the cell supply at frontier {frontier} \
                     (tip {}, block txs {:?})",
                    self.chain.tip_height(),
                    txs.iter().map(|t| t.inputs.clone()).collect::<Vec<_>>()
                );
            }
        };
        let block = slash::chain::Block {
            version: 1,
            prev: self.chain.tip_hash(),
            time: 1_700_000_000 + height,
            height,
            nonce: 0,
            miner,
            mined_cell: slot,
            txs,
            fee_claims: Vec::new(),
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        };
        self.chain.apply_recovered(block).is_ok()
    }
}

/// The conservation invariant: the balance cache is the O(1) mirror of
/// the ledger and must always sum to exactly N cells.
pub fn assert_invariants(node: &Node) {
    let supply: u64 = node.chain.state.balance_cache.values().sum();
    assert_eq!(
        supply,
        slash::state::N,
        "node {}: conservation invariant violated",
        node.id
    );
}
