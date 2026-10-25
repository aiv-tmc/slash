//! Full-state snapshots for pruned recovery: one file per retained
//! height, pruned to the three most recent so the directory cannot grow
//! without bound. A snapshot carries the state together with the
//! governance metadata that was in effect at its height — the
//! version-bit deployment table and the checkpoint map — so a node
//! recovering from a snapshot replays its retained block window against
//! exactly the table the snapshot recorded, and the in-memory deployments
//! become a pure function of (snapshot table + retained blocks) instead
//! of a trusted copy of whatever a previous process persisted.

use super::{atomic_write, data_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One full-state snapshot plus the governance metadata in effect at its
/// height. Old snapshot files hold a bare `State`; they deserialize into
/// this record with empty governance, which the loader then backfills
/// from the persisted chain metadata exactly as before.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub state: crate::state::State,
    pub deployments: Vec<crate::chain::Deployment>,
    pub checkpoints: BTreeMap<u64, [u8; 32]>,
}

/// Save a full state snapshot at a specific block height without
/// governance metadata. Snapshots written through this entry point carry
/// an empty deployment table and checkpoint map; the loader backfills
/// those from the persisted chain metadata when it reads the snapshot
/// back, exactly as it treats legacy bare-state snapshot files.
pub fn save_snapshot(state: &crate::state::State, height: u64) -> anyhow::Result<()> {
    save_snapshot_record(
        &SnapshotRecord {
            state: state.clone(),
            deployments: Vec::new(),
            checkpoints: BTreeMap::new(),
        },
        height,
    )
}

/// Save a full snapshot record at a specific block height. Snapshots
/// allow pruned nodes to recover state without replaying from genesis,
/// and the record form additionally anchors governance recovery: the
/// retained block window replays from the recorded table, so the table
/// stays a pure function of the data on disk.
pub fn save_snapshot_record(record: &SnapshotRecord, height: u64) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_path("snapshots"))?;
    let path = format!("snapshots/state_{}.bin", height);
    let encoded = bincode::serialize(record)?;
    atomic_write(&path, &encoded)?;
    Ok(())
}

/// Load the most recent snapshot record with height less than or equal to
/// `max_height`. Candidates are examined newest first: the first file
/// that reads and deserializes cleanly wins, so a torn newest snapshot
/// degrades gracefully to the previous consistent anchor instead of
/// failing the whole load. Both the record format and the legacy
/// bare-state format are accepted; a legacy file yields a record with
/// empty governance for the caller to backfill.
pub fn load_latest_snapshot_record(max_height: u64) -> anyhow::Result<(u64, SnapshotRecord)> {
    let mut candidates: Vec<(u64, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(data_path("snapshots")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(num) = name
                .strip_prefix("state_")
                .and_then(|s| s.strip_suffix(".bin"))
            {
                if let Ok(h) = num.parse::<u64>() {
                    if h <= max_height {
                        candidates.push((h, entry.path()));
                    }
                }
            }
        }
    }
    // Newest first: a snapshot that fails to read or parse is skipped, so
    // damage to the most recent file falls through to the previous anchor.
    candidates.sort_by_key(|(h, _)| std::cmp::Reverse(*h));
    for (h, path) in candidates {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        // The record format is tried first; a legacy file holds exactly a
        // State and fails this attempt, after which the bare form is
        // accepted with empty governance.
        if let Ok(record) = bincode::deserialize::<SnapshotRecord>(&bytes) {
            return Ok((h, record));
        }
        if let Ok(state) = bincode::deserialize::<crate::state::State>(&bytes) {
            return Ok((
                h,
                SnapshotRecord {
                    state,
                    deployments: Vec::new(),
                    checkpoints: BTreeMap::new(),
                },
            ));
        }
    }
    Err(anyhow::anyhow!(
        "no readable snapshot found up to height {}",
        max_height
    ))
}

/// Load the most recent snapshot with height less than or equal to
/// `max_height` and return only its state half. Convenience wrapper over
/// [`load_latest_snapshot_record`] for callers that do not participate in
/// governance recovery.
pub fn load_latest_snapshot(max_height: u64) -> anyhow::Result<(u64, crate::state::State)> {
    let (h, record) = load_latest_snapshot_record(max_height)?;
    Ok((h, record.state))
}

/// Retain only the three most recent snapshots and delete all older ones.
/// This prevents unbounded growth of the snapshots directory.
pub fn prune_snapshots() -> anyhow::Result<()> {
    let mut snapshots: Vec<(u64, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(data_path("snapshots")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(num) = name
                .strip_prefix("state_")
                .and_then(|s| s.strip_suffix(".bin"))
            {
                if let Ok(h) = num.parse::<u64>() {
                    snapshots.push((h, entry.path()));
                }
            }
        }
    }
    snapshots.sort_by_key(|(h, _)| *h);
    if snapshots.len() > 3 {
        for (_, path) in &snapshots[..snapshots.len() - 3] {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}
