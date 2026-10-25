//! src/rpc/quotes.rs — the treasury bonding-curve quote endpoints.
//!
//! Both quotes are evaluated at the chain's current curve position
//! (`total_sold`) and report that position explicitly, so the treasury
//! signer can re-verify the price against the tip at signing time rather
//! than trusting a stale quote. Inclusion-time repricing is expected:
//! every pair that lands earlier in a block advances the position the
//! later pairs price against. To make the window visible, each response
//! also reports the volume of treasury payloads already queued in the
//! pending buffer, from which the signer derives the worst-case position
//! the curve can reach before this pair executes.

use super::{decode_address, RpcError};
use serde_json::{json, Value};

/// Sum the queued treasury volumes in the pending buffer: mint payloads
/// (treasury-signed sends) advance the curve position when included, and
/// sells (transfers paying the treasury) lower it. Returns (mint volume,
/// sell volume); a poisoned or absent buffer yields zeros, which only
/// makes the quote conservatively empty, never wrong.
fn pending_volumes() -> (u64, u64) {
    let Ok(buffer) = crate::PENDING_TXS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
    else {
        return (0, 0);
    };
    let mut mints = 0u64;
    let mut sells = 0u64;
    for tx in buffer.iter() {
        for out in &tx.outputs {
            let width = out.end.saturating_sub(out.start);
            if tx.from == crate::state::TREASURY {
                // A queued mint advances total_sold by its minted amount.
                mints = mints.saturating_add(width);
            }
            if out.to == crate::state::TREASURY {
                // A queued sell returns cells to the treasury and lowers
                // the position the next pairs price against.
                sells = sells.saturating_add(width);
            }
        }
    }
    (mints, sells)
}

/// buildTreasuryBuy: price a purchase and return the unsigned mint
/// transaction the treasury signer must sign. The cost is the exact curve
/// price at the current position; the response reports the position, the
/// queued mint volume ahead of this pair, and the worst-case position the
/// curve reaches if every queued mint lands first — the price the signer
/// should size against. The mint funds itself through the canonical
/// treasury selection so every node reproduces identical ranges and page
/// roots.
pub(crate) async fn handle_build_treasury_buy(params: &Value) -> Result<Value, RpcError> {
    let buyer = decode_address(params, 0)?;
    let amount = params
        .get(1)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing or invalid amount parameter"))?;
    if amount < crate::state::MIN {
        return Err(RpcError::invalid_params(
            "amount is below the minimum transaction size",
        ));
    }
    if amount > crate::state::N {
        return Err(RpcError::invalid_params(
            "amount exceeds the total cell supply",
        ));
    }
    let chain = crate::cached_load();
    let cost = crate::treasury::buy_price(amount, chain.state.total_sold);
    let mint_inputs = chain
        .state
        .select_single(crate::state::TREASURY, amount)
        .ok_or_else(|| RpcError::invalid_tx("treasury cannot fund this mint"))?;
    let mint_tx = crate::chain::Tx {
        from: crate::state::TREASURY,
        inputs: vec![mint_inputs],
        outputs: vec![crate::chain::Output {
            start: mint_inputs.0,
            end: mint_inputs.1,
            to: buyer,
            lock: None,
        }],
        sig: vec![],
        scheme: 0,
        // The memo stays empty: treasury pairs ride the pairing rule, not
        // the governance payload channel.
        memo: None,
    };
    let encoded_mint = bincode::serialize(&mint_tx)
        .map_err(|_| RpcError::invalid_params("mint transaction serialization failed"))?;
    let (queued_mints, _) = pending_volumes();
    let worst_position = chain.state.total_sold.saturating_add(queued_mints);
    Ok(json!({
        "cost": cost,
        "price_position": chain.state.total_sold,
        "queued_mint_volume": queued_mints,
        "worst_case_position": worst_position,
        "unsigned_mint_tx": hex::encode(encoded_mint),
    }))
}

/// buildTreasurySell: price a sell-back and return the unsigned payout
/// transaction that the treasury signer must sign, paying 98% of the curve
/// price out of the treasury vault. Amounts whose payout rounds to zero
/// are refused with a named error instead of an unpayable quote. The
/// quoted payout is evaluated at the chain's current curve position and
/// reported as `price_position`; queued sells ahead of this pair lower the
/// position the payout ultimately prices against, so the response reports
/// the queued sell volume and the worst-case (lowest) position the curve
/// can reach before this pair executes.
pub(crate) async fn handle_build_treasury_sell(params: &Value) -> Result<Value, RpcError> {
    let seller = decode_address(params, 0)?;
    let amount = params
        .get(1)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing or invalid amount parameter"))?;
    if amount == 0 {
        return Err(RpcError::invalid_params("amount must be positive"));
    }
    if amount > crate::state::N {
        return Err(RpcError::invalid_params(
            "amount exceeds the total cell supply",
        ));
    }
    let chain = crate::cached_load();
    let payout = crate::treasury::sell_price(amount, chain.state.total_sold);
    if payout == 0 {
        return Err(RpcError::invalid_tx("payout rounds to zero"));
    }
    let (start, end) = chain
        .state
        .select_single(crate::state::TREASURY_VAULT, payout)
        .ok_or_else(|| RpcError::invalid_tx("treasury vault cannot fund this payout"))?;
    let payout_tx = crate::chain::Tx {
        from: crate::state::TREASURY,
        inputs: vec![(start, end)],
        outputs: vec![crate::chain::Output {
            start,
            end,
            to: seller,
            lock: None,
        }],
        sig: vec![],
        scheme: 0,
        // The memo stays empty: treasury pairs ride the pairing rule, not
        // the governance payload channel.
        memo: None,
    };
    let encoded = bincode::serialize(&payout_tx)
        .map_err(|_| RpcError::invalid_params("payout transaction serialization failed"))?;
    let (_, queued_sells) = pending_volumes();
    let worst_position = chain.state.total_sold.saturating_sub(queued_sells);
    Ok(json!({
        "payout": payout,
        "price_position": chain.state.total_sold,
        "queued_sell_volume": queued_sells,
        "worst_case_position": worst_position,
        "unsigned_payout_tx": hex::encode(encoded),
    }))
}
