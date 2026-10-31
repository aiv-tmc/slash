# Security Policy

## Reporting a Vulnerability

We take security seriously. If you discover a vulnerability, please follow the responsible disclosure process outlined below.

### Do Not

- Do not open a public issue for security vulnerabilities.
- Do not disclose the vulnerability publicly before a fix is released.
- Do not attack the mainnet or testnet to demonstrate the vulnerability.

### Do

- Email your findings to **security@slash-protocol.org**.
- Include a detailed description of the vulnerability.
- Include steps to reproduce, proof-of-concept code, or a detailed analysis.
- Include your assessment of impact and severity.
- Provide a timeline for public disclosure that allows us time to patch.

### What to Expect

1. **Acknowledgment** within 48 hours of your report.
2. **Initial assessment** within 7 days, including severity classification and planned fix timeline.
3. **Progress updates** every 14 days until the issue is resolved.
4. **Public disclosure** coordinated with you after a fix is released.

## Severity Classification

| Severity | Description | Example |
|----------|-------------|---------|
| Critical | Consensus failure, infinite minting, private key extraction, remote node compromise | Invalid PoW bypass (including a degenerate zero-difficulty target); state corruption; unsigned or mis-shaped coinbase minting subsidy; treasury signer key forgery; emission schedule violation (subsidy != subsidy_at(height)); unauthenticated remote shutdown of a node |
| High | Network partition, remote code execution, fund theft | P2P unauthenticated commands; mempool DoS; treasury buy/sell pair replay or double-mint against a single payment |
| Medium | Information disclosure, performance degradation, host-process compromise via embedding | Timing attacks on crypto; metadata leaks; a panicking call escaping the FFI boundary and aborting the embedding process |
| Low | Defense in depth, non-exploitable bugs | Missing input validation on admin RPCs behind loopback; verbose error messages |

## Bug Bounty

Critical and High severity findings that affect the mainnet consensus or economic layer may be eligible for a bug bounty. Details are shared privately with reporters upon acknowledgment.

Emission-critical areas with the highest expected reward:

- Coinbase shape validation (exactly one, first, correct amount, miner-only outputs).
- Treasury signature enforcement outside the coinbase.
- Buy/sell pairing (same-block, ordered, one payment funds exactly one mint).
- Fee-vault claim validation.
- Proof-of-work target validation: a zero difficulty must never satisfy `verify_pow`, and the declared difficulty must always equal the retargeted value.
- Transaction amplification caps: block execution must enforce the same range/page/field caps as mempool admission.

## Security Checklist for Contributors

- Never log or print private keys, seeds, or wallet passwords.
- Use `zeroize` for all secret byte arrays.
- Validate all P2P message lengths before deserialization.
- Ensure atomic writes use `fsync` before `rename`.
- Run `cargo audit` regularly to check for vulnerable dependencies.
- Every new consensus rule enters both validation paths (mempool admission and block execution); the TEST.md matrix tracks this pairing per rule.
- Control-plane RPC methods (`stop`, `mine`) must refuse unauthenticated calls whenever the socket accepts non-loopback connections.

## Known Security Considerations

The following are acknowledged limitations tracked in the codebase:

- **L5 (resolved)**: The historical three-layer `save()` was not atomic across blocks, state, and metadata. Persistence now goes through a single manifest snapshot plus a crash-recovery journal: `save_manifest` writes a consistent (blocks, state, base height, checkpoints, version-bit deployments) snapshot, and the journal records applied blocks so that a crash between manifest writes replays committed blocks on load. Recovery additionally rebuilds the bounded `state_history` via `rebuild_state_history()` and prunes expired stake locks.
- **L6 (resolved)**: `sendRawTx` previously validated without relaying. Transactions are now queued into a global pending buffer that the P2P node drains into the mempool and gossips every 2 seconds.
- **L11**: Per-PeerId rate limiting can be bypassed by a Sybil attacker generating many PeerIds. Subnet-level and node-wide inbound budgets bound the damage; stake-based rate limiting is under research.
- **RPC control methods (hardened)**: `stop` and `mine` previously answered any caller on an `0.0.0.0` bind — an unauthenticated remote shutdown. The server now binds loopback by default, refuses an external bind without `--rpc-token`, and requires the token on every control call that crosses loopback.
- **Treasury signer key**: mints and payouts are authorized by a single Ed25519 signer key (`TreasuryState.signer_public`). Loss or compromise of this key is a Critical-severity event. Multisig or HSM-backed signing is planned before mainnet (see the v0.3.0 design document, §10.1). Nodes that have not configured a signer (all-zero key) refuse every non-coinbase treasury transaction by construction. Once `dao_v1` activates, the signer loses authority entirely and treasury moves flow only through passed proposals.
- **Onion metadata**: While onion payloads are encrypted, libp2p connection metadata (who talks to whom) is visible to the transport layer. Guard nodes and traffic padding are future work.
- **Mempool/consensus parity**: mempool admission (`Chain::validate_tx`) and block execution (`execute_block_txs`) enforce the same structural invariants (field caps, range/page caps, emergency freeze, scheme gate, dev-fund cap). Any future validation rule must be added to both paths; the TEST.md matrix tracks this pairing per rule.

## Cryptographic Dependencies

- `ed25519-dalek` 2.1 for signatures
- `x25519-dalek` 2.0 for key exchange
- `aes-gcm` 0.10 for symmetric encryption
- `blake3` 1.5 for hashing
- `rust-randomx` 0.7 for proof-of-work

These dependencies are monitored via `cargo audit` in CI.
