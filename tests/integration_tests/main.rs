//! End-to-end integration tests: wallet encryption, sync semantics,
//! staking, pruning, and persistence, each in its own module for
//! readability. The whole library surface and the shared test fixtures
//! are re-exported publicly from the crate root, so every test module
//! reaches them through one `use super::*;` glob; each test file also
//! imports the `Chain` type explicitly so it compiles on its own.

pub use slash::test_fixtures::*;
pub use slash::*;

mod test_auto_recovery_from_snapshot;
mod test_block_hash_deterministic;
mod test_difficulty_clamp;
mod test_dos_spam_rejection;
mod test_duplicate_block_rejected;
mod test_empty_block_valid;
mod test_genesis_chain_id;
mod test_header_first_download_range;
mod test_invalid_height_sequence;
mod test_median_timestamp;
mod test_mempool_overflow_eviction;
mod test_multi_input_balance_per_page;
mod test_p2p_sync_tip_request;
mod test_pristine_page_root_constant;
mod test_rate_limiting_simulation;
mod test_reorg_state_consistency;
mod test_select_single_contiguous;
mod test_snapshot_pruning;
mod test_staking_creates_locked_output;
mod test_treasury_buy_price_integral;
mod test_treasury_secret_encryption;
mod test_wallet_encryption_roundtrip;
