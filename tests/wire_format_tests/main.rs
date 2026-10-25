//! Wire-format contract tests for the consensus-critical bincode schemas.
//!
//! Every node, wallet, and explorer must agree on the exact byte layout of
//! `Tx`, `Block`, and `Output`. These tests pin the serialized sizes of the
//! canonical shapes and the stability of the roundtrip so that an
//! accidental layout change (a reordered field, a changed integer width, a
//! different bincode configuration) fails loudly in CI instead of forking
//! the network silently.
//!
//! A size assertion failure is an intentional serialization fork: update
//! the constants and the layout comment deliberately, and bump the network
//! protocol identifier so old nodes reject the new format.
//!
//! P2P message sizes (`Req`/`Resp`) are pinned separately in
//! `p2p_tests.rs`; this file covers the block and transaction layer.

use slash::chain::Output;
use slash::chain::{self, Block, Tx};
use std::collections::BTreeMap;

/// Minimal transaction: from (32) + inputs vec length (8) + outputs vec
/// length (8) + sig vec length (8) + scheme (1) = 57 bytes.
const TX_MINIMAL_SIZE: usize = 57;

/// Empty block: version (4) + prev (32) + time (8) + height (8) + nonce (8)
/// + miner (32) + mined_cell (8) + txs vec length (8) + fee_claims vec
///   length (8) + difficulty (8) + page_roots map length (8) = 132 bytes.
const BLOCK_EMPTY_SIZE: usize = 132;

/// Output with a lock: start (8) + end (8) + to (32) + Option discriminant
/// (1) + lock payload (8) = 57 bytes.
const OUTPUT_LOCKED_SIZE: usize = 57;

/// Output without a lock: start (8) + end (8) + to (32) + Option
/// discriminant (1) = 49 bytes.
const OUTPUT_UNLOCKED_SIZE: usize = 49;

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod test_block_empty_size;
mod test_field_mutation_changes_serialization;
mod test_output_sizes;
mod test_roundtrip_preserves_structure;
mod test_tx_minimal_size;
mod test_wire_format_contract;
