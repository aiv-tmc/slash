# Slash Protocol — Test Coverage Matrix

**Version:** 0.7.0
**Status:** authoritative. A documented rule without a row in this matrix is treated as not implemented. Every consensus change must update this matrix in the same pull request.

## Legend

| Mark | Meaning |
|---|---|
| Covered | A test exercising the rule exists in the tree and runs in CI. |
| Specified | The rule is implemented; the named test pins it. |
| Partial | The rule is implemented and partially tested; the gap is tracked. |
| Planned | The rule is designed but the implementation or test is pending. |

## 1. Consensus: blocks and headers

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Chain identifier pins the signature domain per network | `chain.rs::MAINNET_ID`, `chain.rs::TESTNET_ID` | `chain_tests.rs::test_genesis_chain_id` | Covered |
| Block extends the tip by height and previous hash | `chain.rs::Chain::check_block_meta` | `chain_tests.rs` reorg/side-fork cases | Covered |
| Median-time rule over the last 11 blocks | `chain.rs::Chain::check_block_meta` | `chain_tests.rs` timestamp cases | Covered |
| Difficulty equals the retargeted value | `chain.rs::Chain::next_difficulty`, `difficulty_for_height` | `slow_chain_tests::meta::test_difficulty_retargets_every_interval` | Specified |
| Retarget clamps to 4x and never below 1 | `chain.rs::Chain::difficulty_for_height` | `chain.rs::tests::schedule_tests::retarget_boundary_applies_clamp` | Covered |
| Zero difficulty never satisfies proof-of-work | `chain.rs::verify_pow` | `chain_tests.rs::test_verify_pow_rejects_zero_difficulty`; `chain/pow.rs::tests::zero_difficulty_never_verifies` | Covered |
| PoW input carries a versioned domain tag through the single shared builder | `chain/pow.rs::POW_DOMAIN`, `chain/pow.rs::pow_input` | `chain/pow.rs::tests::pow_input_prefixes_the_versioned_domain_tag`; `crypto_agility_tests::pow_input_is_domain_tagged` | Covered |
| Miner and verifier hash identical domain-tagged inputs | `chain/pow.rs::pow_input` callers in `mining.rs` | `chain/pow.rs::tests::miner_and_verifier_hash_identical_inputs`; `crypto_agility_tests::pow_verifier_accepts_difficulty_one_and_rejects_zero` | Covered |
| PoW slot cell treasury-owned, never confiscates user cells | `chain.rs::verify_pow` callers; `state.rs::State::mint_slot` | `chain_tests.rs::test_mine_respects_locked_cells`; `state.rs::mint_slot_refuses_non_treasury_cells` | Covered |
| Checkpoint mismatch is a permanent rejection | `chain.rs::Chain::check_block_meta` | `chain_tests.rs` checkpoint cases | Covered |
| Reorg depth limited to the retained history window | `chain.rs::Chain::reorg_to_fork`, `MAX_REORG_DEPTH` | `reorg_persistence_tests` | Covered |
| Orphan adoption bounded per pass | `chain.rs::Chain::process_orphans`, `MAX_ORPHAN_ADOPTION` | `chain.rs::process_orphans_adopts_chains_up_to_the_cap` | Covered |
| Side forks retained up to a bounded pool | `chain.rs::Chain::process_block`, `MAX_SIDE_FORKS` | `reorg_persistence_tests` | Covered |
| Recovery replay refuses broken linkage | `chain.rs::Chain::apply_recovered` | `chain.rs::apply_recovered_rejects_broken_linkage` | Covered |
| Side-fork pool rejects garbage heights/difficulties | `chain.rs::Chain::validate_side_fork_entry` | `chain.rs::side_fork_pool_rejects_garbage` | Covered |
| Header batch validates linkage, median time, retarget, slot | `chain/validation.rs::Chain::validate_headers_batch` | `chain/tests.rs::validate_headers_batch_walks_the_full_window` | Specified |
| Deployment table validated at load (bit < 32, threshold ≤ window) | `lib.rs::enforce_deployment_table` | `load.rs::tests::deployment_table_enforcement_*` | Specified |
| Deployment table lifecycle coherence at load (activation slot, Failed tally) | `load.rs::enforce_deployment_table` | `load.rs::tests::deployment_table_enforcement_*` | Specified |
| LockedIn activates before timeout; non-terminal deployments expire to Failed | `chain/deployments.rs::VersionBits::update` | `version_bits_window_tests::deployment_timeout_expires_to_failed` | Specified |
| Built-in deployments start on retarget window boundaries | `chain/version_bits.rs::VersionBits::new` | `version_bits_window_tests::built_in_start_heights_align_with_retarget_windows` | Covered |

## 2. Emission and coinbase

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Subsidy schedule exact at every era boundary | `chain.rs::subsidy_at` | `emission_harness_tests` boundary vectors | Covered |
| Coinbase mandatory and first while subsidy positive | `chain.rs::Chain::validate_coinbase` | `emission_harness_tests` coinbase shape cases | Covered |
| Coinbase outputs only to miner plus exact dev share | `chain.rs::Chain::validate_coinbase` | `emission_harness_tests` coinbase shape cases | Covered |
| Coinbase amount equals `subsidy_at(height)` | `chain.rs::Chain::validate_coinbase` | `chain_tests.rs::test_coinbase_amount_matches_schedule` | Specified |
| Coinbase inputs are the canonical treasury range | `chain.rs::Chain::validate_coinbase` | `emission_harness_tests::test_coinbase_canonical_inputs_reproduce_root` | Specified |
| Era 9 omits coinbase, empty payload valid | `chain.rs::Chain::validate_coinbase` | `chain_tests.rs::test_era_eight_to_nine_transition` | Specified |
| Perpetual slot tail one cell per block at every height | `state.rs::State::mint_slot` | `chain_tests.rs::test_tail_slot_emission_is_perpetual` | Specified |
| Tail preserves conservation | `state.rs::State::mint_slot` | `state.rs::conservation_invariant` | Specified |
| Cumulative emission never exceeds 417_957_120 | `chain.rs::subsidy_at` | `emission_harness_tests` cap checks | Covered |
| Fee-funded tail: miner revenue = subsidy + claimed fees + slot; vault drained; cache parity | `simulation/tail.rs::simulate_fee_only_tail` | `emission_harness_tests::test_fee_only_tail_simulation_sweep` | Specified |

## 3. Transactions and PMIS

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Conservation of cells per transaction | `state.rs::State::tx` | `state.rs::conservation_invariant` | Covered |
| Minimum 50 cells per transaction | `state.rs::State::tx` | `chain_tests.rs` minimum cases | Covered |
| Mandatory fee ≥ total_out / 200 | `state.rs::State::tx`; `chain.rs::Chain::validate_tx` | `emission_harness_tests::test_min_fee_enforced_at_state_level`; `p2p_tests::test_dos_spam_tx_with_zero_fee_rejected` | Specified |
| Exact output packing | `state.rs::State::apply_transfer`; `chain.rs::tx_structure_valid` | `state.rs::rejection_leaves_no_partial_mutation` | Covered |
| Per-page balance for cross-page transfers | `state.rs::State::apply_transfer` | `state.rs::multi_input_cross_page` | Covered |
| Per-transaction range and page amplification caps | `chain/params.rs::MAX_RANGES_PER_TX`, `MAX_PAGES_PER_TX`; `chain/validation.rs::Chain::tx_caps_valid` | `chain/tests.rs::tx_caps_reject_range_and_page_amplification` | Specified |
| Field caps: signature ≤ 64 bytes, memo ≤ 4096 bytes | `chain/params.rs::MAX_SIG_BYTES`, `MAX_MEMO_BYTES`; `chain/validation.rs::tx_structure_valid` | `chain/tests.rs::tx_field_caps_reject_oversized_sig_and_memo` | Specified |
| Ownership and lock checks before spend | `state.rs::State::owns`, `remove_for_spend` | `state.rs::lock_prevents_spending` | Covered |
| Locked output binds every cell of its range | `state/transfer.rs::apply_transfer` (per-cell registration) | `state/lock_range_tests.rs::locked_range_tail_cannot_be_spent_before_expiry` | Specified |
| Locked range spendable once the height is reached | `state.rs::State::owns` (expiry predicate) | `state/lock_range_tests.rs::full_locked_range_spend_succeeds_after_expiry` | Specified |
| Pruning releases every cell of an expired range | `state/locks.rs::State::prune_locks` (expiry filter) | `state/lock_range_tests.rs::prune_locks_filters_by_expiry_not_cell_index` | Specified |
| Governance weight accrues over the full locked range | `dao.rs::locked_weight`, `total_locked_weight` | `state/lock_range_tests.rs::locked_range_weight_counts_every_cell` | Specified |
| Governance weight requires both lock maps (expiry + creation) | `dao.rs::weight_holds` | `dao_tests::vote_weight_uses_snapshot_not_current_height` | Specified |
| Global lock cap bounds the live map | `state/transfer.rs::stage_transfer` (live count + new width) | `state/lock_range_tests.rs::global_lock_cap_rejects_transfers_past_the_live_map` | Specified |
| Per-transaction locked-width bound | `chain.rs::tx_structure_valid` | `state.rs::lock_cap_is_enforced` | Covered |
| Mempool/consensus structural parity | `chain.rs::Chain::validate_tx` | `p2p_tests` policy cases | Covered |
| Slot mint releases lock only after reassignment | `state.rs::State::mint_slot` | `state.rs::mine_expired_lock_releases_only_after_reassignment` | Covered |
| Fee claims select only unlocked vault cells | `p2p.rs::build_fee_claims` | `p2p.rs::fee_claims_skip_locked_vault_cells` | Covered |
| Empty fee vault yields no claims | `p2p.rs::build_fee_claims` | `p2p.rs::fee_claims_empty_vault_yields_no_claims` | Covered |
| Treasury mint debits unissued supply | `state.rs::State::mint` | `state.rs::treasury_mint_without_prior_mine_uses_unissued_supply` | Covered |
| Unsigned treasury tx rejected | `state.rs::State::tx` | `state.rs::unsigned_treasury_tx_is_rejected` | Covered |
| Wrong-signer treasury tx rejected | `state.rs::State::tx` | `state.rs::treasury_tx_with_wrong_signer_is_rejected` | Covered |
| Treasury tx refused without configured signer | `state.rs::State::tx` | `state.rs::treasury_tx_rejected_without_configured_signer` | Covered |
| Dev-fund tx refused without configured signer | `state/transfer.rs::tx` (signer dispatch) | `dev_fee_tests::dev_fund_spends_require_configured_signer` | Covered |
| Dev-fund spend signed by the configured signer accepted | `state/transfer.rs::tx` (signer dispatch) | `dev_fee_tests::dev_fund_spends_require_configured_signer`; `dev_fee_tests::dev_fund_spends_capped_at_one_per_block` | Covered |
| Rejection leaves no partial mutation | `state.rs::State::apply_transfer` | `state.rs::rejection_leaves_no_partial_mutation` | Covered |
| Locked output back to treasury refused (both paths) | `state/transfer.rs::transfer_gates`; `chain/validation.rs::tx_structure_valid` | `state/lock_range_tests::treasury::locked_sell_to_treasury_is_rejected_by_*` | Covered |
| Signature schemes verify through a fail-closed dispatcher on both paths | `crypto.rs::verify_scheme`, `crypto.rs::scheme_supported`; `state/transfer.rs::transfer_gates` | `crypto.rs::tests::scheme_dispatch_*`; `crypto_agility_tests::scheme_dispatch_is_fail_closed` | Covered |
| Sighash commits to the declared scheme byte | `chain/sighash.rs::tx_signature_hash` | `crypto_agility_tests::sighash_binds_the_declared_scheme` | Covered |

## 4. Treasury and bonding curve

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Buy pair mints exact amount, updates total_sold | `chain.rs::Chain::execute_block_txs` | `treasury_tests` buy-pair cases | Covered |
| Mint without earlier payment rejected | `chain.rs::Chain::execute_block_txs` | `treasury_tests.rs::test_treasury_mint_without_payment_rejected` | Specified |
| Underpayment rejected | `chain.rs::Chain::execute_block_txs` | `treasury_tests` underpayment case | Specified |
| Sell pair pays 98% | `chain.rs::Chain::execute_block_txs`; `treasury.rs::sell_price` | `treasury_tests.rs::test_treasury_sell_pair_payout_98pct` | Specified |
| Sell without vault balance rejected | `chain.rs::Chain::execute_block_txs` | `treasury_tests` vault case | Specified |
| Curve strictly monotonic | `treasury.rs::buy_price` | `treasury_tests.rs::test_treasury_curve_monotonic_in_cells` | Specified |
| Treasury sig required at any non-coinbase position | `chain.rs::Chain::execute_block_txs` | `era_nine_boundary_tests` position-zero case | Covered |
| Signed unpaired mint rejected (era 9, position 0) | `chain.rs::Chain::execute_block_txs` | `era_nine_boundary_tests` signed-unpaired case | Covered |
| Sell quote with zero payout refused | `rpc/quotes.rs::handle_build_treasury_sell` | `rpc` quote cases | Specified |
| DAO-minted cells carry no curve buy-back rights | `dao/state.rs::DaoState::execute_due` (provenance pinning); `state/transfer.rs::tx` (treasury-bound output gate) | `dao_tests::dao_minted_cells_refuse_curve_sellback`; `dao/types.rs::provenance_tests` | Covered |

## 5. Governance and dev fund

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Windowed signaling; reset at boundaries | `chain.rs::Chain::update_version_bits` | `version_bits_window_tests` | Covered |
| Lock-in only at window end, threshold in one window | `chain.rs::Chain::update_version_bits` | `version_bits_window_tests::single_window_reaching_threshold_locks_in_at_window_end` | Covered |
| Activation deferred one interval | `chain.rs::Chain::update_version_bits` | `version_bits_window_tests::locked_in_deployment_activates_at_scheduled_height` | Covered |
| Dev-fund required when active (mempool and consensus) | `state.rs::State::tx` gated in `chain.rs` | `dev_fee_tests` mempool and block cases | Covered |
| No dev-fee obligation before activation | `chain.rs::Chain::execute_block_txs` | `dev_fee_tests::dev_fee_not_required_before_activation` | Covered |
| Rounding max(total_out*bps/10000, 1) | `state.rs::State::tx` | `dev_fee_tests::dev_fee_rounding_pins_minimum_one_cell` | Covered |
| Treasury and pairing exempt | `state.rs::State::tx` | `dev_fee_tests::treasury_and_pairing_paths_exempt_from_dev_fee` | Covered |
| Dev-fund spend cap one per block | `chain.rs::Chain::execute_block_txs` | `dev_fee_tests::dev_fund_spends_capped_at_one_per_block` | Covered |
| DAO ops ride memo, apply after transfer | `dao.rs::DaoState::apply_op`; `chain.rs` | `dao_tests.rs::memo_submit_and_vote_flow_through_blocks` | Covered |
| Snapshot-pinned voting weight (both lock maps) | `dao.rs::locked_weight`, `weight_holds` | `dao_tests.rs::vote_weight_uses_snapshot_not_current_height` | Covered |
| Quorum denominator captured at submission | `dao.rs::DaoState::submit` | `dao_tests.rs::proposal_lifecycle_submit_vote_close_execute` | Specified |
| One vote per address | `dao.rs::DaoState::vote` | `dao_tests.rs::proposal_lifecycle_submit_vote_close_execute` | Covered |
| Quorum against snapshot locked supply | `dao.rs::close_outcome` | `dao_tests.rs::quorum_failure_rejects_proposal` | Covered |
| Execution at end + delay via canonical selection | `dao.rs::execute_due`, `execute_payload` | `dao_tests` lifecycle and treasury cases | Covered |
| Payload structural bounds | `dao.rs::payload_valid` | `dao_tests.rs::payload_validation_bounds_submission` | Covered |
| Legacy treasury txs rejected while DaoV1 active (mempool + consensus) | `chain.rs` both paths | `dao_tests.rs::treasury_transactions_rejected_while_dao_active` | Covered |
| Key rotation executes at scheduled height | `dao.rs::execute_payload` | `dao_tests.rs::rotation_payload_replaces_signer_at_execution` | Covered |
| Terminal proposals pruned after the retention window | `dao.rs::DaoState::prune_terminal` | `dao_tests::terminal_proposals_prune_after_retention` | Covered |
| Emergency pause freezes classes without halting blocks | `state.rs::emergency_active`; `chain/execution/txs.rs`; `chain/validation.rs` | `dao_tests::emergency_pause_freezes_without_halting` | Covered |

## 6. Privacy and networking

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Onion build requires ≥3 relays, typed error | `onion.rs::create_onion` | `onion_tests` relay-count cases | Covered |
| Peel order and exit decryption | `onion.rs::peel`, `decrypt_inner` | `onion_tests` peel cases | Covered |
| Low-order X25519 refused at build, peel, and exit | `crypto.rs::x25519_shared`; `onion/{build,peel}.rs` | `onion_tests::test_onion_*low_order*`; `crypto_tests::test_x25519_shared_rejects_low_order_public_key` | Covered |
| Domain-separated layer keys (routing/inner/marker) | `crypto.rs::kdf_domain` | `onion::tests::honest_onion_still_peels` | Specified |
| Replay cache FIFO + persistence | `p2p.rs::OnionReplayCache` | `p2p.rs` replay cache tests | Covered |
| Onion keys only with verified attestation (freshness nonce + identity sig + curve probe) | `p2p/handlers/sync.rs` | `p2p.rs::peer_id_ed25519_key_feeds_attestation_verification` | Covered |
| Peer identity multihash layout pinned with test vector | `p2p/util.rs::peer_id_ed25519_key` | `p2p/util.rs::tests::peer_id_ed25519_key_pins_the_identity_multihash_layout` | Covered |
| Per-peer token buckets | `p2p.rs` bucket constants | `p2p_tests` rate-limit cases | Covered |
| Per-subnet and node-wide inbound limits | `p2p/conn.rs::ConnectionBudgets` | `p2p/conn.rs::subnet_cap_blocks_further_inbound_until_a_slot_releases`; `p2p/conn.rs::node_wide_cap_counts_all_subnets_together` | Specified |
| Inbound slots release on close | `p2p/conn.rs::ConnectionBudgets::on_closed` | `p2p/conn.rs::inbound_slots_release_when_the_connection_closes` | Specified |
| Outbound dials never consume inbound budget | `p2p/conn.rs::ConnectionBudgets::on_established` | `p2p/conn.rs::outbound_dials_never_consume_inbound_budget` | Specified |
| Strikes and exponential backoff bans | `p2p.rs::ban_duration_secs` | `p2p.rs::ban_duration_escalates_and_saturates` | Covered |
| Request channel throttled (cost-priced bucket, strike on sustained refusal) | `p2p/handlers/sync.rs::serve_request`; `p2p/limits.rs` | `p2p_tests` request-throttle cases | Specified |
| Sampled sync polling | `p2p.rs::SYNC_SAMPLE_SIZE` | `p2p_tests` sync cases | Covered |
| Wire frames bounded | `p2p.rs::MAX_FRAME` | `wire_format_tests` | Covered |
| Onion envelopes length-guarded before deserialization | `p2p/limits.rs::MAX_ONION_ENVELOPE` | `p2p_tests` onion frame cases | Covered |
| Fork/orphan pools byte-budgeted; parked bytes charged to peer | `chain/ingress.rs`; `p2p/limits.rs::charge_block_bytes` | `chain/ingress.rs` byte-cap tests; `p2p/limits.rs` charge tests | Covered |
| Mempool cap enforced at admission time | `p2p/handlers/gossip.rs::handle_tx` | `p2p_tests` mempool cap cases | Covered |
| Re-gossip suppressed during bulk sync | `p2p/handlers/gossip.rs` | `p2p_tests` sync cases | Covered |
| Node identity loaded once per process | `p2p/lifecycle.rs::NODE_SECRET` | `p2p/lifecycle.rs` identity tests | Covered |

## 7. Storage, persistence, cryptography, FFI

| Rule | Implementation | Test | Status |
|---|---|---|---|
| Atomic writes (tmp, fsync, rename) | `storage.rs::atomic_write` | `storage_tests` | Covered |
| Manifest + crash-recovery journal; replay stops at the first refused record | `storage.rs`; `load.rs::recover` | `storage_tests`; `reorg_persistence_tests` | Covered |
| Load-time sanity package (height coordinate, contiguity, checkpoints, cache sum) | `load_sanity.rs::sanity_check` | `load_sanity.rs` tests | Covered |
| Pruned recovery replay bounded | `chain.rs::HISTORY_REBUILD_MAX_REPLAY` | `reorg_persistence_tests` | Covered |
| Dev-fund/DAO state survives restart | `storage.rs::Header` | `storage_tests` round-trip | Specified |
| Ed25519 round-trip | `crypto.rs` | `crypto_tests` | Covered |
| X25519 shared-secret rejection of low-order points | `crypto.rs::x25519_shared` | `crypto_tests::test_x25519_shared_rejects_low_order_public_key` | Covered |
| Wallet encryption Argon2id + AES-GCM (64 MiB / 3 / 4) | `crypto.rs::derive_key` | `integration_tests` wallet cases | Covered |
| Page proofs verify | `state.rs::verify_page_proof` | `state.rs::merkle_root_and_proof` | Covered |
| PoW domain tag separates proof spaces across domains | `chain/pow.rs::POW_DOMAIN` | `chain/pow.rs::tests`; `crypto_agility_tests` | Covered |
| Signature scheme dispatcher is fail-closed | `crypto.rs::verify_scheme` | `crypto.rs::tests::scheme_dispatch_rejects_every_unimplemented_scheme` | Covered |
| FFI datadir selection without working-directory mutation | `storage::set_data_dir`; `ffi.rs::slash_init` | `ffi_tests::datadir_selection_does_not_touch_cwd` | Covered |
| FFI wallet flow: encrypted keypair generation and decryption | `ffi.rs::slash_generate_encrypted_keypair`, `slash_decrypt_secret` | `ffi_tests::wallet_round_trip_via_ffi` | Covered |
| FFI proof verification bounds siblings and index | `ffi.rs::slash_verify_page_proof` | `ffi_tests::proof_verification_rejects_oversized_input` | Covered |

## Maintenance rules

- Every new consensus rule enters with both validation paths (mempool and block execution) named.
- Every bug fix enters with its regression test named.
- Rows are never deleted; closed items move to the appendix with the closing change named.
- CPU-heavy consensus walks (full retarget interval, history-window growth, lock-expiry walks) live in the `slow_chain_tests` integration target and run with limited test threads; the matrix names the target for every row they cover.
