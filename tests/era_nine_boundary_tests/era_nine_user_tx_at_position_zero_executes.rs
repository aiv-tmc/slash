//! `era_nine_user_tx_at_position_zero_executes` — one test case of the
//! `era_nine_boundary_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn era_nine_user_tx_at_position_zero_executes() {
    // From era 9 onward the coinbase is omitted, so position 0 carries
    // an ordinary user transaction: it must execute through the full
    // pipeline — signature verification, input spend, fee collection —
    // exactly like any other transaction. The transfer is neither
    // dropped nor minted as a coinbase.
    let _tmp = setup_test_dir("era_nine_user_tx");
    let mut chain = chain_with_next_height(ERA_NINE_START);
    let alice_sk = [11u8; 32];
    let alice_pk = slash::crypto::public_from_secret(&alice_sk);
    let bob_pk = slash::crypto::public_from_secret(&[22u8; 32]);
    // Fund Alice with 200 treasury cells outside any block: era-9
    // transaction execution is under test here, not block production.
    for cell in 1000..1200 {
        chain.state.mine(cell, alice_pk, 0);
    }
    let outputs = vec![
        Output {
            start: 1000,
            end: 1180,
            to: bob_pk,
            lock: None,
        },
        Output {
            start: 1180,
            end: 1200,
            to: slash::state::FEE_VAULT,
            lock: None,
        },
    ];
    let hash = Chain::tx_signature_hash(
        &alice_pk,
        &[(1000, 1200)],
        &outputs,
        chain.chain_id.as_bytes(),
        0,
        &None,
    );
    let sig = slash::crypto::sign(&alice_sk, &hash);
    let tx = chain::Tx {
        from: alice_pk,
        inputs: vec![(1000, 1200)],
        outputs: outputs
            .iter()
            .map(|o| chain::Output {
                start: o.start,
                end: o.end,
                to: o.to,
                lock: o.lock,
            })
            .collect(),
        sig,
        scheme: 0,
        memo: None,
    };
    let b = build_pow_block(&chain, vec![tx]);
    chain
        .apply(b)
        .expect("an era-9 block with a user tx at position 0 must apply");
    assert_eq!(chain.tip_height(), ERA_NINE_START);
    // The position-0 transfer executed fully: Bob received the payment,
    // the fee vault collected the fee, and Alice's input is spent.
    assert_eq!(chain.state.balance(bob_pk), 180);
    assert_eq!(chain.state.balance(alice_pk), 0);
    assert_eq!(chain.state.balance(slash::state::FEE_VAULT), 20);
}
