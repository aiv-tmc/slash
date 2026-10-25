//! End-to-end tests for atomic storage, the crash-recovery journal,
//! snapshot lifecycle, pruned recovery, and restart equivalence.

use slash::*;

use slash::test_fixtures::*;

#[test]
fn test_storage_save_and_load_state() {
    let _tmp = setup_test_dir("storage_state");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let owner = [1u8; 32];

    chain.state.mine(100, owner, 0);
    storage::save(&chain.state).unwrap();

    let loaded = storage::load().unwrap();
    assert_eq!(loaded.balance(owner), 1);
}

#[test]
fn test_storage_save_and_load_blocks() {
    let _tmp = setup_test_dir("storage_blocks");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    for _ in 0..3 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }

    storage::save_blocks(&chain.blocks).unwrap();
    let loaded = storage::load_blocks().unwrap();

    assert_eq!(loaded.len(), chain.blocks.len());
}

#[test]
fn test_storage_chain_meta_roundtrip() {
    let _tmp = setup_test_dir("storage_meta");
    reset_testnet();

    storage::save_chain_meta(12345).unwrap();
    let loaded = storage::load_chain_meta().unwrap();
    assert_eq!(loaded, 12345);
}

#[test]
fn test_snapshot_save_and_load() {
    let _tmp = setup_test_dir("storage_snapshot");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let owner = [1u8; 32];

    chain.state.mine(50, owner, 0);
    storage::save_snapshot(&chain.state, 10).unwrap();

    let (height, loaded) = storage::load_latest_snapshot(10).unwrap();
    assert_eq!(height, 10);
    assert_eq!(loaded.balance(owner), 1);
}

#[test]
fn test_snapshot_prune_keeps_last_three() {
    let _tmp = setup_test_dir("storage_prune");
    reset_testnet();
    let state = chain::Chain::genesis().state;

    for h in 1..=6 {
        storage::save_snapshot(&state, h).unwrap();
    }

    storage::prune_snapshots().unwrap();

    let mut remaining = 0;
    if let Ok(entries) = std::fs::read_dir("snapshots") {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(".bin") {
                remaining += 1;
            }
        }
    }
    assert_eq!(remaining, 3);
}

#[test]
fn test_load_pruned_recovers_from_snapshot() {
    let _tmp = setup_test_dir("storage_load_pruned");
    reset_testnet();

    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    for _ in 0..5 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }

    let height = chain.blocks.len() as u64 + chain.base_height;
    storage::save_blocks(&chain.blocks).unwrap();
    storage::save(&chain.state).unwrap();
    storage::save_chain_meta(chain.base_height).unwrap();
    storage::save_snapshot(&chain.state, height).unwrap();

    let pruned = slash::load_pruned();
    assert_eq!(pruned.blocks.len(), chain.blocks.len());
}

#[test]
fn test_journal_replay_recovers_unsaved_blocks() {
    let _tmp = setup_test_dir("storage_journal_replay");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    // Commit the first three blocks through a full manifest save.
    for _ in 0..3 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    slash::save(&chain);
    assert!(storage::journal_load().is_empty());

    // Apply two more blocks the way the node does: journal each block as it
    // is applied, but do not flush the manifest (simulating a crash before
    // the periodic save timer fires).
    for _ in 0..2 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b.clone()).unwrap();
        storage::journal_append(&b).unwrap();
    }

    // A fresh load must reconstruct the committed tip by replaying the journal.
    let recovered = slash::load_pruned();
    assert_eq!(recovered.blocks.len(), chain.blocks.len());
    assert_eq!(recovered.tip_hash(), chain.tip_hash());
    assert_eq!(recovered.state.balance(miner), chain.state.balance(miner));

    // Once the manifest covers the replayed blocks, the journal is empty.
    slash::save(&recovered);
    assert!(storage::journal_load().is_empty());
}

#[test]
fn test_journal_tolerates_torn_trailing_record() {
    let _tmp = setup_test_dir("storage_journal_torn");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..3 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b.clone()).unwrap();
        storage::journal_append(&b).unwrap();
    }
    let full_len = std::fs::metadata("chain_journal.bin").unwrap().len();

    // Corrupt the tail to simulate a crash mid-append: every truncated
    // variant must load a clean prefix of complete records without panicking.
    for cut in 1..=full_len {
        let bytes = std::fs::read("chain_journal.bin").unwrap();
        std::fs::write("chain_journal.bin", &bytes[..(full_len - cut) as usize]).unwrap();
        let loaded = storage::journal_load();
        assert!(loaded.len() <= 3);
    }
}

#[test]
fn test_locks_pruned_after_expiry() {
    let _tmp = setup_test_dir("storage_locks_prune");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (sk, pk) = crypto::generate_keypair();
    let owner = pk;

    // Give the owner 50 cells and lock 49 of them until height 2.
    let b1 = mine_next_block(&chain, owner, vec![]);
    chain.apply(b1).unwrap();
    let outputs = vec![
        state::Output {
            start: 0,
            end: 49,
            to: owner,
            lock: Some(2),
        },
        state::Output {
            start: 49,
            end: 50,
            to: state::FEE_VAULT,
            lock: None,
        },
    ];
    let lock_tx = make_tx(owner, &sk, &[(0, 50)], &outputs, &chain.chain_id);
    let b2 = mine_next_block(&chain, state::TREASURY, vec![lock_tx]);
    chain.apply(b2).unwrap();
    assert_eq!(chain.state.locks.len(), 49);

    // Advancing the chain past the expiry and pruning empties the map.
    for _ in 0..3 {
        let b = mine_next_block(&chain, state::TREASURY, vec![]);
        chain.apply(b).unwrap();
    }
    let tip = chain.blocks.len() as u64 + chain.base_height - 1;
    chain.state.prune_locks(tip);
    assert!(chain.state.locks.is_empty());
}
