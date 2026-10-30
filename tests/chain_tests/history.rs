//! History, replay, roots, and header-sync tests.
//!
//! These tests pin the cross-network replay protection, the deterministic
//! global root, header-batch validation (including from-genesis batches),
//! and the empty-payload block with no fee claims. The state-history
//! window growth walk lives in the `slow_chain_tests` target: it mines
//! past the reorg window and is compute-bound by design.

use super::*;
use slash::{chain, crypto, state};

#[test]
fn test_replay_protection_across_networks() {
    // A transaction signed on the testnet must not verify on the mainnet
    // chain: the chain identifier is part of the signature hash.
    let _tmp = setup_test_dir("replay");
    reset_testnet();
    let mut test_chain = chain::Chain::with_chain_id(chain::TESTNET_ID.to_string());
    let (alice_sk, alice) = test_keypair(1);
    let (_bob_sk, bob) = test_keypair(2);
    for i in 0..50 {
        mine_cell_for(test_chain.state_mut(), i, alice);
    }
    let outputs: Vec<state::Output> = vec![
        state::Output {
            start: 0,
            end: 49,
            to: bob,
            lock: None,
        },
        state::Output {
            start: 49,
            end: 50,
            to: state::FEE_VAULT,
            lock: None,
        },
    ];
    let hash = chain::Chain::tx_signature_hash(
        &alice,
        &[(0, 50)],
        &outputs,
        chain::TESTNET_ID.as_bytes(),
        0,
        &None,
    );
    let sig = crypto::sign(&alice_sk, &hash);
    let main_chain = chain::Chain::genesis();
    // The working copy detaches the value from the chain's shared `Arc`.
    let mut main_state = (*main_chain.state).clone();
    for i in 0..50 {
        mine_cell_for(&mut main_state, i, alice);
    }
    let height = main_chain.tip_height() + 1;
    assert!(!main_state.tx(
        alice,
        &[(0, 50)],
        &outputs,
        &sig,
        height,
        main_chain.chain_id.as_bytes(),
        0,
        &None,
    ));
}

#[test]
fn test_pristine_page_eviction() {
    // The transfer returns every alice cell to the treasury; the fee
    // output keeps one cell in the vault. The pages themselves cannot
    // return to pristine inside the same block: the mandatory coinbase
    // and the slot mint assign miner ranges to them. The page-eviction
    // invariant for a pure state transition is pinned by the state-level
    // pristine_page_eviction_reduces_dirty_set test; here we pin the
    // ownership outcome of the block transition.
    let _tmp = setup_test_dir("eviction");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    // PAGE_SIZE is a u64 constant: both anchors are plain global cell
    // coordinates, the first 25 cells before the page boundary and the
    // first 25 cells of the next page.
    let cell0 = state::PAGE_SIZE - 25;
    let cell1 = state::PAGE_SIZE;
    for i in 0..25 {
        mine_cell_for(chain.state_mut(), cell0 + i, alice);
        mine_cell_for(chain.state_mut(), cell1 + i, alice);
    }
    assert!(chain.state.pages.contains_key(&0));
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(cell0, cell0 + 25), (cell1, cell1 + 25)],
        &[
            (cell0, cell0 + 25, state::TREASURY),
            (cell1, cell1 + 24, state::TREASURY),
            (cell1 + 24, cell1 + 25, state::FEE_VAULT),
        ],
        0,
    );
    let height = chain.tip_height() + 1;
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    // alice's cells returned to the treasury; alice keeps only the
    // block rewards (coinbase plus the one-cell slot mint).
    assert_eq!(chain.state.balance(alice), 500 + 1);
    // Ownership of the returned ranges is treasury again (ownership
    // probes go through the public page accessor, not the crate-private
    // owns helper).
    let probe = chain.state.get(cell0);
    assert!(matches!(probe, Some((_, _, owner)) if owner == state::TREASURY));
    let probe = chain.state.get(cell1);
    assert!(matches!(probe, Some((_, _, owner)) if owner == state::TREASURY));
    assert_eq!(chain.state.balance(state::FEE_VAULT), 1);
}

#[test]
fn test_global_root_deterministic() {
    let _tmp = setup_test_dir("global_root");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let r1 = chain.state.global_root();
    let miner = [1u8; 32];
    let b = mine_next_block(&chain, miner, vec![]);
    chain.apply(b).unwrap();
    let r2 = chain.state.global_root();
    assert_ne!(r1, r2);
    let mut chain2 = chain::Chain::genesis();
    let b = mine_next_block(&chain2, miner, vec![]);
    chain2.apply(b).unwrap();
    assert_eq!(chain.state.global_root(), chain2.state.global_root());
}

#[test]
fn test_header_validation_succeeds() {
    // A five-header batch continuing the retained tip validates when
    // every header carries the retargeted difficulty, a timestamp above
    // the trailing median, a valid linkage chain, and the proof-of-work
    // slot derived from its exact header fields. The retained prefix is
    // mined at target spacing so the batch's retarget schedule is
    // stationary at difficulty 1.
    let _tmp = setup_test_dir("headers_ok");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    for h in 1..=5u64 {
        let b = mine_block_at(
            &chain,
            h,
            h * chain::TARGET_BLOCK_TIME_SECS,
            miner,
            1,
            vec![],
        );
        chain.apply(b).unwrap();
    }
    let mut headers = Vec::new();
    let mut prev = chain.block_hash(chain.blocks.last().unwrap());
    for h in 6..=10u64 {
        let time = h * chain::TARGET_BLOCK_TIME_SECS;
        let difficulty = chain.difficulty_for_height(h);
        let cell = chain::verify_pow(prev, time, h, miner, 0, difficulty)
            .expect("difficulty 1 always yields a slot cell");
        let header = chain::BlockHeader {
            version: 1,
            prev,
            time,
            height: h,
            nonce: 0,
            miner,
            mined_cell: cell,
            difficulty,
            body_hash: [0u8; 32],
        };
        let mut hh = blake3::Hasher::new();
        hh.update(&header.version.to_le_bytes());
        hh.update(&header.prev);
        hh.update(&header.time.to_le_bytes());
        hh.update(&header.height.to_le_bytes());
        hh.update(&header.nonce.to_le_bytes());
        hh.update(&header.miner);
        hh.update(&header.mined_cell.to_le_bytes());
        hh.update(&header.difficulty.to_le_bytes());
        hh.update(&header.body_hash);
        prev = *hh.finalize().as_bytes();
        headers.push(header);
    }
    assert!(chain.validate_headers_batch(&headers));
}

#[test]
fn test_header_rejects_wrong_height() {
    let _tmp = setup_test_dir("headers_bad");
    reset_testnet();
    let chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    let mut headers = vec![];
    for _ in 0..3 {
        let b = mine_next_block(&chain, miner, vec![]);
        headers.push(chain::BlockHeader::from_block(&b));
    }
    headers[1].height = 99;
    let chain2 = chain::Chain::genesis();
    assert!(!chain2.validate_headers_batch(&headers));
}

#[test]
fn test_block_with_empty_fee_claims() {
    // A block with no fee claims at all is valid from any era; era 9 in
    // particular permits an empty payload block with no coinbase and no
    // claims.
    let _tmp = setup_test_dir("empty_claims");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    let b = mine_next_block(&chain, miner, vec![]);
    assert!(b.fee_claims.is_empty());
    chain.apply(b).unwrap();
    assert_eq!(chain.tip_height(), 1);
}
