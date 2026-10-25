//! The state files themselves: an append-only page log with a seek index,
//! plus the legacy per-page directory as a read fallback. Saving appends
//! only the pages mutated since the last save — tracked by the state
//! itself — instead of rewriting every dirty page, and the sidecar index
//! turns loads into direct seeks. The header still carries the page id
//! list, so readers that predate the log keep working against the legacy
//! directory until the first log save migrates the pages forward.

use super::{atomic_write, data_path, Header};
use std::collections::BTreeMap;

/// Append-only log of page versions. Each save appends the serialized
/// pages it touched; the index sidecar maps page id to (offset, length),
/// so a load never scans the log.
const PAGES_LOG: &str = "pages.log";
/// Seek index for the page log: page id to (byte offset, byte length).
const PAGES_INDEX: &str = "pages.index";
/// When the log grows past this size, the next save compacts it: the log
/// is rewritten holding exactly one current version of every dirty page.
const COMPACT_THRESHOLD_BYTES: u64 = 256 * 1024 * 1024;

/// Read the page-log index when it exists and parses; an absent or
/// corrupt index yields an empty map, which the save path then rebuilds
/// by migrating the current pages into the log.
fn read_page_index() -> Vec<(u16, u64, u64)> {
    std::fs::read(data_path(PAGES_INDEX))
        .ok()
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
        .unwrap_or_default()
}

/// Rewrite the page log with exactly one current version of every indexed
/// page, then update the index in place. Called when the log passes the
/// compaction threshold; bounded by the dirty-set size, not the log's
/// history length.
fn compact_page_log(
    state: &crate::state::State,
    index: &mut Vec<(u16, u64, u64)>,
) -> anyhow::Result<()> {
    use std::io::Write;
    let staged = data_path("pages.log.tmp");
    let mut file = std::fs::File::create(&staged)?;
    let mut rebuilt = Vec::with_capacity(index.len());
    for (pid, _, _) in index.iter() {
        let page = state
            .pages
            .get(pid)
            .expect("indexed page is dirty in memory");
        let bytes = bincode::serialize(page)?;
        let off = file.metadata()?.len();
        file.write_all(&bytes)?;
        rebuilt.push((*pid, off, bytes.len() as u64));
    }
    file.sync_all()?;
    std::fs::rename(&staged, data_path(PAGES_LOG))?;
    rebuilt.sort_by_key(|(p, _, _)| *p);
    *index = rebuilt;
    Ok(())
}

/// Persist the state: append the touched pages to the page log, refresh
/// the seek index, and write the header last. Pages are appended at their
/// current version; unchanged pages are not rewritten, so a steady-state
/// save costs O(touched) instead of O(dirty). The header still lists
/// every dirty page id, so a reader that falls back to the legacy
/// directory layout finds the same page set described.
pub fn save(state: &crate::state::State) -> anyhow::Result<()> {
    // Drain the touched set first: even if a later step fails, the worst
    // outcome is one redundant rewrite of these pages on the next save,
    // never a missed write.
    let touched: Vec<u16> = {
        let mut guard = state
            .touched_pages
            .lock()
            .expect("touched-pages lock poisoned");
        guard.drain().collect()
    };
    let mut index = read_page_index();
    // Migrate any dirty page that has no log entry yet — pages loaded
    // from the legacy per-file layout — so the log is authoritative from
    // this save onward.
    let pending: Vec<u16> = touched
        .into_iter()
        .chain(
            state
                .pages
                .keys()
                .copied()
                .filter(|p| !index.iter().any(|(q, _, _)| q == p)),
        )
        .collect();
    if !pending.is_empty() {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data_path(PAGES_LOG))?;
        for pid in pending {
            let page = state.pages.get(&pid).expect("pending page is dirty");
            let bytes = bincode::serialize(page)?;
            let off = file.metadata()?.len();
            file.write_all(&bytes)?;
            index.retain(|(q, _, _)| *q != pid);
            index.push((pid, off, bytes.len() as u64));
        }
        file.sync_all()?;
        index.sort_by_key(|(p, _, _)| *p);
    }
    // Pages evicted back to pristine drop out of the index: their
    // commitment equals the pristine reconstruction, so the loader loses
    // nothing by not finding them.
    index.retain(|(p, _, _)| state.pages.contains_key(p));
    if std::fs::metadata(data_path(PAGES_LOG))
        .map(|m| m.len())
        .unwrap_or(0)
        > COMPACT_THRESHOLD_BYTES
    {
        compact_page_log(state, &mut index)?;
    }
    atomic_write(PAGES_INDEX, &bincode::serialize(&index)?)?;
    // Build and write the state header last: it lists both lock maps and
    // the balance cache, so a restart rebuilds balances without replaying
    // the chain, and it pins the emergency pause in effect at this save.
    let header = Header {
        page_ids: state.pages.keys().cloned().collect(),
        locks: state.locks.clone(),
        lock_created: state.lock_created.clone(),
        balance_cache: state.balance_cache.clone(),
        treasury_signer: state.treasury_signer,
        total_sold: state.total_sold,
        total_bought_back: state.total_bought_back,
        dev_fund_signer: state.dev_fund_signer,
        dev_fee_bps: state.dev_fee_bps,
        mine_fee_bps: state.mine_fee_bps,
        dao: state.dao.clone(),
        emergency_flags: state.emergency_flags,
        emergency_until: state.emergency_until,
    };
    atomic_write("header.bin", &bincode::serialize(&header)?)?;
    Ok(())
}

/// Load the state. The page log and its index are authoritative when the
/// index exists: every page is read by a direct seek, and an index entry
/// outside the log is a hard error, not a silent skip. Without the index
/// the loader falls back to the legacy per-page directory named by the
/// header. Missing or corrupt header files are hard errors: a state that
/// cannot be reconstructed must never be silently replaced with a
/// fabricated one.
pub fn load() -> anyhow::Result<crate::state::State> {
    let header_bytes = std::fs::read(data_path("header.bin"))?;
    let header: Header = bincode::deserialize(&header_bytes)?;
    let mut pages = BTreeMap::new();
    if let Ok(index_bytes) = std::fs::read(data_path(PAGES_INDEX)) {
        if let Ok(index) = bincode::deserialize::<Vec<(u16, u64, u64)>>(&index_bytes) {
            let log = std::fs::read(data_path(PAGES_LOG))?;
            for (pid, off, len) in index {
                let start = off as usize;
                let end = start
                    .checked_add(len as usize)
                    .filter(|end| *end <= log.len())
                    .ok_or_else(|| anyhow::anyhow!("page index entry outside the page log"))?;
                let page: crate::state::Page = bincode::deserialize(&log[start..end])?;
                pages.insert(pid, page);
            }
            return Ok(crate::state::State {
                pages,
                locks: header.locks,
                lock_created: header.lock_created,
                balance_cache: header.balance_cache,
                treasury_signer: header.treasury_signer,
                total_sold: header.total_sold,
                total_bought_back: header.total_bought_back,
                dev_fund_signer: header.dev_fund_signer,
                dev_fee_bps: header.dev_fee_bps,
                mine_fee_bps: header.mine_fee_bps,
                dao: header.dao,
                emergency_flags: header.emergency_flags,
                emergency_until: header.emergency_until,
                touched_pages: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashSet::new(),
                )),
            });
        }
    }
    for page_id in &header.page_ids {
        let path = format!("pages/{:04x}.bin", page_id);
        let bytes = std::fs::read(data_path(&path))?;
        let page: crate::state::Page = bincode::deserialize(&bytes)?;
        pages.insert(*page_id, page);
    }
    Ok(crate::state::State {
        pages,
        locks: header.locks,
        lock_created: header.lock_created,
        balance_cache: header.balance_cache,
        treasury_signer: header.treasury_signer,
        total_sold: header.total_sold,
        total_bought_back: header.total_bought_back,
        dev_fund_signer: header.dev_fund_signer,
        dev_fee_bps: header.dev_fee_bps,
        mine_fee_bps: header.mine_fee_bps,
        dao: header.dao,
        emergency_flags: header.emergency_flags,
        emergency_until: header.emergency_until,
        touched_pages: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
    })
}
