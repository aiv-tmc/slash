//! End-to-end tests for P2P wire format, request-response codec,
//! mempool policy simulation, and DoS resistance through chain-level validation.

use slash::test_fixtures::*;
use slash::*;

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod test_block_gossip_payload_size;
mod test_dos_future_timestamp_rejected;
mod test_dos_invalid_pow_rejected;
mod test_dos_oversized_block_rejected;
mod test_dos_spam_tx_with_zero_fee_rejected;
mod test_header_first_validation_chain;
mod test_mempool_cap_eviction_simulated;
mod test_mempool_double_spend_rejected_at_block_level;
mod test_mempool_rate_limiting_simulated;
mod test_req_get_block_serialization;
mod test_req_get_blocks_serialization;
mod test_resp_block_serialization;
mod test_resp_headers_serialization;
