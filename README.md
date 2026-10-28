# Slash Protocol

[![CI](https://github.com/slash-protocol/slash/actions/workflows/ci.yml/badge.svg)](https://github.com/slash-protocol/slash/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)](https://www.rust-lang.org)

A cell-based blockchain with RandomX proof-of-work, paged multi-input state (PMIS), adaptive governance via version bits, layered onion routing for transaction privacy, and a cell-denominated bonding-curve treasury with a halving emission schedule.

## Overview

Slash introduces a **Paged Multi-Input State (PMIS)** engine that partitions a global ledger of one billion cells into 65,536-cell pages. This design enables:

- **O(1) balance lookups** via an incremental balance cache.
- **Atomic cross-page transactions** with speculative execution and rollback.
- **Light-client verification** through per-page Merkle roots and compact proofs.
- **Stake-locking** at the cell level for time-bound ownership.
- **Emission-backed security**: a Bitcoin-like subsidy schedule funds the RandomX hash rate from day one, and a perpetual one-cell-per-block slot tail keeps paying the miner after the subsidy ends.

Consensus is secured by **RandomX proof-of-work**, ensuring CPU-friendly and ASIC-resistant mining. The network supports soft-fork upgrades through **BIP-9 style version bits** with real signal counting and threshold enforcement.

## Key Features

### PMIS — Paged Multi-Input State
- **1 billion cells** partitioned into 15,259 pages of 65,536 cells each.
- Dirty pages are materialized in memory; clean pages are lazily reconstructed as treasury-owned.
- Pristine page eviction automatically drops pages that return to a single treasury range, keeping memory bounded.

### RandomX Proof-of-Work
- Miner and verifier use identical hash inputs (`prev || time || height || nonce || miner`) for consensus consistency.
- Parallel mining with extranonce striding across all CPU cores.
- Cooperative cancellation via `Arc<AtomicBool>` so the event loop never stalls.
- Context cache with 600-second TTL separates full-memory mining contexts from light verification contexts.

### Emission Schedule
- **Coinbase subsidy** of 500 cells per block at genesis, halving every 4 years (420,480 blocks at a 300-second target).
- Nine eras; total emission **417,957,120 cells (~41.8 % of supply)** over ~36 years. From era 9 on, miner revenue is transaction fees **plus the perpetual one-cell slot tail** paid every block forever — the slot mint never stops (see `PERPETUAL_TAIL_EMISSION.md` for the decision record).
- Coinbase is a mandatory `txs[0]` treasury transaction paying the miner exactly `subsidy_at(height)`; all other treasury transactions require the treasury signer key.

### Governance (Version Bits)
- Soft-fork deployments track lifecycle states: `Defined → Started → LockedIn → Active / Failed`.
- Real cumulative signal counting against configurable thresholds; the tally of a window is evaluated by its closing block and the counter opens fresh with the next one.
- Active rules include `MinTxAmount`, `MaxBlockSize`, `AllowScheme1`, and `RequireBlockVersion2`.

### On-Chain DAO
- Proposals and votes ride transaction memos; weight is locked supply pinned at the proposal snapshot.
- Treasury movements execute only through passed proposals once `dao_v1` activates; the legacy signer loses authority at activation.
- An `EmergencyPause` freezes flagged transaction classes without halting blocks, bounded to a horizon governance can always revisit; an `EmergencyResume` submission is the lifeline admitted while the broad freeze is live.

### Onion Routing
- Three-hop onion envelopes with fixed-size layers (1024 bytes) and inner payloads (2048 bytes).
- X25519 ephemeral keys and AES-256-GCM encryption at each hop.
- P2P forwarding uses request-response unicast to the next relay, preventing broadcast-based traffic correlation.

### Economic Layer
- **Mandatory Fees**: every transaction pays at least 0.5 % of the transferred amount into the `FEE_VAULT`; miners claim accumulated fees in their blocks.
- **Bonding Curve Treasury**: linear price growth denominated in cells; buys are paid into `TREASURY_VAULT` and minted atomically in the same block; sells are paid back at 98 % of the curve price, keeping the 2 % spread in the vault.
- **Treasury Signer**: mint and payout transactions are signed with a dedicated Ed25519 key (key separation: signer / onion).

## Build Requirements

- **Rust** 1.87 or later
- **Cargo**
- **OpenSSL development headers** (for libp2p noise protocol)
- **CMake** (for RandomX VM compilation)

On Ubuntu/Debian:
```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake libssl-dev pkg-config
```

On macOS:
```bash
brew install cmake openssl
```

## Building

```bash
# Clone the repository
git clone https://github.com/slash-protocol/slash.git
cd slash

# Build in release mode
cargo build --release

# Build with all optimizations and benchmarks
cargo build --release --benches
```

## Testing

```bash
# Run the full test suite
cargo test

# Run tests with output visible
cargo test -- --nocapture

# Run a specific test
cargo test test_coinbase_amount_matches_schedule

# Run benchmarks
cargo bench
```

## Running a Node

```bash
# Start a full node: RPC binds loopback only (safe default)
./target/release/slash-node --port 30333 --rpc-port 9944

# Expose the RPC port on every interface: control methods (stop, mine)
# require a token — an unauthenticated control socket is a remote
# shutdown vector
./target/release/slash-node --rpc-external --rpc-token <token> --port 30333 --rpc-port 9944

# Start in testnet mode
./target/release/slash-node --testnet --port 30333 --rpc-port 9944

# Start in pruned mode (keeps only the last 10,000 blocks)
./target/release/slash-node --pruned --port 30333 --rpc-port 9944

# Mine with an authorized key: the coinbase and fee claims pay the key's address
./target/release/slash-node --miner-key <hex-secret> --port 30333 --rpc-port 9944
```

## Project Structure

```
slash/
├── src/
│   ├── lib.rs               # Crate root, testnet flag, save/load helpers, RPC chain cache
│   ├── main.rs              # Node binary: CLI, startup, RPC + event loop wiring
│   ├── chain/               # Blocks, validation, coinbase rules, PoW, version bits, reorg
│   │   ├── pow.rs           # Proof-of-work input builder and verifier
│   │   ├── difficulty.rs    # Retarget arithmetic
│   │   ├── subsidy.rs       # Emission schedule
│   │   ├── sighash.rs       # Transaction signature hash
│   │   ├── version_bits.rs  # Deployment lifecycle types
│   │   ├── deployments.rs   # Per-window signal counting
│   │   ├── validation.rs    # Mempool admission gates, header batch validation
│   │   ├── execution/       # Block execution pipeline (meta, transactions, pairing)
│   │   ├── meta.rs          # Header checks: linkage, median time, checkpoints
│   │   ├── history.rs       # Reorg window, state snapshots
│   │   └── ingress.rs       # Fork/orphan pool admission with byte budgets
│   ├── state/               # PMIS engine: pages, transfers, locks, selection, minting
│   │   ├── transfer.rs      # Staging/commit pipeline (shared by engine and admission)
│   │   ├── transfer_gates.rs# The shared predicate core
│   │   ├── transfer_speculative.rs # The read-only admission twin
│   │   ├── locks.rs         # Lock registration, pruning, spend gating
│   │   ├── minting.rs       # Slot mint, conservation invariants
│   │   └── selection.rs     # Canonical range selection
│   ├── dao/                 # On-chain governance: types, state, weights, payloads
│   ├── treasury.rs          # Bonding-curve math and treasury state
│   ├── crypto.rs            # Ed25519, X25519, AES-GCM, Argon2id, wallets
│   ├── mining.rs            # RandomX miner with extranonce search
│   ├── p2p/                 # libp2p swarm, mempool, sync, onion forwarding
│   │   ├── limits.rs        # Cost-priced request budgets, byte charges
│   │   ├── conn.rs          # Subnet/node-wide connection budgets
│   │   ├── replay.rs        # Onion replay cache
│   │   └── handlers/        # Sync, gossip, wire handlers
│   ├── rpc/                 # JSON-RPC HTTP server and method handlers
│   ├── storage/             # Atomic writes, manifest, crash-recovery journal
│   ├── onion/               # 3-hop envelope construction and peeling
│   ├── governance.rs        # Soft-fork rule registry
│   ├── global_root.rs       # Incremental global root
│   ├── ffi.rs               # C-API for external wallet integration
│   ├── simulation.rs        # Economic stress tests
│   └── test_fixtures/       # Shared fixtures (test-utils feature)
├── benches/
│   ├── main.rs              # Criterion entry point
│   └── <group>.rs           # Benchmark groups (tx, pov, serialization, ...)
├── tests/
│   ├── chain_tests/         # Consensus, coinbase, emission, forks, limits
│   ├── dao_tests/           # Governance lifecycle, payloads, provenance
│   ├── dev_fee_tests/       # Dev-fund activation and spend rules
│   ├── emission_harness_tests/ # Era boundary vectors
│   ├── era_nine_boundary_tests/ # Zero-subsidy boundary
│   ├── integration_tests/ # Wallet, sync, staking, pruning, persistence
│   ├── p2p_tests/           # Wire format, mempool policy, DoS resistance
│   ├── reorg_persistence_tests/ # Reorgs across restarts
│   ├── slow_chain_tests/    # CPU-heavy consensus walks
│   ├── soak_invariants_tests/   # Long-run conservation soaks
│   ├── soak_network_tests/  # Multi-node fork soaks
│   ├── treasury_tests/      # Bonding-curve pairing
│   ├── wire_format_tests/   # Codec and frame bounds
│   └── <standalone>.rs    # crypto, onion, storage, ffi, version-bits windows
├── Cargo.toml
├── README.md
├── LICENSE
└── .github/
    └── workflows/
        └── ci.yml
```

## JSON-RPC API

The node exposes a JSON-RPC 2.0 HTTP endpoint. It binds loopback by default; `--rpc-external` exposes it on every interface and requires `--rpc-token`, without which the server refuses to start — the `stop` and `mine` methods accept unauthenticated calls only on the loopback bind.

| Method | Params | Description |
|--------|--------|-------------|
| `getBalance` | `["hex_address"]` | Returns the cell count for an address. |
| `getBlock` | `[height]` | Returns the full block at the given height, or `null`. |
| `sendRawTx` | `["hex_raw_tx"]` | Validates a raw transaction and broadcasts it to the mempool. |
| `getTip` | `[]` | Returns the current tip height and block hash. |
| `getDifficulty` | `[]` | Returns the difficulty target for the next block. |
| `getPageProof` | `[cell_index]` | Returns a Merkle proof for the page containing the cell. |

Control methods: `stop` and `mine` forward to the node command channel. On an external bind they require the token as the first string param or as a `{"token": "..."}` object.

Example:
```bash
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getTip","params":[],"id":1}'
```

## FFI

A C-compatible API is provided for hardware wallet and mobile integration:

- `slash_init(datadir)` → anchors all library file access to the directory without changing the process working directory; call exactly once, before any other call
- `slash_generate_encrypted_keypair(password)` → hex public key plus an encrypted wallet blob (JSON: `{"public","wallet"}`)
- `slash_decrypt_secret(wallet_hex, password)` → hex-encoded secret for signing (caller frees and overwrites)
- `slash_get_balance(hex_public)` → balance as `uint64_t`
- `slash_sign_transaction(hex_secret, hex_payload)` → hex-encoded signature; the payload must already be the protocol's per-transaction sighash
- `slash_verify_page_proof(hex_root, hex_leaf, leaf_index, siblings_json)` → `int`; sibling arrays are capped at 64 entries
- `slash_free_string(char*)`

## Network Identifiers

- **Mainnet**: `/slash/0.6.0`
- **Testnet**: `/slash/test/0.6.0`

The `TESTNET_MODE` atomic boolean switches signature hashing between the two chain IDs, preventing replay across networks.

## Emission Reference

| Era | Heights | Subsidy/block | Era supply |
|---|---|---|---|
| 0 | 0 – 420,479 | 500 | 210,240,000 |
| 1 | 420,480 – 840,959 | 250 | 105,120,000 |
| 2 | 840,960 – 1,261,439 | 125 | 52,560,000 |
| 3 | 1,261,440 – 1,681,919 | 62 | 26,069,760 |
| 4 | 1,681,920 – 2,102,399 | 31 | 13,034,880 |
| 5 | 2,102,400 – 2,522,879 | 15 | 6,307,200 |
| 6 | 2,522,880 – 2,943,359 | 7 | 2,943,360 |
| 7 | 2,943,360 – 3,363,839 | 3 | 1,261,440 |
| 8 | 3,363,840 – 3,784,319 | 1 | 420,480 |
| 9+ | from 3,784,320 | 0 | 0 |

Total: **417,957,120 cells (~41.8 % of N)**. From era 9 onward the miner still receives exactly one slot cell per block, forever; see `PERPETUAL_TAIL_EMISSION.md` and `SLASH_EMISSION_AND_TREASURY_DESIGN.md` for the full specifications.

## Security

Please see [SECURITY.md](SECURITY.md) for our security policy and vulnerability reporting process.

## Contributing

We welcome contributions. Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on code style, testing, and the pull request process.

All code comments and documentation must be written in English. Comments must describe what the code does, not the development process (avoid tags like `NEW`, `CHANGE`, `FIXME` in committed code).

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

## Acknowledgments

- RandomX by the Monero project.
- libp2p by Protocol Labs.
- Ed25519/X25519 implementations by the dalek-cryptography project.
