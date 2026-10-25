//! Payload builders of the network soak: plain transfers between nodes
//! and treasury buy pairs, plus the randomized payload mixer the driver
//! calls once per operation.

use super::*;

/// Build a plain transfer of `amount` cells from `node` to `recipient`,
/// paying the mandatory fee into the shared fee vault. Returns `None`
/// when the node cannot fund the amount plus the fee.
pub fn build_transfer(
    node: &Node,
    recipient: [u8; 32],
    amount: u64,
) -> Option<Vec<slash::chain::Tx>> {
    let fee = amount / slash::state::MIN_FEE_DENOMINATOR.max(1);
    let total = amount.checked_add(fee)?;
    let (s, e) = node.chain.state.select_single(node.miner_pk, total)?;
    let outputs = vec![
        slash::state::Output {
            start: s,
            end: s + amount,
            to: recipient,
            lock: None,
        },
        slash::state::Output {
            start: s + amount,
            end: e,
            to: slash::state::FEE_VAULT,
            lock: None,
        },
    ];
    Some(vec![make_tx(
        node.miner_pk,
        &node.miner_sk,
        &[(s, e)],
        &outputs,
        node.chain.chain_id.as_bytes(),
    )])
}

/// Build an atomic treasury buy pair — a buyer payment into the treasury
/// vault plus the treasury-signer mint — at the current curve price.
/// Returns `None` when the node cannot fund the payment.
pub fn build_buy_pair(node: &Node, amount: u64) -> Option<Vec<slash::chain::Tx>> {
    let cost = slash::treasury::buy_price(amount, node.chain.state.total_sold);
    let (s, e) = node.chain.state.select_single(node.miner_pk, cost)?;
    let pay_outputs = vec![slash::state::Output {
        start: s,
        end: e,
        to: slash::state::TREASURY_VAULT,
        lock: None,
    }];
    let pay = make_tx(
        node.miner_pk,
        &node.miner_sk,
        &[(s, e)],
        &pay_outputs,
        node.chain.chain_id.as_bytes(),
    );
    let (ms, me) = node
        .chain
        .state
        .select_single(slash::state::TREASURY, amount)?;
    let mint_outputs = vec![slash::state::Output {
        start: ms,
        end: me,
        to: node.miner_pk,
        lock: None,
    }];
    let mint = make_tx(
        slash::state::TREASURY,
        &TREASURY_SK,
        &[(ms, me)],
        &mint_outputs,
        node.chain.chain_id.as_bytes(),
    );
    Some(vec![pay, mint])
}

/// Compose one random operation payload: with equal probability a plain
/// transfer to a random peer or a treasury buy pair.
pub fn random_payload(rng: &mut Rng, nodes: &[Node], miner: usize) -> Vec<slash::chain::Tx> {
    let node = &nodes[miner];
    let amount = 50 + rng.below(200);
    if rng.below(2) == 0 {
        let recipient = nodes[rng.below(nodes.len() as u64) as usize].miner_pk;
        build_transfer(node, recipient, amount).unwrap_or_default()
    } else {
        build_buy_pair(node, amount).unwrap_or_default()
    }
}
