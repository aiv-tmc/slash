//! The token-bucket tests for the network resource limits. The module
//! is wired into `limits.rs` through a path attribute, so it drives the
//! private `RateBucket` balance through `PeerBuckets` exactly as an
//! inline module would.

use super::*;

/// The request bucket prices serving work, counts consecutive
/// refusals toward the strike threshold, and resets the count on the
/// next admitted request — the three outcomes the serving side
/// reads when it decides between a silent drop and a strike.
#[test]
fn req_bucket_prices_refusals_and_reset() {
    let mut buckets = PeerBuckets::new();
    // The initial token admits one cheap request at once; a
    // full-cost block-range request on the empty bucket is refused.
    assert!(buckets.take_req(REQ_COST_TIP));
    assert!(!buckets.take_req(REQ_COST_BLOCKS));
    // The first two consecutive refusals stay below the strike
    // threshold: ordinary congestion costs nothing.
    assert!(!buckets.note_req_refusal());
    assert!(!buckets.take_req(REQ_COST_BLOCKS));
    assert!(!buckets.note_req_refusal());
    // The third consecutive refusal reports the threshold, and the
    // serving side turns that into one strike.
    assert!(!buckets.take_req(REQ_COST_BLOCKS));
    assert!(buckets.note_req_refusal());
    // After one refill period a cheap request is admitted again; the
    // serving side clears the count, so old refusals cannot stack
    // into a future strike.
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    assert!(buckets.take_req(REQ_COST_TIP));
    buckets.clear_req_refusal();
    assert!(!buckets.take_req(REQ_COST_BLOCKS));
    assert!(
        !buckets.note_req_refusal(),
        "the count restarts after an admitted request"
    );
}

/// A charge larger than the whole balance empties the bucket, and
/// the peer's next block message waits out a full refill cycle.
/// A partial charge leaves less than one token — admission still
/// fails — and a couple of seconds of refill restores it: the charge
/// consumed real budget without draining it.
#[test]
fn block_byte_charge_drains_the_bucket_past_the_burst() {
    let mut buckets = PeerBuckets::new();
    buckets.charge_block_bytes(1024 * 1024);
    assert!(
        !buckets.take_block(),
        "a parked one-MiB block zeroes the block bucket"
    );
    let mut small = PeerBuckets::new();
    small.charge_block_bytes(100 * 1024);
    assert!(
        !small.take_block(),
        "a partial charge leaves less than one token"
    );
    std::thread::sleep(std::time::Duration::from_millis(2_200));
    assert!(
        small.take_block(),
        "two seconds of refill restore admission"
    );
}
