//! src/load.rs — the startup recovery path: manifest load with domain
//! gates, snapshot fallback, journal replay, and the bounded state-history
//! rebuild. Every entry point funnels into [`load_pruned`], which never
//! operates on fabricated history: when no exact anchor exists for the
//! reorg window the history stays empty and reorgs fail closed.

use crate::{active_chain_id, chain, state, storage};
use std::sync::atomic::Ordering;

/// The load-time sanity package: a freshly restored triple is verified
/// as one coherent chain before it drives consensus, so a stamp-clean
/// but internally inconsistent restore falls back to snapshot recovery
/// instead of wedging block by block.
#[path = "load_sanity.rs"]
mod sanity;

/// Enforce that on-disk chain data belongs to the replay domain the node
/// was started for: the persisted identifier is authoritative, a mismatch
/// panics, and an empty persisted identifier marks data written before
/// the field existed and is adopted under the active mode.
fn enforce_chain_id(persisted: &[u8], active: &[u8]) {
    if !persisted.is_empty() && persisted != active {
        panic!(
            "[load] chain data belongs to '{}' but the node was started for '{}'; \
             refusing to cross replay domains",
            String::from_utf8_lossy(persisted),
            String::from_utf8_lossy(active)
        );
    }
}

/// Validate a restored version-bit deployment table before it drives
/// consensus. Structural checks reject a bit position outside the 32-bit
/// version field (the signal-count shift would overflow) and a threshold
/// beyond the retarget window (such a deployment could never lock in).
/// Coherence checks reject states the machine never writes: only LockedIn
/// and Active carry a scheduled activation height, and Failed never
/// carries a signal tally.
fn enforce_deployment_table(deployments: &[chain::Deployment]) {
    for d in deployments {
        if d.bit >= 32 || d.threshold as u64 > chain::DIFFICULTY_ADJUSTMENT_INTERVAL {
            panic!(
                "[load] corrupt deployment table: entry '{}' has bit {} / threshold {}; \
                 refusing to load",
                d.name, d.bit, d.threshold
            );
        }
        let scheduled = matches!(
            d.status,
            chain::DeploymentStatus::LockedIn | chain::DeploymentStatus::Active
        );
        if scheduled != d.activation_height.is_some() {
            panic!(
                "[load] incoherent deployment table: entry '{}' in state {:?} \
                 carries activation_height {:?}; refusing to load",
                d.name, d.status, d.activation_height
            );
        }
        if d.status == chain::DeploymentStatus::Failed && d.signals != 0 {
            panic!(
                "[load] incoherent deployment table: failed entry '{}' \
                 carries a stale signal tally {}; refusing to load",
                d.name, d.signals
            );
        }
    }
}

/// Replay the crash-recovery journal into a freshly loaded chain. Replay
/// skips the header checks (including the RandomX proof) and relies on
/// the linkage and state checks in apply_recovered. Replay stops at the
/// first record the chain refuses — every later record presumes the
/// failed one — and the journal is rewritten to exactly the applied
/// prefix so the discarded tail cannot be replayed after the next
/// restart either.
fn recover(mut chain: chain::Chain) -> chain::Chain {
    let journal = storage::journal_load();
    let mut applied = 0usize;
    for b in &journal {
        if let Err(err) = chain.apply_recovered(b.clone()) {
            eprintln!(
                "[load] journal replay refused the record at height {}: {:?}; \
                 recovery stops and the journal is truncated to the applied prefix",
                b.height, err
            );
            break;
        }
        applied += 1;
    }
    if applied < journal.len() {
        // The records past the cut presume the refused record; the
        // journal is rebuilt from exactly the applied prefix, truncated
        // first so an interrupted rewrite still leaves a consistent file.
        let _ = storage::journal_truncate();
        let _ = storage::journal_append_batch(&journal[..applied]);
    }
    let tip_len = chain.blocks.len() as u64;
    let tip = tip_len.saturating_add(chain.base_height).saturating_sub(1);
    chain.state.prune_locks(tip);
    chain
}

/// Rebuild the state history on a freshly loaded chain; the manifest
/// persists only the tip state, so the reorg machinery would otherwise
/// have no historical states to restore. An exact anchor is required —
/// genesis for a short chain, the snapshot before the first stored block
/// for a pruned chain, the newest snapshot inside the reorg window for a
/// long one — and the replay never exceeds HISTORY_REBUILD_MAX_REPLAY;
/// with no exact anchor the history stays empty and reorgs fail closed.
fn restore_history(chain: &mut chain::Chain) {
    let tip = (chain.blocks.len() as u64)
        .saturating_add(chain.base_height)
        .saturating_sub(1);
    let anchor = if chain.base_height == 0 && tip <= chain::HISTORY_REBUILD_MAX_REPLAY {
        // Short full chain: replaying from genesis stays inside the bound.
        Some((state::State::genesis(), 0u64))
    } else if chain.base_height > 0 {
        // The state after block base_height - 1 is the only valid replay
        // start for the stored window, which never exceeds the bound.
        let want = chain.base_height - 1;
        storage::load_latest_snapshot(want)
            .ok()
            .filter(|(h, _)| *h == want)
            .map(|(h, s)| (s, h))
    } else {
        // Long full chain: the newest snapshot at or below the reorg
        // window is the only anchor cheap enough to replay from.
        storage::load_latest_snapshot(tip.saturating_sub(chain::MAX_REORG_DEPTH))
            .ok()
            .map(|(h, s)| (s, h))
    };
    if let Some((anchor_state, anchor_height)) = anchor {
        if chain
            .rebuild_state_history(anchor_state, anchor_height)
            .is_err()
        {
            // Replay failure means corruption; without a trustworthy
            // history every reorg must be rejected.
            chain.state_history.clear();
        }
    }
}

/// Load the chain, recovering from the most recent snapshot when the live
/// state files are damaged. First attempts a manifest-based load; on
/// failure uses the latest snapshot and replays the blocks stored after
/// the snapshot height. Governance metadata is always validated before
/// it drives consensus, and the state history is either rebuilt from an
/// exact anchor or left empty so reorgs fail closed.
pub fn load_pruned() -> chain::Chain {
    // Manifest load first: the manifest is written last by every
    // consistent save, so its presence certifies the triple.
    if let Ok((blocks, state, meta)) = storage::load_from_manifest_full() {
        let active = active_chain_id();
        enforce_chain_id(&meta.chain_id, &active);
        // The deployment table drives the signal-count shifts and the
        // window thresholds of every future block; load it only after
        // the structural gate above has certified it.
        enforce_deployment_table(&meta.deployments);
        let mut chain = chain::Chain {
            blocks,
            base_height: meta.base_height,
            state,
            // History is rebuilt from an exact anchor below; it starts
            // empty so a failed rebuild can never leave fabricated
            // entries behind.
            state_history: std::collections::HashMap::new(),
            deployments_history: std::collections::HashMap::new(),
            checkpoints: meta.checkpoints,
            version_bits: chain::VersionBits {
                deployments: meta.deployments,
            },
            chain_id: if meta.chain_id.is_empty() {
                String::from_utf8_lossy(&active).into_owned()
            } else {
                // The persisted identifier is stored as raw bytes; the
                // in-memory chain carries it as an owned string.
                String::from_utf8_lossy(&meta.chain_id).into_owned()
            },
            // The testnet flag mirrors the process-wide network selector
            // so the rebuilt chain signs against the same chain id the
            // rest of the node uses.
            testnet: crate::TESTNET_MODE.load(Ordering::Relaxed),
            orphan_blocks: std::collections::HashMap::new(),
            side_forks: std::collections::HashMap::new(),
            reorg_disconnected: Vec::new(),
        };
        // The stamps certify the files as one atomic write; the sanity
        // package certifies the triple as one coherent chain. A shifted
        // height coordinate, a checkpoint disagreement, or a broken
        // conservation invariant means the files came from different
        // moments even though every file matches its stamp, and
        // continuing on such a triple would wedge the node block by
        // block — so a failure falls through to snapshot recovery with
        // the named diagnostic instead of driving consensus.
        if let Err(reason) = sanity::sanity_check(&chain) {
            eprintln!(
                "[load] manifest triple failed the sanity package ({reason}); \
                 falling back to snapshot recovery"
            );
        } else {
            restore_history(&mut chain);
            return recover(chain);
        }
    }

    // Normal load failed: recover from the most recent snapshot. When the
    // snapshot record carries a deployment table and checkpoint map, the
    // retained window replays from exactly that table — every applied
    // block advances it through the ordinary commit path — so the
    // in-memory deployments become a pure function of the snapshot plus
    // the retained blocks instead of a trusted copy of whatever a
    // previous process persisted. A legacy bare-state snapshot carries no
    // governance and is backfilled from the persisted metadata below.
    let fallback = storage::SnapshotRecord {
        state: state::State::genesis(),
        deployments: Vec::new(),
        checkpoints: std::collections::BTreeMap::new(),
    };
    let (snap_height, record) =
        storage::load_latest_snapshot_record(u64::MAX).unwrap_or((0, fallback));
    let active = active_chain_id();
    let mut chain = chain::Chain {
        blocks: Vec::new(),
        base_height: snap_height,
        state: record.state,
        state_history: std::collections::HashMap::new(),
        deployments_history: std::collections::HashMap::new(),
        checkpoints: record.checkpoints,
        version_bits: chain::VersionBits {
            deployments: record.deployments,
        },
        chain_id: String::from_utf8_lossy(&active).into_owned(),
        testnet: crate::TESTNET_MODE.load(Ordering::Relaxed),
        orphan_blocks: std::collections::HashMap::new(),
        side_forks: std::collections::HashMap::new(),
        reorg_disconnected: Vec::new(),
    };

    // Replay any blocks stored after the snapshot height through the
    // ordinary apply path in height order; a record the chain refuses
    // ends the replay, since every record above it presumes it. Every
    // applied block also advances the version-bit table from the
    // snapshot's seed, making the retained window a pure function of the
    // on-disk data.
    let stored: Vec<chain::Block> = std::fs::read(crate::storage::data_path("chain_blocks.bin"))
        .ok()
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
        .unwrap_or_default();
    for b in stored.into_iter().filter(|b| b.height > snap_height) {
        if chain.apply(b).is_err() {
            break;
        }
    }

    // Backfill governance only when the snapshot carried none; the
    // persisted metadata is validated before it drives consensus.
    if let Ok(meta) = storage::load_chain_meta_full() {
        enforce_chain_id(&meta.chain_id, &active);
        enforce_deployment_table(&meta.deployments);
        if chain.version_bits.deployments.is_empty() {
            chain.checkpoints = meta.checkpoints;
            chain.version_bits.deployments = meta.deployments;
        }
    }
    restore_history(&mut chain);
    recover(chain)
}

/// Reduce the stored block list to the pruned retention window, dropping
/// the oldest blocks and advancing the base height by the same amount,
/// in place: the caller keeps ownership of the same chain value.
pub fn prune(chain: &mut chain::Chain) {
    if chain.blocks.len() as u64 > crate::p2p::PRUNED_KEEP_BLOCKS {
        let excess = (chain.blocks.len() as u64).saturating_sub(crate::p2p::PRUNED_KEEP_BLOCKS);
        chain.blocks.drain(..excess as usize);
        chain.base_height = chain.base_height.saturating_add(excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The built-in deployment table, used as the baseline the
    /// enforcement tests mutate.
    fn built_in_table() -> Vec<chain::Deployment> {
        chain::VersionBits::default().deployments
    }

    #[test]
    fn enforce_chain_id_panics_on_mismatch() {
        let result = std::panic::catch_unwind(|| {
            enforce_chain_id(b"/slash/other", chain::MAINNET_ID.as_bytes());
        });
        assert!(result.is_err());
    }

    #[test]
    fn enforce_chain_id_accepts_empty_persisted() {
        enforce_chain_id(b"", chain::MAINNET_ID.as_bytes());
    }

    #[test]
    fn enforce_chain_id_accepts_matching() {
        enforce_chain_id(chain::MAINNET_ID.as_bytes(), chain::MAINNET_ID.as_bytes());
    }

    #[test]
    fn deployment_table_enforcement_rejects_bad_bit() {
        let mut table = built_in_table();
        table[0].bit = 32;
        let result = std::panic::catch_unwind(|| enforce_deployment_table(&table));
        assert!(result.is_err());
    }

    #[test]
    fn deployment_table_enforcement_rejects_bad_threshold() {
        let mut table = built_in_table();
        table[0].threshold = chain::DIFFICULTY_ADJUSTMENT_INTERVAL as u32 + 1;
        let result = std::panic::catch_unwind(|| enforce_deployment_table(&table));
        assert!(result.is_err());
    }

    #[test]
    fn deployment_table_enforcement_accepts_built_in() {
        enforce_deployment_table(&built_in_table());
    }
}
