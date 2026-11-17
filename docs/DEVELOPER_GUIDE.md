# Slash Protocol — Developer Guide

**Protocol version:** `0.8.0`

## What is Slash?

Slash is a standalone Layer-1 blockchain using a **Paged Multi-Input State (PMIS)** instead of a UTXO or account model:

* **1 billion cells** as the atomic unit of value, owned through contiguous ranges.
* **RandomX proof-of-work** funded by a halving emission schedule plus a perpetual one-cell-per-block slot tail.
* **Adaptive governance** via BIP-9 style version bits with per-window signal counting.
* **On-chain DAO**: proposals and votes ride transaction memos; weight is snapshot-pinned locked supply; treasury moves execute only through passed proposals; an EmergencyPause freezes flagged classes without halting blocks, and an EmergencyResume submission is the lifeline admitted while the freeze is live.
* **Dev fund**: a basis-point share of transfers and coinbase, activated by soft fork, adjustable by the DAO.
* **Native onion routing** with challenge-attested relay keys.
* **Per-page Merkle trees** for compact light-client proofs.
* **Atomic cross-page transactions** with speculative execution and rollback.
* **A cell-denominated bonding-curve treasury** with atomic buy/sell settlement.

## Core Concepts: PMIS

The ledger is exactly **1,000,000,000 cells** (indices `0..999,999,999`). Cells partition into **pages** of **65,536 cells** → **15,259 pages** (page `p` covers `p*65536 .. p*65536+65535`; the last page holds 51,712 cells).

* **Clean (pristine)** page: never modified; implicitly owned entirely by the **Treasury** (`[0u8; 32]`), lazily reconstructed.
* **Dirty** page: stored as a sorted map of non-overlapping, gapless ranges `(start_offset, end_exclusive, owner_32)`, eagerly merged on adjacency; materialized in memory and persisted to disk. A dirty page that returns to a single treasury range is **evicted** automatically.

### Special Addresses

| Address (hex) | Name | Purpose |
|---|---|---|
| `0000…0000` | **Treasury** | Owns all unissued cells; mints subsidy and treasury-sale cells. |
| `0101…0101` | **Fee Vault** | Collects mandatory fees; miners claim without a signature. |
| `0202…0202` | **Treasury Vault** | Collects buy payments; funds sell payouts. |
| `0303…0303` | **Dev Fund** | Collects the dev-fund share; spends require the dev fund signer key, at most one per block. |

## Cryptographic Primitives

| Primitive | Use | Notes |
|---|---|---|
| Ed25519 | Transaction signing, treasury/dev-fund signer, peer identity, onion attestation | 32-byte secret, 32-byte public, 64-byte signature |
| X25519 | Onion key exchange | Ephemeral-static ECDH; all-zero shared secret (low-order peer) is rejected at every use site (RFC 7748 §6.1) |
| AES-256-GCM | Onion layers, wallet files | 12-byte random nonce; onion keys are domain-separated by a one-byte tag per plaintext class (routing/inner/marker) |
| Blake3 | Sighash, block hash, body hash, Merkle nodes, txid | |
| Argon2id | Wallet KDF | RFC 9106 first recommendation: 64 MiB, 3 iterations, 4 lanes, 32-byte output (compile-time constants) |

### Algorithm Agility (Crypto Rotation Without Downtime)

* **Proof-of-work domain tag**: every RandomX input is prefixed with `POW_DOMAIN = "SLASH-POW/v1"` (see `chain::pow_input`). Miner and verifier share the single builder, so both hash identical buffers. Rotating the PoW algorithm or domain is a new tag plus a new network identifier — a coordinated upgrade, never a mid-network switch.
* **Signature scheme dispatcher**: every transaction signature is verified through `crypto::verify_scheme`, which is fail-closed — only implemented schemes reach a verifier, all other identifiers are rejected outright. A scheme is consensus-valid only when (a) the version-bit policy admits it and (b) an implementation exists in the build. Deploying a new scheme therefore ships as an ordinary soft fork: implementation first, activation vote second.
* The transaction signature hash already commits to the one-byte `scheme` field, so a signature minted for one scheme can never be replayed under another.

### Consensus Predicate Notes (Specification)

* The proof-of-work predicate is **exact divisibility**: `value % difficulty == 0` over the first eight little-endian bytes of the RandomX output. The rule is the predicate itself, not the intuition `1/difficulty` per hash; re-derive any future parameter change against the predicate.
* For difficulties approaching 2^63 the success probability per hash deviates measurably from `1/difficulty` because the value space is bounded; the deviation is irrelevant until difficulties eight orders of magnitude beyond any realistic network, and zero difficulty fails closed on every input.
* The same `value` selects the PoW slot (`value % N`), so validity and slot are correlated: grinding for a desirable slot costs a quantifiable work penalty. This coupling is consensus-mandated — the slot binds the reward to the proof — and is recorded here so future reviewers do not re-derive it under pressure.

## Accounts, Addresses, and Keys

An address is the **32-byte Ed25519 public key** as 64 lowercase hex characters; no checksum — validation is signature verification.

**Wallet format**: `salt [u8;16] || nonce [u8;12] || ciphertext` (bincode), key = Argon2id(password, salt), cipher = AES-256-GCM. The FFI wallet functions transport the same triple hex-encoded inside a JSON envelope.

**Building a transaction**: select input ranges covering amount + fee + dev share; fee output to `FEE_VAULT` ≥ `total_out / 200`; while `dev_fund` is active, dev output to `DEV_FUND` ≥ `max(total_out * dev_fee_bps / 10000, 1)`; outputs pack inputs exactly in declaration order; `total_out ≥ 50`; `sig ≤ 64` bytes, `memo ≤ 4096` bytes; sign the sighash (hash first — never sign application data directly); broadcast via `sendRawTx`, gossipsub `slash_txs`, or an onion.

## Transactions

| Field | Type | Description |
|---|---|---|
| `from` | `[u8; 32]` | Sender public key, or TREASURY for treasury transactions. |
| `inputs` | `Vec<(u64, u64)>` | Global cell ranges, end exclusive. |
| `outputs` | `Vec<Output>` | `Output { start: u64, end: u64, to: [u8; 32], lock: Option<u64> }`; exactly packs inputs in declaration order. |
| `sig` | `Vec<u8>` | 64-byte Ed25519 signature (scheme 0); empty for coinbase and rule-validated cases. |
| `scheme` | `u8` | Signature scheme; committed into the sighash. |
| `memo` | `Option<Vec<u8>>` | DAO operations while `dao_v1` is active. |

**Validation rules** (all must hold): non-empty inputs; conservation `sum(inputs) == sum(outputs)`; `total_out ≥ 50`; field caps; known scheme admitted by active rules; valid signature against `from` (treasury txs verify against the treasury signer, dev-fund spends against the dev fund signer; while `dao_v1` is active every *signed* treasury tx is rejected in both paths); ownership and lock expiry at the executing height; exact output packing; per-page balance; fee ≥ `total_out / 200`; dev share while active; treasury pairing (mint/payout at index `i > 0` requires a matching earlier payment in the same block); locked outputs back to treasury refused in both paths.

**Signature hash** (Blake3 over): `from(32) || inputs(le u64 pairs) || outputs(start, end, to(32), lock?) || chain_id || scheme(1) || memo(0x00, or 0x01 || len || bytes)`. The chain id pins the replay domain; the memo commitment prevents re-purposing a signed transfer as a vote.

**Transaction id**: `txid = blake3(sig || serialize(tx))`.

**Fees**: miners claim accumulated vault fees through `fee_claims`, sweeping only unlocked vault cells in canonical order; a self-claim (recipient = fee vault) is a validated no-op, and claims carrying a lock are refused.

**Governance payloads (memo)**, while `dao_v1` active — `DaoOp::SubmitProposal { payload }` / `Vote { proposal_id, approve }` with `DaoPayload`: `TreasuryMint { amount, to }`, `TreasuryPayout { amount, to }`, `RotateDevFundSigner { public }`, `RotateTreasurySigner { public }`, `SetDevFee { bps }`, `SetMineFee { bps }`, `EmergencyPause { flags, until_height }`, `EmergencyResume`. The op applies only after the carrying transfer succeeds. Weight = locked cells with expiry ≥ `snapshot + 10,000` and creation ≤ snapshot; one address one vote (first wins); quorum 5 % of snapshot locked supply; strict majority of cast weight; execution at `end + 1008` — the close derives the scheduled execution height from the end height; terminal proposals pruned after retention. DAO-minted cells carry mint provenance and can never be sold back to the curve.

## Blocks and Consensus

Block fields: `version, prev, time, height, nonce, miner, mined_cell, txs (≤ 1000; txs[0] = coinbase while subsidy > 0), fee_claims, difficulty, page_roots (BTreeMap<u16, [u8;32]>)`. Light header = all fields except `txs`/`fee_claims`. `BlockHash = blake3(version || prev || time || height || nonce || miner || mined_cell || difficulty || body_hash)`, where `body_hash` commits to every transaction (including memos) and fee claim, so the header chain binds the post-state commitment.

**Proof-of-Work**: `input = POW_DOMAIN || prev(32) || time(le u64) || height || nonce || miner(32)`; `value = first 8 bytes of RandomX(input)`; valid iff `value % difficulty == 0`; slot = `value % 1_000_000_000`. Zero difficulty never validates. The stamped time must match the time the proof was ground against (median-time rule enforces it).

**Coinbase (eras 0–8)**: `txs[0]`, `from == TREASURY`, empty sig, outputs to `miner` plus the exact dev share while `dev_fund` is active, sum exactly `subsidy_at(height)`, inputs = canonical `select_single(TREASURY, subsidy)`. Era 9+: no coinbase; `txs` may be empty; transactions execute from index 0; every treasury transaction — signed or not — is refused. At every height the perpetual slot mint pays exactly one treasury cell to the miner (≈105,120 cells/year, ~0.0105 % of unissued supply), keeping `sum(balance_cache) == N` after every block.

**Emission**: `subsidy_at(h) = era >= 9 ? 0 : 500 >> (h / 420_480)`; total 417,957,120 cells (~41.8 % of N).

**Difficulty**: retarget every 2,016 blocks, target 300 s, `new = old * (2016*300) / actual_time`, clamped 4× per interval, minimum 1.

**Time rules**: `time` > median of the last 11 blocks; `time ≤ now + 2 h` (run NTP).

**Size**: serialized block ≤ 1,048,576 bytes; active `smaller_blocks` lowers this to 524,288 (stricter of the two applies at every validation path).

**Checkpoints**: a block at height ≤ H with a hash mismatching the checkpoint at H is rejected; the checkpoint match still runs the median-time, future-limit, and difficulty checks. Checkpoint maps are operator-produced trusted boot input — see SECURITY.md, "Checkpoint Management".

**Version bits**: BIP-9 lifecycle `Defined → Started → LockedIn → Active / Failed`; per-window counting with reset at window boundaries; threshold 1,512/2,016 in one completed window; activation deferred one interval; timeout expires to `Failed`. All built-in start heights are exact multiples of 2,016, so every deployment's first window is complete. Built-ins: `min_tx_v2` (bit 0), `smaller_blocks` (1), `scheme1_ready` (2), `require_v2` (3), `dev_fund` (4), `dao_v1` (5).

## P2P Network

libp2p: TCP + Noise + Yamux; gossipsub topics `slash_blocks`, `slash_txs`, `slash_onion`; request-response (4-byte LE length prefix, max frame 10 MiB) for header-first sync, block download, onion unicast, onion-key discovery.

`Req`: `GetBlock(u64)`, `GetBlocks(from, to)`, `GetTip`, `GetHeaders(from, to)`, `GetOnionKey`, `GetOnionKeyV2(nonce)`, `ForwardOnion(Vec<u8>)`. `Resp`: `Block`, `Blocks`, `Tip`, `Headers`, `OnionKey`, `OnionKeyV2(nonce, key, sig)`, `Ack`.

Relay onion keys enter the routing table only after the `OnionKeyV2` attestation verifies against the peer's Ed25519 identity and the key survives a low-order probe. The request channel is cost-priced per peer (three consecutive refusals = one strike; an admitted request resets the count). Sync: sampled tip polls (8 peers/tick, full poll every 12 ticks), header-first validation (height, linkage, median time, difficulty, PoW), then bodies; a full chunk triggers the next request immediately. Inbound caps: 64 node-wide, 8 per subnet; over-budget dials close at establishment. Strikes escalate to exponential backoff bans (60 s base, 3,600 s max). Bootstrap: DNS seeds, `bootstrap.json`, compiled-in list; addresses propagate through Kademlia.

## JSON-RPC API

JSON-RPC 2.0 over HTTP: `getBalance`, `getBlock`, `sendRawTx` (returns the content-based txid), `getTip`, `getDifficulty`, `getPageProof`, `getStatus`, `getMempool` (capped rendering), `getTreasuryState` (curve counters and signer keys), `buildTreasuryBuy` / `buildTreasurySell` (report `price_position`, queued volume, `worst_case_position`; zero-payout sells refused), `stop`, `mine`. Rate-limited per IP with swept idle buckets; poisoned locks recover via `into_inner`. Example: `curl -X POST http://localhost:9944 -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"getTip","params":[],"id":1}'`.

## C / FFI Bindings

```c
int   slash_init(const char* datadir);                    // once per process; anchors all file I/O, never touches cwd
char* slash_generate_encrypted_keypair(const char* password); // -> JSON {"public","wallet"}
char* slash_decrypt_secret(const char* wallet_hex, const char* password); // hex secret; free + overwrite promptly
unsigned long long slash_get_balance(const char* hex_public);
char* slash_sign_transaction(const char* hex_secret, const char* hex_payload); // payload must be the sighash
int   slash_verify_page_proof(const char* hex_root, const char* hex_leaf_hash,
                                unsigned long long leaf_index, const char* siblings_json); // siblings capped at 64
void  slash_free_string(char* s);
```

## Treasury & Economics

Linear bonding curve in cells: `price_per_cell = 1 * (1 + total_sold / 1_000_000)`; `buy_price(amount) = amount + amount * (2*total_sold + amount - 1) / (2*1_000_000)`; `sell_price = buy_price * 98 / 100` (2 % spread seeds the vault). Quotes report the curve position and queued volume; inclusion-time repricing is expected (earlier pairs in a block move the position for later ones).

* **Buy pair**: payment `T_buy` to `TREASURY_VAULT` at index `j`, then signer-minted `T_mint` to the buyer at index `i > j` with `cost ≥ buy_price(amount)` at the running `total_sold`. One payment funds exactly one mint.
* **Sell pair**: transfer `T_sell` to `TREASURY`, then signer `T_payout` of `sell_price(amount)` from the vault; vault must cover it. One-cell sells (zero payout) are refused with a named error.
* While `dao_v1` is active the signer key has no authority — treasury moves execute exclusively through passed proposals. `EmergencyPause` can freeze the curve surface (or all user transactions) without halting blocks; `EmergencyResume` is the only transaction class admitted while the broad freeze is live.
* **Dev fund**: transfers carry `≥ max(total_out * dev_fee_bps / 10000, 1)` to `DEV_FUND` while active; coinbase carries `subsidy * mine_fee_bps / 10000`; both bps values are DAO-adjustable; dev-fund spends verify against `dev_fund_signer`, require a configured key, and are limited to one per block.

## DAO Lifecycle

Submit at H (snapshot = H, votes in `[H, H+4032)`) → one vote per address, weight pinned at the snapshot via both lock maps → close at `H+4032` (quorum 5 % of snapshot locked supply, strict majority of cast weight) → execute at `end + 1008` through canonical treasury selection; unexecutable payloads expire. ≤ 64 open proposals; retention `5,040` blocks; ids never reused.

## Light-Client Verification

`PageProof { leaf_hash, siblings, leaf_index }`: fold `leaf_hash` with siblings (`blake3(hash || sib)` left, `blake3(sib || hash)` right), assert equality with the page root. Global root: Blake3 over every page id paired with its root; pristine pages contribute `blake3(0 || page_size || TREASURY)`. Headers carry PoW, difficulty schedule, and the page-root map, so a light client verifies the full header chain and then requests proofs only for cells it cares about.

## Onion-Routed Transactions

`OnionTx { layers: Vec<Layer> (≥ 3, attested keys only), inner (AES-GCM of the padded 2048-byte tx), inner_nonce, final_ephemeral }`; `Layer { ephemeral, nonce, ciphertext (padded to 1024 bytes) }`. Each layer derives its key from an ephemeral-static X25519 shared secret with a one-byte domain tag (routing vs inner vs marker); low-order points are refused at every Diffie-Hellman site. A relay peels its layer, forwards the remainder via request-response unicast; the exit relay decrypts the inner transaction and submits it to the mempool. Replay cache: 10,000 envelope hashes, FIFO, dirty-flagged atomic persist to `onion_seen.bin`.

## Storage & Persistence

`node_key.bin`, `x25519.bin`, `bootstrap.json`, `header.bin` (state header: page ids, both lock maps, balance cache, treasury/dev-fund/DAO/emergency fields), `pages/XXXX.bin` (per dirty page), `blocks.log` + `blocks.meta` (append-only block log with its window sidecar), `chain_journal.bin` (crash-recovery journal of applied blocks), `chain_meta.bin` (base height, checkpoints, deployments, chain id), `manifest.bin` (last consistent triple with per-file content stamps, written last), `snapshots/state_H.bin` (3 retained), `onion_seen.bin`. Atomic writes everywhere: `.tmp` → fsync → rename → fsync parent. The journal replays blocks committed after the last manifest and stops at the first record the chain refuses. Load-time sanity checks (height coordinate, contiguity, checkpoints, `sum(balance_cache) == N`) gate every startup, with fallback to snapshot recovery. All paths resolve against the data directory selected by `slash_init`.

## Constants & Limits

| Constant | Value | Description |
|---|---|---|
| `N` | `1_000_000_000` | Total cells. |
| `PAGE_SIZE` / `PAGE_COUNT` | `65_536` / `15_259` | Cells per page / page count. |
| `MIN` | `50` | Minimum cells per transaction. |
| `MIN_FEE_DENOMINATOR` | `200` | Fee ≥ total_out / 200. |
| `MAX_SIG_BYTES` / `MAX_MEMO_BYTES` | `64` / `4096` | Field caps. |
| `SUBSIDY_INITIAL` / `HALVING_INTERVAL` | `500` / `420_480` | Emission schedule. |
| `EMISSION_END_ERA` | `9` | Zero-subsidy boundary. |
| `MAX_TX_PER_BLOCK` | `1_000` | Transaction cap. |
| `MAX_BLOCK_SIZE_BYTES` | `1_048_576` | Base block size limit. |
| `DIFFICULTY_ADJUSTMENT_INTERVAL` | `2_016` | Retarget period. |
| `TARGET_BLOCK_TIME_SECS` | `300` | Target spacing. |
| `MAX_REORG_DEPTH` / `MAX_HISTORY` | `100` / `100` | Reorg window / state history depth. |
| `MEMPOOL_CAP` | `10_000` | Mempool transactions. |
| `PRUNED_KEEP_BLOCKS` | `10_000` | Pruned retention. |
| `MAX_LOCKED_CELLS` | `1_000_000` | Global lock cap. |
| `DAO_LOCK_MIN` | `10_000` | Weight lock horizon. |
| `DAO_QUORUM_BPS` / `DAO_THRESHOLD_BPS` | `500` / `5_000` | Quorum / majority. |
| `DAO_VOTING_PERIOD` / `DAO_EXEC_DELAY` / `DAO_TERMINAL_RETENTION` | `4_032` / `1_008` / `5_040` | Governance timing. |
| `MAX_SIDE_FORK_BYTES` / `MAX_ORPHAN_BYTES` | `32 MiB` / `64 MiB` | Fork/orphan pool byte ceilings. |
| `MAX_ONION_ENVELOPE` | `1 MiB` | Onion frame length guard. |

## Error Handling Cheat-Sheet

| Error | Likely cause |
|---|---|
| `InvalidPow` | Nonce misses difficulty, or header time disagrees with the proof's time. |
| `InvalidDifficulty` | Declared difficulty differs from the retargeted value. |
| `InvalidTimestamp` | Time ≤ median of the last 11, or > 2 h ahead. |
| `InvalidHeight` | Height does not continue the sequence. |
| `InvalidCoinbase` | Coinbase missing, mis-positioned, wrong amount, wrong outputs, dev share mismatch. |
| `InvalidTreasurySignature` | Signed treasury tx without a valid signer signature — or any treasury tx in the zero-subsidy era. |
| `InvalidPair` | Mint/payout without a matching earlier payment in the same block. |
| `MinFeeNotMet` / `DevFeeNotMet` | Fee or dev share below the required floor. |
| `InvalidTx` | Conservation, ownership, signature (scheme dispatch fail-closed), packing, lock, cap, or emergency-freeze failure. |
| `InvalidFeeClaims` | Claim pays someone other than the miner, carries a lock, or selects locked vault cells. |
| `InvalidPageRoot` | Recomputed page root differs from the commitment. |
| `Orphan` | Parent unknown; the block waits in the orphan pool. |
| `CheckpointViolation` | Hash mismatch at a checkpointed height. |
| `Duplicate` | Block already known. |

## Wallet Integration Checklist

1. Generate 32 random bytes → Ed25519 public key → address = hex(public).
2. Encrypt the secret (Argon2id 64 MiB / 3 / 4 + AES-256-GCM); persist `salt, nonce, ciphertext`; the FFI wallet functions produce and consume exactly this format.
3. Sync balance via `getBalance`, page proofs via `getPageProof`, or full replay via `GetBlocks`.
4. Build the transaction (inputs, exact packing, fee, dev share, optional lock for staking weight).
5. Compute the sighash per the construction above; sign; place the 64-byte signature in `tx.sig`.
6. Broadcast via `sendRawTx` / `slash_txs` / onion; match confirmations by `txid = blake3(sig || serialize(tx))`.
7. Governance: serialize `DaoOp` with bincode, attach as `memo`, re-sign (the memo is committed into the sighash), broadcast.
