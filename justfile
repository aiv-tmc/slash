# Local parity with CI. Install: cargo install just
set dotenv-load := false

default: ci

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --all-targets --all-features --locked -- -D warnings

test:
    cargo test --lib --bins --locked
    cargo test --doc --locked
    cargo test --locked --test chain_tests --test dao_tests --test dev_fee_tests \
        --test emission_harness_tests --test era_nine_boundary_tests \
        --test integration_tests --test p2p_tests --test reorg_persistence_tests \
        --test treasury_tests --test wire_format_tests

test-slow:
    cargo test --locked --test slow_chain_tests -- --test-threads=2
    cargo test --locked --test soak_invariants_tests -- --test-threads=2
    cargo test --locked --test soak_network_tests -- --test-threads=2

docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked --open

audit:
    cargo audit
    cargo deny check

bench-smoke:
    cargo bench --no-run --locked

ci: fmt-check lint test docs
