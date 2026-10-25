//! Connection lifecycle and node key material.
//!
//! Inbound dials are gated against the connection budgets, slots are
//! charged at establishment and released at close, misbehavior
//! accumulates strikes into exponential bans, and the node's Ed25519
//! identity and X25519 onion key are loaded or created on disk. Every
//! admitted connection is challenged for an attested onion key, so the
//! routing table fills with verified relay keys instead of
//! unauthenticated advertisements. Key files live inside the library
//! data directory like every other on-chain file.
//!
//! The Ed25519 identity resolves exactly once per process and is cached
//! in memory: the libp2p keypair and the attestation signer derive from
//! the same cached seed, a deleted key file can no longer rotate the
//! attesting identity mid-session, and a corrupt file regenerates loudly
//! with the new public key printed instead of silently severing every
//! attestation the old identity had accumulated.

use super::node::Node;
use super::wire::Req;
use libp2p::{identity, Multiaddr, PeerId};
use std::time::{Duration, Instant};

/// The node identity seed, resolved exactly once per process. Every
/// consumer of the identity — the libp2p keypair and the onion-key
/// attestation signer — reads the same cached value, so the identity
/// cannot flap between two different keys no matter what happens to the
/// file on disk after startup.
static NODE_SECRET: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

impl Node {
    /// Gate an inbound dial against the node-wide and per-subnet budgets.
    /// The dial has not identified its peer yet, so no slot is charged
    /// here; the admission verdict is enforced when the connection
    /// establishes, since the incoming event cannot refuse the dial
    /// itself — an over-budget address is remembered and its connection
    /// is closed at establishment time before any traffic flows.
    pub(crate) fn on_incoming_connection(&mut self, send_back: Multiaddr) {
        if !self.budgets.admit(&send_back) {
            self.rejected_inbound.insert(send_back);
        }
    }

    /// Release the connection budgets and the onion routing entries when
    /// a connection closes. Only connections the budget table tracked as
    /// inbound release a slot; outbound dials release nothing. The
    /// attestation challenge is dropped with the connection: a response
    /// arriving after the close was not requested by any live session.
    pub(crate) fn on_connection_closed(&mut self, peer: PeerId) {
        self.budgets.on_closed(&peer);
        self.onion_keys.remove(&peer);
        self.onion_challenges.remove(&peer);
        if let Some(key) = self
            .onion_key_to_peer
            .iter()
            .find(|(_, p)| **p == peer)
            .map(|(k, _)| *k)
        {
            self.onion_key_to_peer.remove(&key);
        }
    }

    /// Charge the inbound budgets for an established connection, enforce
    /// any negative admission verdict, and challenge the peer for an
    /// attested onion key. The challenge binds the peer's X25519 relay
    /// key to its Ed25519 identity through a signature over a fresh
    /// random nonce, so the routing table only ever fills with keys the
    /// peer provably controls and that are provably fresh.
    pub(crate) fn on_connection_established(
        &mut self,
        peer: PeerId,
        endpoint: libp2p::core::ConnectedPoint,
    ) {
        self.budgets.on_established(peer, &endpoint);
        if let libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } = &endpoint {
            if self.rejected_inbound.remove(send_back_addr) {
                let _ = self.swarm.disconnect_peer_id(peer);
                return;
            }
        }
        // Issue the onion-key challenge: the random nonce is remembered
        // until the response arrives or the connection closes, and the
        // response arm accepts an attestation only when it carries this
        // exact nonce. A captured attestation from an earlier session or
        // another peer is therefore worthless.
        let nonce: [u8; 32] = rand::random();
        self.onion_challenges.insert(peer, nonce);
        let _ = self
            .swarm
            .behaviour_mut()
            .req_resp
            .send_request(&peer, Req::GetOnionKeyV2(nonce));
    }

    /// Record a misbehavior strike and ban the peer at the threshold;
    /// every further strike doubles the ban up to the maximum.
    pub(crate) fn record_strike(&mut self, peer: PeerId) {
        let strikes = self.strikes.entry(peer).or_insert(0);
        *strikes += 1;
        if let Some(duration) = super::util::ban_duration_secs(*strikes) {
            self.banned_until
                .insert(peer, Instant::now() + Duration::from_secs(duration));
        }
    }

    /// Whether the peer is currently banned.
    pub(crate) fn is_banned(&self, peer: &PeerId) -> bool {
        self.banned_until
            .get(peer)
            .is_some_and(|until| *until > Instant::now())
    }

    /// The 32-byte seed of the node identity, used to sign onion-key
    /// attestations. The value is resolved once per process and held in
    /// memory for the node's whole life: this method never touches the
    /// disk, so a key file deleted or corrupted after startup cannot
    /// rotate the attesting identity underneath the live libp2p
    /// keypair and trigger attestation-mismatch strikes.
    pub(crate) fn node_secret(&self) -> [u8; 32] {
        *NODE_SECRET.get_or_init(resolve_node_secret)
    }

    /// The X25519 onion secret, persisted as `x25519.bin` inside the
    /// library data directory.
    pub(crate) fn onion_secret(&self) -> [u8; 32] {
        load_or_create_onion_key()
    }

    /// The X25519 onion public key derived from the persisted onion secret.
    pub(crate) fn onion_public(&self) -> [u8; 32] {
        use x25519_dalek::{PublicKey, StaticSecret};
        let secret = StaticSecret::from(self.onion_secret());
        *PublicKey::from(&secret).as_bytes()
    }

    /// Resolve every DNS seed into dialable TCP multiaddresses and merge
    /// them into the known-addresses table.
    pub(crate) async fn resolve_dns_seeds(&mut self) {
        for seed in super::util::dns_seeds() {
            if let Ok(lookup) = tokio::net::lookup_host(format!("{}:30333", seed)).await {
                for ip in lookup {
                    let addr = format!("/ip4/{}/tcp/30333", ip.ip())
                        .parse()
                        .expect("seed addr");
                    self.add_known_addr(addr);
                }
            }
        }
    }

    /// Merge a bootstrap file's multiaddresses into the known-addresses
    /// table; a missing or corrupt file is not an error. The path is
    /// resolved against the library data directory.
    pub(crate) fn load_bootstrap_file(&mut self, path: &str) {
        let Ok(contents) = std::fs::read_to_string(crate::storage::data_path(path)) else {
            return;
        };
        let Ok(addrs) = serde_json::from_str::<Vec<String>>(&contents) else {
            return;
        };
        for addr in addrs {
            if let Ok(multi) = addr.parse::<Multiaddr>() {
                self.add_known_addr(multi);
            }
        }
    }
}

/// Resolve the node's Ed25519 identity seed exactly once per process.
/// A well-formed key file yields its seed; a missing or corrupt file
/// regenerates the identity and prints a loud warning naming the new
/// public key, because an unexpected rotation severs every attestation
/// the old identity had accumulated and an operator must be able to
/// tell "we rotated" from "we recovered". The regenerated seed is
/// written atomically before it is returned, so the next process start
/// adopts it.
fn resolve_node_secret() -> [u8; 32] {
    let path = crate::storage::data_path("node_key.bin");
    match std::fs::read(&path) {
        Ok(bytes) => match bincode::deserialize::<[u8; 32]>(&bytes) {
            Ok(sk) => sk,
            Err(_) => {
                let (sk, pk) = crate::crypto::generate_keypair();
                eprintln!(
                    "[p2p] node key file at {} is corrupt; regenerating the identity. \
                     New public key: {}",
                    path.display(),
                    hex::encode(pk)
                );
                persist_node_secret(&sk);
                sk
            }
        },
        Err(_) => {
            let (sk, pk) = crate::crypto::generate_keypair();
            eprintln!(
                "[p2p] node key file at {} is missing; generating a fresh identity. \
                 New public key: {}",
                path.display(),
                hex::encode(pk)
            );
            persist_node_secret(&sk);
            sk
        }
    }
}

/// Write the node secret through the atomic-write machinery so a crash
/// mid-rotation leaves either the old file or the new one, never a torn
/// seed that the next start would read as corrupt and rotate again.
fn persist_node_secret(sk: &[u8; 32]) {
    if let Ok(encoded) = bincode::serialize(sk) {
        let _ = crate::storage::atomic_write("node_key.bin", &encoded);
    }
}

/// Build the libp2p identity from the cached node seed. The raw 32-byte
/// seed decodes through libp2p's SecretKey interface — Keypair decoding
/// expects the 64-byte expanded form, so the seed enters as a SecretKey
/// and the keypair derives from it. The swarm keypair and the
/// attestation signer read the same once-per-process seed, so the
/// identity the network sees and the identity that signs onion-key
/// attestations are always one and the same key.
pub(super) fn load_or_create_node_key() -> identity::Keypair {
    let mut seed = *NODE_SECRET.get_or_init(resolve_node_secret);
    let secret = identity::ed25519::SecretKey::try_from_bytes(&mut seed)
        .expect("the stored seed is a valid ed25519 secret");
    identity::Keypair::from(identity::ed25519::Keypair::from(secret))
}

/// Load or create the node's X25519 onion secret, persisted as a bincode
/// 32-byte secret in `x25519.bin` inside the library data directory.
fn load_or_create_onion_key() -> [u8; 32] {
    if let Ok(bytes) = std::fs::read(crate::storage::data_path("x25519.bin")) {
        if let Ok(sk) = bincode::deserialize::<[u8; 32]>(&bytes) {
            return sk;
        }
    }
    let sk = crate::crypto::generate_x25519_secret();
    if let Ok(encoded) = bincode::serialize(&sk) {
        let _ = crate::storage::atomic_write("x25519.bin", &encoded);
    }
    sk
}
