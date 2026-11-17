//! The router tests for the JSON-RPC surface. The module is wired into
//! `rpc/mod.rs` through a path attribute, so it drives the private
//! `RpcShared` bucket logic and the dispatch table exactly as an inline
//! module would.

use super::*;

/// A request envelope parses into the id, method, and params the
/// router reads; unparsable bodies carry the parse-error code.
#[test]
fn envelope_parse_and_method_extraction() {
    let request: Value =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"getTip","params":[],"id":1}"#)
            .expect("a well-formed envelope parses");
    assert_eq!(request.get("id").cloned().unwrap_or(Value::Null), json!(1));
    assert_eq!(
        request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "getTip"
    );
    let bad: Result<Value, _> = serde_json::from_str("{not json");
    assert!(
        bad.is_err(),
        "an unparsable body is rejected before routing"
    );
}

#[tokio::test]
async fn dispatch_routes_results_and_unknown_methods() {
    let shared = RpcShared::new(None);
    let params = serde_json::from_str("[]").unwrap();
    let result = dispatch(&shared, "getTip", &params).await;
    assert!(result.is_ok(), "getTip resolves through the chain cache");
    let unknown = dispatch(&shared, "nope", &params).await;
    match unknown {
        Err(err) => assert_eq!(err.code, -32601),
        Ok(_) => panic!("an unknown method must not resolve"),
    }
}

#[tokio::test]
async fn control_methods_report_unconfigured_channel() {
    // Without an installed command channel the control methods answer
    // with a plain internal error, never with a false success. The
    // loopback default carries no token, so authorization passes and the
    // refusal comes from the missing channel.
    let shared = RpcShared::new(None);
    let params = serde_json::from_str("[]").unwrap();
    match dispatch(&shared, "stop", &params).await {
        Err(err) => assert_eq!(err.code, -32603),
        Ok(_) => panic!("stop without a channel must fail closed"),
    }
}

#[tokio::test]
async fn control_methods_require_the_token_on_external_config() {
    // With a control token installed, an unauthenticated call is
    // refused with the dedicated authorization code before it reaches
    // the command channel, and the matching token passes authorization
    // (the refusal then comes from the unconfigured channel, proving the
    // call crossed the gate).
    let shared = RpcShared::new(None).with_control_token(Some("sekrit".to_string()));
    let params = serde_json::from_str("[]").unwrap();
    match dispatch(&shared, "stop", &params).await {
        Err(err) => assert_eq!(err.code, -32001),
        Ok(_) => panic!("stop without the token must be refused"),
    }
    let wrong = serde_json::from_str(r#"[{"token":"wrong"}]"#).unwrap();
    match dispatch(&shared, "mine", &wrong).await {
        Err(err) => assert_eq!(err.code, -32001),
        Ok(_) => panic!("a wrong token must be refused"),
    }
    let right = serde_json::from_str(r#"[{"token":"sekrit"}]"#).unwrap();
    match dispatch(&shared, "stop", &right).await {
        Err(err) => assert_eq!(err.code, -32603),
        Ok(_) => panic!("authorized control without a channel still fails closed"),
    }
    let bare = serde_json::from_str(r#"["sekrit"]"#).unwrap();
    match dispatch(&shared, "stop", &bare).await {
        Err(err) => assert_eq!(err.code, -32603),
        Ok(_) => panic!("the bare-string token form must authorize"),
    }
}

#[test]
fn rate_limit_allows_up_to_the_burst_cap_then_refuses() {
    // A fresh bucket holds exactly BUCKET_CAPACITY tokens: that many
    // back-to-back requests are admitted, the next one is refused.
    // The calls run within one refill quantum, so the linear refill
    // does not smuggle an extra token in.
    let shared = RpcShared::new(None);
    let ip = "10.0.0.1";
    for _ in 0..BUCKET_CAPACITY as usize {
        assert!(shared.rate_limit(ip));
    }
    assert!(!shared.rate_limit(ip));
}

#[test]
fn sweep_reclaims_idle_entries_past_the_watermark() {
    // Fill the map with strictly more distinct entries than the
    // watermark, every one idle beyond the horizon: the next
    // admission crosses the watermark, the sweep reclaims all idle
    // entries, and the map ends holding exactly the one active entry
    // created by that admission.
    let shared = RpcShared::new(None);
    {
        let mut buckets = shared.buckets.lock().unwrap();
        for i in 0..=BUCKET_SWEEP_WATERMARK {
            buckets.insert(
                format!("10.1.0.{i}"),
                (
                    BUCKET_CAPACITY,
                    Instant::now() - BUCKET_IDLE_HORIZON - Duration::from_secs(1),
                ),
            );
        }
    }
    assert!(shared.rate_limit("10.0.0.1"));
    assert_eq!(
        shared.buckets.lock().unwrap().len(),
        1,
        "the sweep reclaims every idle entry and keeps only the active one"
    );
}

#[test]
fn poisoned_bucket_lock_still_serves() {
    // A panicking holder leaves the lock poisoned; the rate limiter
    // recovers the guard and keeps admitting requests instead of
    // failing every later call for the process lifetime.
    let shared = RpcShared::new(None);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = shared.buckets.lock().unwrap();
        panic!("simulated panic while holding the bucket lock");
    }));
    assert!(shared.rate_limit("10.0.0.2"));
}
