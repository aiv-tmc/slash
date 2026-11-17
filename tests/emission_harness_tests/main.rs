//! Local accelerated harness for the emission schedule and the coinbase
//! rules (design document sections 3-5 and 9.1).
//!
//! The harness crosses every halving boundary and at least two difficulty
//! retarget intervals without paying for RandomX proof-of-work:
//! - era boundaries and cumulative emission are verified against the exact
//!   design-document table at pure function level and through state-level
//!   minting that mirrors the coinbase execution;
//! - retarget intervals are exercised by pushing synthetic headers with a
//!   controlled mock clock directly into the public block vector, which is
//!   sufficient because the retarget formula reads only timestamps and
//!   difficulties;
//! - the coinbase shape regressions from design-document section 9.1 run
//!   through the public `Chain::validate_block` entry point.

pub use slash::test_fixtures::reset_testnet;

use slash::chain::{self, Block, Tx};
use slash::crypto;
use slash::state::{Output, State, FEE_VAULT, N, PAGE_SIZE, TREASURY};
use std::collections::BTreeMap;

/// Total emission over all nine eras, as specified in the design document.
const EMISSION_CAP: u64 = 417_957_120;

/// Subsidy value at the first block of every era, indexed by era.
const ERA_SUBSIDIES: [u64; 9] = [500, 250, 125, 62, 31, 15, 7, 3, 1];

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod test_coinbase_amount_matches_schedule;
mod test_coinbase_canonical_inputs_reproduce_root;
mod test_coinbase_outputs_only_to_miner;
mod test_coinbase_shape_required_in_era_zero;
mod test_emission_cumulative_matches_design_document;
mod test_era_eight_to_nine_transition;
mod test_fee_only_tail_simulation_sweep;
mod test_min_fee_enforced_at_state_level;
mod test_retarget_intervals_on_mock_clock;
mod test_subsidy_at_exact_boundary_values;
mod test_tail_slot_emission_is_perpetual;

mod support;
pub use support::*;
