//! The chain aggregate: data layout and constructors.
//!
//! `Chain` bundles everything the node needs to answer "is this block
//! next": the retained block window, the materialized tip state, the
//! bounded histories that make reorgs correct, checkpoints, the side-fork
//! and orphan pools, and the version-bit deployment table. Methods live in
//! the sibling files grouped by responsibility; this file owns the struct
//! and the genesis constructors only.
//!
//! The state and the history entries ride behind `Arc`: block execution
//! is a pure function returning a fresh post-state, so the commit path
//! installs one `Arc` and the history records the same `Arc` — a
//! reference-count bump instead of a deep copy per block, and a cheap
//! snapshot share for fork rehearsal, reorg rehearsal, and drill-style
//! harnesses that clone whole chains.

use super::params::{MAINNET_ID, TESTNET_ID};
use super::types::Block;
use super::version_bits::{Deployment, VersionBits};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The chain: an ordered vector of blocks above `base_height`, the
/// materialized state at the tip, a bounded history of states for reorgs,
/// checkpoints, side forks, orphans, and the version-bit deployments.
#[derive(Clone)]
pub struct Chain {
    pub blocks: Vec<Block>,
    pub base_height: u64,
    /// The current ledger state — the post-state of the tip block. Shared
    /// behind an `Arc`: readers dereference transparently, and the few
    /// legitimate in-place mutations (load-time pruning, fixture seeding)
    /// go through `state_mut`/`Arc::make_mut`, which writes in place while
    /// the reference is unique.
    pub state: Arc<crate::state::State>,
    /// The post-state of every block inside the reorg window, keyed by
    /// height. Values are shared `Arc`s: recording history is a
    /// reference-count bump on the state the chain just installed, not a
    /// copy.
    pub state_history: HashMap<u64, Arc<crate::state::State>>,
    pub deployments_history: HashMap<u64, Vec<Deployment>>,
    pub checkpoints: BTreeMap<u64, [u8; 32]>,
    /// Competing branches the chain has validated but not adopted, keyed
    /// by block hash. Each entry pairs the block with its serialized
    /// size, captured at ingress: the pool enforces a byte ceiling in
    /// addition to the count cap, so an adversary shipping maximum-size
    /// blocks cannot park unbounded memory in every node, and the budget
    /// check sums cached integers instead of re-serializing the pool.
    pub side_forks: HashMap<[u8; 32], (Block, u32)>,
    /// Blocks whose parents are unknown to both the main chain and the
    /// side-fork pool, keyed by block hash, paired with their serialized
    /// size for the same byte-budgeted admission as the side-fork pool.
    pub orphan_blocks: HashMap<[u8; 32], (Block, u32)>,
    /// Blocks disconnected by the most recent reorg, oldest first, so the
    /// node can return their transactions to the mempool.
    pub reorg_disconnected: Vec<Block>,
    pub version_bits: VersionBits,
    pub chain_id: String,
    pub testnet: bool,
}

impl Chain {
    /// Build the genesis chain: the genesis block is implicit (height 0
    /// with an all-zero prev), so the block vector starts empty and the
    /// state starts from the treasury-owned genesis state. The genesis
    /// subsidy of 500 cells is credited inside the initial state.
    pub fn genesis() -> Self {
        Self::with_chain_id(MAINNET_ID.to_string())
    }

    /// Build a genesis chain with an explicit network identifier. The
    /// identifier pins the signature domain for every transaction.
    pub fn with_chain_id(chain_id: String) -> Self {
        let testnet = chain_id == TESTNET_ID;
        let mut state = crate::state::State::genesis();
        // The genesis coinbase is executed directly into the initial state
        // so that height 0 already carries the first era-0 subsidy; the
        // implicit genesis block itself never appears in the block vector.
        let miner = crate::state::TREASURY;
        let coinbase = Self::build_coinbase(&state, miner, 0, 0).expect("genesis coinbase");
        // The genesis coinbase debits the canonical treasury range through
        // the ordinary mint primitive, which packs outputs exactly over the
        // inputs. A failure here is a construction bug and panics.
        assert!(
            state.mint(
                &coinbase.inputs,
                &Self::to_state_outputs(&coinbase.outputs),
                0
            ),
            "genesis mint"
        );
        // The freshly built state becomes the shared tip state: the
        // history records the same allocation, so genesis pays one state
        // build and zero copies.
        let state = Arc::new(state);
        let mut state_history = HashMap::new();
        state_history.insert(0, Arc::clone(&state));
        let mut chain = Self {
            blocks: Vec::new(),
            base_height: 0,
            state,
            state_history,
            // The deployment history starts empty here and receives the
            // genesis deployment table below, so a reorg back to the
            // genesis height can restore the initial rule set.
            deployments_history: HashMap::new(),
            checkpoints: BTreeMap::new(),
            side_forks: HashMap::new(),
            orphan_blocks: HashMap::new(),
            reorg_disconnected: Vec::new(),
            version_bits: VersionBits::new(),
            chain_id,
            testnet,
        };
        chain
            .deployments_history
            .insert(0, chain.version_bits.deployments.clone());
        chain
    }

    /// Build the genesis chain for the testnet identifier: the same
    /// construction as the mainnet genesis under the testnet chain id.
    pub fn testnet_genesis() -> Self {
        Self::with_chain_id(TESTNET_ID.to_string())
    }

    /// Switch the signature domain between mainnet and testnet. Changing
    /// the identifier invalidates every existing signature, which is
    /// exactly the replay isolation the flag exists for.
    pub fn set_testnet(&mut self, testnet: bool) {
        self.testnet = testnet;
        self.chain_id = if testnet {
            TESTNET_ID.to_string()
        } else {
            MAINNET_ID.to_string()
        };
    }

    /// Absolute height of the current tip; the implicit genesis is height 0.
    pub fn tip_height(&self) -> u64 {
        self.blocks.len() as u64 + self.base_height
    }

    /// Hash of the current tip; the implicit genesis hashes to all zeros.
    pub fn tip_hash(&self) -> [u8; 32] {
        self.blocks
            .last()
            .map(|b| self.block_hash(b))
            .unwrap_or([0u8; 32])
    }

    /// Exclusive mutable access to the tip state for fixture seeding and
    /// operator tooling. Every legitimate caller owns its chain uniquely,
    /// so the write goes through `Arc::make_mut` in place; a shared
    /// reference would clone, and the seeding call sites are exactly the
    /// ones that can afford an in-place write but not an accidental deep
    /// copy of the whole ledger.
    pub fn state_mut(&mut self) -> &mut crate::state::State {
        Arc::make_mut(&mut self.state)
    }
}
