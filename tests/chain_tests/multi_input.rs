//! Multi-input transaction structure tests.
//!
//! These tests pin per-page balance, exact output packing, double-spend
//! rejection inside a single block, and cross-page transfers. Outputs
//! pack the inputs exactly in declaration order: the output cursor must
//! reach the end of the last input, so a multi-range input set is valid
//! only when its ranges are adjacent.

use super::*;
use slash::{chain, state};

#[test]
fn test_double_spend_same_block_rejected() {
    let _tmp = setup_test_dir("double_spend");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    for i in 0..50 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx1 = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50)],
        &[(0, 49, [9u8; 32]), (49, 50, state::FEE_VAULT)],
        0,
    );
    let tx2 = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50)],
        &[(0, 49, [8u8; 32]), (49, 50, state::FEE_VAULT)],
        0,
    );
    let b = mine_block_at_unchecked(&chain, height, 1, alice, 1, vec![tx1, tx2]);
    assert!(chain.apply(b).is_err());
}

#[test]
fn test_output_gap_rejected() {
    let _tmp = setup_test_dir("gap");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    // Outputs must pack the inputs exactly: the gap between 40 and 41
    // leaves input cell 40 unclaimed.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50), (50, 100)],
        &[
            (0, 40, [9u8; 32]),
            (41, 99, alice),
            (99, 100, state::FEE_VAULT),
        ],
        0,
    );
    let b = mine_block_at_unchecked(&chain, height, 1, alice, 1, vec![tx]);
    assert!(chain.apply(b).is_err());
}

#[test]
fn test_unbalanced_pages_rejected() {
    let _tmp = setup_test_dir("unbalanced");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    for i in 0..50 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    // The declared outputs total 40 cells against 50 cells of inputs.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 25), (25, 50)],
        &[(0, 39, [9u8; 32]), (39, 40, state::FEE_VAULT)],
        0,
    );
    let b = mine_block_at_unchecked(&chain, height, 1, alice, 1, vec![tx]);
    assert!(chain.apply(b).is_err());
}

#[test]
fn test_cross_page_transfer() {
    let _tmp = setup_test_dir("cross_page");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    let cell0 = state::PAGE_SIZE - 25;
    let cell1 = state::PAGE_SIZE;
    for i in 0..25 {
        mine_cell_for(chain.state_mut(), cell0 + i, alice);
        mine_cell_for(chain.state_mut(), cell1 + i, alice);
    }
    let height = chain.tip_height() + 1;
    // The two inputs are adjacent (cell0 + 25 == cell1), so the outputs
    // must run contiguously from cell0 across the page boundary: packing
    // is a single global declaration order, not a per-page arrangement.
    // The transfer moves 50 cells in total: the minimum transaction size
    // bounds the combined output width, and the one-cell fee satisfies
    // the mandatory fee at this size.
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(cell0, cell0 + 25), (cell1, cell1 + 25)],
        &[
            (cell0, cell0 + 25, [9u8; 32]),
            (cell0 + 25, cell0 + 49, alice),
            (cell0 + 49, cell0 + 50, state::FEE_VAULT),
        ],
        0,
    );
    let b = mine_block_at(&chain, height, 1, alice, 1, vec![tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance([9u8; 32]), 25);
}

#[test]
fn test_consolidate_disjoint_ranges() {
    // "Disjoint" inputs are impossible under exact packing (the output
    // cursor cannot cross a gap no input covers), so this test pins the
    // achievable form: two adjacent ranges consolidated by one
    // transaction. The miner is a third party so the assertion reads the
    // consolidation outcome alone.
    let _tmp = setup_test_dir("consolidate");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let (alice_sk, alice) = test_keypair(1);
    let miner = test_keypair(7).1;
    for i in 0..100 {
        mine_cell_for(chain.state_mut(), i, alice);
    }
    let height = chain.tip_height() + 1;
    let tx = make_tx_for(
        &chain,
        alice,
        &alice_sk,
        &[(0, 50), (50, 100)],
        &[(0, 99, alice), (99, 100, state::FEE_VAULT)],
        0,
    );
    let b = mine_block_at(&chain, height, 1, miner, 1, vec![tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(alice), 99);
}
