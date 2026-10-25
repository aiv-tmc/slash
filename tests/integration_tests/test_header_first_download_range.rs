//! `test_header_first_download_range` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_header_first_download_range() {
    // The header download range over a five-block chain returns headers
    // for heights 2..=4 in ascending order.
    let mut chain = Chain::genesis();
    let miner = [1u8; 32];
    for h in 1..=5 {
        let b = mine_valid_block(&chain, h, miner, chain.next_difficulty());
        let b = chain.prepare_block(b);
        chain.apply(b).unwrap();
    }
    let headers: Vec<_> = chain
        .blocks
        .iter()
        .filter(|b| b.height >= 2 && b.height <= 4)
        .map(chain::BlockHeader::from_block)
        .collect();
    assert_eq!(headers.len(), 3);
    assert!(headers.windows(2).all(|w| w[0].height < w[1].height));
}
