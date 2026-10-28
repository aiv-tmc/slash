//! Payload builders of the network soak: plain transfers between nodes
//! and treasury buy pairs, plus the randomized payload mixer the driver
//! calls once per operation.
//!
//! Every built transaction obeys the engine's structural rules, so a
//! payload either applies or the builder returns nothing: ranges are cut
//! at page boundaries (the engine stages every range on a single page),
//! the mandatory fee satisfies the fee gate against the total output —
//! `fee = ceil(amount / (MIN_FEE_DENOMINATOR - 1))`, the closed form of
//! `fee * DENOMINATOR >= amount + fee` — and the treasury mint takes the
//! range after the coinbase range, because the coinbase claims the
//! canonical frontier first inside the block.

use super::*;

/// Split a span at page boundaries into page-local pieces: the engine
/// stages every input and output range on the single page it belongs
/// to, so a range crossing a boundary would be refused. The pieces keep
/// declaration order, so packing stays exact.
fn split_span(s: u64, e: u64) -> Vec<(u64, u64)> {
    let page = slash::state::PAGE_SIZE;
    let mut out = Vec::new();
    let mut cur = s;
    while cur < e {
        let boundary = ((cur / page) + 1) * page;
        let end = e.min(boundary);
        out.push((cur, end));
        cur = end;
    }
    out
}

/// Split one logical output into page-local engine outputs to the same
/// recipient, preserving order.
fn split_output(s: u64, e: u64, to: [u8; 32]) -> Vec<slash::state::Output> {
    split_span(s, e)
        .into_iter()
        .map(|(a, b)| slash::state::Output {
            start: a,
            end: b,
            to,
            lock: None,
        })
        .collect()
}

/// The mandatory fee for a transfer of `amount` cells when the fee
/// output is part of the total output: the gate reads
/// `fee * MIN_FEE_DENOMINATOR >= amount + fee`, whose closed form is
/// `ceil(amount / (MIN_FEE_DENOMINATOR - 1))`.
fn mandatory_fee(amount: u64) -> u64 {
    amount.div_ceil(slash::state::MIN_FEE_DENOMINATOR.saturating_sub(1).max(1))
}

/// Build a plain transfer of `amount` cells from `node` to `recipient`,
/// paying the mandatory fee into the shared fee vault. Returns `None`
/// when the node cannot fund the amount plus the fee.
pub fn build_transfer(
    node: &Node,
    recipient: [u8; 32],
    amount: u64,
) -> Option<Vec<slash::chain::Tx>> {
    let fee = mandatory_fee(amount);
    let total = amount.checked_add(fee)?;
    let (s, e) = node.chain.state.select_single(node.miner_pk, total)?;
    let mut outputs = split_output(s, s + amount, recipient);
    outputs.extend(split_output(s + amount, e, slash::state::FEE_VAULT));
    Some(vec![make_tx(
        node.miner_pk,
        &node.miner_sk,
        &split_span(s, e),
        &outputs,
        node.chain.chain_id.as_bytes(),
    )])
}

/// Build an atomic treasury buy pair — a buyer payment into the treasury
/// vault plus the treasury-signer mint — at the current curve price.
/// Returns `None` when the node cannot fund the payment.
pub fn build_buy_pair(node: &Node, amount: u64) -> Option<Vec<slash::chain::Tx>> {
    let height = node.chain.tip_height() + 1;
    let subsidy = slash::chain::subsidy_at(height);
    let cost = slash::treasury::buy_price(amount, node.chain.state.total_sold);
    let fee = mandatory_fee(cost);
    let total = cost.checked_add(fee)?;
    let (s, e) = node.chain.state.select_single(node.miner_pk, total)?;
    let mut pay_outputs = split_output(s, s + cost, slash::state::TREASURY_VAULT);
    pay_outputs.extend(split_output(s + cost, e, slash::state::FEE_VAULT));
    let pay = make_tx(
        node.miner_pk,
        &node.miner_sk,
        &split_span(s, e),
        &pay_outputs,
        node.chain.chain_id.as_bytes(),
    );
    // The mint range follows the coinbase range: the coinbase claims the
    // canonical frontier first inside the block, so a mint selecting the
    // same frontier would find it already spent. With no subsidy there
    // is no coinbase and the mint takes the frontier itself.
    let (ms, me) = if subsidy > 0 {
        let (_, coinbase_end) = node
            .chain
            .state
            .select_single(slash::state::TREASURY, subsidy)?;
        (coinbase_end, coinbase_end + amount)
    } else {
        node.chain
            .state
            .select_single(slash::state::TREASURY, amount)?
    };
    let mint_outputs = split_output(ms, me, node.miner_pk);
    let mint = make_tx(
        slash::state::TREASURY,
        &TREASURY_SK,
        &split_span(ms, me),
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
