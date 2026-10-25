//! The manifest: the atomic chain snapshot, published last so readers
//! always see a consistent triple. Every referenced file carries a
//! content stamp verified before deserialization, so a swapped or
//! corrupted file never loads silently. The block half is an append-only
//! log: each save writes only the blocks mined since the previous save,
//! so persistence cost tracks new work instead of the whole chain.
//!
//! The append-only block log — framing, appending, reading — lives in
//! the `block_log` child module; this file keeps the snapshot format,
//! the stamps, the save/load orchestration, and the legacy-layout
//! compatibility shims.

use super::{save as save_state, ChainMeta, FileStamp, Header, Manifest};
use std::collections::BTreeMap;

#[path = "manifest_log.rs"]
mod block_log;

use block_log::{append_blocks_log, decode_blocks_log, read_blocks_log, read_blocks_meta};

/// Stable on-disk filenames of the snapshot triple and its sidecars.
pub(crate) const BLOCKS_PATH: &str = "blocks.log";
const LEGACY_BLOCKS_PATH: &str = "chain_blocks.bin";
const STATE_PATH: &str = "header.bin";
const META_PATH: &str = "chain_meta.bin";
const MANIFEST_PATH: &str = "manifest.bin";

/// Stamp freshly serialized bytes: length plus a BLAKE3 digest.
fn stamp_bytes(bytes: &[u8]) -> FileStamp {
    FileStamp {
        len: bytes.len() as u64,
        digest: *blake3::hash(bytes).as_bytes(),
    }
}

/// Stamp a file already on disk; a read failure stamps a zero length,
/// which the load-side check then rejects.
fn stamp_path(path: &str) -> FileStamp {
    std::fs::read(super::data_path(path))
        .map(|b| stamp_bytes(&b))
        .unwrap_or(FileStamp {
            len: 0,
            digest: [0u8; 32],
        })
}

/// Verify one data file against its recorded stamp before any
/// deserialization: a length mismatch avoids hashing, a digest mismatch
/// means the file changed after the snapshot was published, and either
/// aborts the load instead of restoring a mixed triple.
fn verify_stamp(path: &str, file_stamp: &FileStamp) -> anyhow::Result<Vec<u8>> {
    let bytes = std::fs::read(super::data_path(path))?;
    if bytes.len() as u64 != file_stamp.len {
        anyhow::bail!(
            "{path}: length {} does not match the manifest stamp {}",
            bytes.len(),
            file_stamp.len
        );
    }
    if *blake3::hash(&bytes).as_bytes() != file_stamp.digest {
        anyhow::bail!("{path}: content digest does not match the manifest stamp");
    }
    Ok(bytes)
}

/// Atomically persist a complete chain snapshot: block tail, state, and
/// metadata first; the manifest is published last so a crash leaves a
/// consistent triple. Every published file is stamped, so the manifest is
/// a complete integrity record.
pub fn save_manifest(
    blocks: &[crate::chain::Block],
    state: &crate::state::State,
    base_height: u64,
    checkpoints: &BTreeMap<u64, [u8; 32]>,
    deployments: &[crate::chain::Deployment],
    chain_id: &[u8],
) -> anyhow::Result<()> {
    // The state half: pages + header.
    save_state(state)?;

    // Persist the block list as an append-only log: only the new tail.
    append_blocks_log(blocks)?;

    // Persist chain metadata so a restart cannot roll back soft-fork
    // gates or replay domains.
    let encoded_meta = bincode::serialize(&ChainMeta {
        base_height,
        checkpoints: checkpoints.clone(),
        deployments: deployments.to_vec(),
        chain_id: chain_id.to_vec(),
    })?;
    super::atomic_write(META_PATH, &encoded_meta)?;

    // Publish the manifest last: a reader that sees it is guaranteed the
    // referenced files are complete.
    let manifest = Manifest {
        blocks_path: BLOCKS_PATH.to_string(),
        state_header_path: STATE_PATH.to_string(),
        chain_meta_path: META_PATH.to_string(),
        blocks_stamp: stamp_path(BLOCKS_PATH),
        state_stamp: stamp_path(STATE_PATH),
        meta_stamp: stamp_bytes(&encoded_meta),
    };
    super::atomic_write(MANIFEST_PATH, &bincode::serialize(&manifest)?)?;
    Ok(())
}

/// The pre-manifest fallback: legacy fixed filenames, no verification.
fn legacy_triple() -> anyhow::Result<(Vec<crate::chain::Block>, crate::state::State, ChainMeta)> {
    Ok((
        load_blocks()?,
        super::state::load()?,
        load_chain_meta_full()?,
    ))
}

/// Load the full chain record from disk. A manifest that parses is
/// authoritative: every referenced file is verified against its stamp
/// before deserialization. A manifest that does not parse falls back to
/// the legacy fixed filenames without verification.
pub fn load_from_manifest_full(
) -> anyhow::Result<(Vec<crate::chain::Block>, crate::state::State, ChainMeta)> {
    let Ok(bytes) = std::fs::read(super::data_path(MANIFEST_PATH)) else {
        return legacy_triple();
    };
    let Ok(manifest) = bincode::deserialize::<Manifest>(&bytes) else {
        return legacy_triple();
    };

    let blocks_bytes = verify_stamp(&manifest.blocks_path, &manifest.blocks_stamp)?;
    let blocks = decode_blocks_log(&blocks_bytes, read_blocks_meta().map(|(_, c)| c as usize))?;

    let header_bytes = verify_stamp(&manifest.state_header_path, &manifest.state_stamp)?;
    let header: Header = bincode::deserialize(&header_bytes)?;

    let mut pages = BTreeMap::new();
    for page_id in &header.page_ids {
        let path = format!("pages/{:04x}.bin", page_id);
        let bytes = std::fs::read(super::data_path(&path))?;
        let page: crate::state::Page = bincode::deserialize(&bytes)?;
        pages.insert(*page_id, page);
    }

    let state = crate::state::State {
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
    };

    let meta_bytes = verify_stamp(&manifest.chain_meta_path, &manifest.meta_stamp)?;
    let meta: ChainMeta = bincode::deserialize(&meta_bytes)?;

    Ok((blocks, state, meta))
}

/// Backward-compatible view returning only the base height; prefer the
/// full variant.
pub fn load_from_manifest() -> anyhow::Result<(Vec<crate::chain::Block>, crate::state::State, u64)>
{
    let (blocks, state, meta) = load_from_manifest_full()?;
    Ok((blocks, state, meta.base_height))
}

/// Persist a block list to the legacy full-file layout.
pub fn save_blocks(blocks: &[crate::chain::Block]) -> anyhow::Result<()> {
    let encoded = bincode::serialize(blocks)?;
    Ok(super::atomic_write(LEGACY_BLOCKS_PATH, &encoded)?)
}

/// Load a block list: the append-only log when its meta sidecar exists,
/// otherwise the legacy full file.
pub fn load_blocks() -> std::result::Result<Vec<crate::chain::Block>, anyhow::Error> {
    if read_blocks_meta().is_some() {
        if let Ok(blocks) = read_blocks_log() {
            return Ok(blocks);
        }
    }
    let bytes = std::fs::read(super::data_path(LEGACY_BLOCKS_PATH))?;
    Ok(bincode::deserialize(&bytes)?)
}

/// Persist the full chain metadata record atomically.
pub fn save_chain_meta_full(meta: &ChainMeta) -> anyhow::Result<()> {
    Ok(super::atomic_write(META_PATH, &bincode::serialize(meta)?)?)
}

/// Load the stored chain metadata; a missing file reads as no metadata.
pub fn load_chain_meta_full() -> anyhow::Result<ChainMeta> {
    let bytes = std::fs::read(super::data_path(META_PATH))?;
    Ok(bincode::deserialize(&bytes)?)
}

/// Save the current base height for pruned nodes.
pub fn save_chain_meta(base_height: u64) -> anyhow::Result<()> {
    save_chain_meta_full(&ChainMeta {
        base_height,
        ..ChainMeta::default()
    })
}

/// Load the stored base height.
pub fn load_chain_meta() -> anyhow::Result<u64> {
    Ok(load_chain_meta_full()?.base_height)
}

/// Write bytes atomically: temp name, fsync, rename, fsync the parent;
/// a crash leaves either the previous or the next content. The target
/// resolves against the library data directory.
pub fn atomic_write(path: &str, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let target = super::data_path(path);
    let tmp = target.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &target)?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}
