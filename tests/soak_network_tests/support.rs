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
    /// and never the difficulty arithmetic, so the block keeps the
    /// difficulty at one. The proof-of-work slot rule is consensus and
    /// stays live: the nonce is ground until the RandomX hash derives a
    /// slot cell that is treasury-owned and unclaimed, exactly as a real
    /// miner would search. The slot mint executes before the
    /// transactions, so the grind rejects any cell a transaction input
    /// claims and the mint can never confiscate a claimed cell. With the
    /// treasury holding nearly the whole supply, the first or second
    /// nonce qualifies. The ground template then records its dirty-page
    /// roots through `prepare_block`, and the block applies through the
    /// recovery path, which re-executes the identical state transition
    /// and skips only the header checks. Returns whether the block was
    /// accepted.
    pub fn mine(&mut self, payload: &[slash::chain::Tx]) -> bool {
        let trace = std::env::var_os("SLASH_SOAK_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let height = self.chain.tip_height() + 1;
        let miner = self.miner_pk;
        if trace {
            eprintln!(
                "[mine] h={height} tip={} +{:?} building coinbase",
                self.chain.tip_height(),
                t0.elapsed()
            );
        }
        let mut txs: Vec<slash::chain::Tx> = Vec::new();
        if slash::chain::subsidy_at(height) > 0 {
            let coinbase = slash::chain::Chain::build_coinbase(&self.chain.state, miner, height, 0)
                .expect("coinbase while the subsidy is positive");
            txs.push(coinbase);
        }
        if trace {
            eprintln!(
                "[mine] h={height} +{:?} coinbase ready, assembling block",
                t0.elapsed()
            );
        }
        txs.extend_from_slice(payload);
        let mut block = slash::chain::Block {
            version: 1,
            prev: self.chain.tip_hash(),
            time: 1_700_000_000 + height,
            height,
            nonce: 0,
            miner,
            mined_cell: 0,
            txs,
            fee_claims: Vec::new(),
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        };
        if trace {
            eprintln!("[mine] h={height} +{:?} grinding nonce", t0.elapsed());
        }
        slash::test_fixtures::grind_nonce(&self.chain, &mut block);
        if trace {
            eprintln!(
                "[mine] h={height} +{:?} ground nonce={} cell={}",
                t0.elapsed(),
                block.nonce,
                block.mined_cell
            );
        }
        let template = self.chain.prepare_block(block);
        if trace {
            eprintln!(
                "[mine] h={height} +{:?} template prepared, applying",
                t0.elapsed()
            );
        }
        let ok = self.chain.apply_recovered(template).is_ok();
        if trace {
            eprintln!("[mine] h={height} +{:?} apply ok={ok}", t0.elapsed());
        }
        ok
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
