//! The append-only block log: the block half of the manifest snapshot.
//!
//! Each save appends only the blocks mined since the previous save, so
//! persistence cost tracks new work instead of the whole chain. Records
//! are length-prefixed bincode frames; the `blocks.meta` sidecar
//! publishes the persisted window, and a window mismatch rebuilds the
//! log once from the in-memory list. A torn tail surfaces as a framing
//! error, not a parse panic, and a framing error is a hard error — a
//! partial block list is worse than none.

use super::{atomic_write, BLOCKS_PATH};
use crate::storage::data_path;

/// (first_height, count) of the persisted block window.
pub(crate) const BLOCKS_META_PATH: &str = "blocks.meta";

/// Read the block-log meta when it exists and parses.
pub(crate) fn read_blocks_meta() -> Option<(u64, u64)> {
    std::fs::read(data_path(BLOCKS_META_PATH))
        .ok()
        .and_then(|bytes| bincode::deserialize(&bytes).ok())
}

/// Append the new tail of the block list to the append-only block log.
/// When the log is absent or the persisted window no longer matches the
/// in-memory list, the log is rebuilt once from the current list —
/// bounded by the retained window. Records are length-prefixed, so a
/// torn tail surfaces as a framing error, not a parse panic.
pub(crate) fn append_blocks_log(blocks: &[crate::chain::Block]) -> anyhow::Result<()> {
    use std::io::Write;
    let rebuild = match (read_blocks_meta(), blocks.first()) {
        (Some((first, count)), Some(head)) => first != head.height || count as usize > blocks.len(),
        (Some(_), None) => true,
        (None, _) => true,
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_path(BLOCKS_PATH))?;
    if rebuild {
        // Truncate-by-rename; the meta sidecar republishes the window.
        drop(file);
        std::fs::File::create(data_path("blocks.log.tmp"))?;
        std::fs::rename(data_path("blocks.log.tmp"), data_path(BLOCKS_PATH))?;
        file = std::fs::OpenOptions::new()
            .append(true)
            .open(data_path(BLOCKS_PATH))?;
    }
    let start = read_blocks_meta()
        .filter(|_| !rebuild)
        .map(|(_, c)| c as usize)
        .unwrap_or(0);
    for block in &blocks[start..] {
        let record = bincode::serialize(block)?;
        file.write_all(&(record.len() as u32).to_le_bytes())?;
        file.write_all(&record)?;
    }
    file.sync_all()?;
    let first = blocks.first().map(|b| b.height).unwrap_or(0);
    atomic_write(
        BLOCKS_META_PATH,
        &bincode::serialize(&(first, blocks.len()))?,
    )?;
    Ok(())
}

/// Decode a framed block log: `count` length-prefixed bincode records.
/// A framing error is a hard error: a partial block list is worse than
/// none.
pub(crate) fn decode_blocks_log(
    bytes: &[u8],
    count: Option<usize>,
) -> anyhow::Result<Vec<crate::chain::Block>> {
    let limit = count.unwrap_or(usize::MAX);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while out.len() < limit && cursor + 4 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if cursor + len > bytes.len() {
            anyhow::bail!("block log truncated inside a record");
        }
        out.push(bincode::deserialize(&bytes[cursor..cursor + len])?);
        cursor += len;
    }
    Ok(out)
}

/// Read every block back from the append-only log.
pub(crate) fn read_blocks_log() -> anyhow::Result<Vec<crate::chain::Block>> {
    let bytes = std::fs::read(data_path(BLOCKS_PATH))?;
    let count = read_blocks_meta()
        .map(|(_, c)| c as usize)
        .ok_or_else(|| anyhow::anyhow!("block log without its meta sidecar"))?;
    decode_blocks_log(&bytes, Some(count))
}
