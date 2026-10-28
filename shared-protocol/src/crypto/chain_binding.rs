//! Chain-identity and protocol-version binding for signature preimages (#2123).
//!
//! # What this makes true
//!
//! Every signature is valid on exactly one network and one protocol version.
//! Without that, a signature harvested from the testnet is replayable on
//! mainnet — #2065 found the signing envelope hardwiring mainnet identifiers,
//! and #2066 is the open block-level instance of the same defect.
//!
//! # Pure core, thin config edge
//!
//! Everything that computes bytes takes an explicit [`ChainIdentity`] and
//! [`SigningProtocolVersion`]. Only [`ChainIdentity::from_config`] and
//! [`SigningProtocolVersion::active`] read the environment. Two reasons:
//!
//! 1. **Determinism.** A preimage builder that reads ambient state is a
//!    preimage builder whose output depends on when you called it.
//! 2. **Testability.** Tests pass identities explicitly rather than mutating
//!    process environment, so they cannot race each other under the default
//!    parallel test runner.
//!
//! # The version gate
//!
//! [`SigningProtocolVersion::V1`] is the default and reproduces today's bytes
//! exactly. Nothing in this module changes what a running node signs, hashes or
//! verifies until an operator explicitly selects V2. That default **is** the
//! gate required by #2123 — see `docs/security/PREIMAGE_MIGRATION.md`.
//!
//! #2076 is why the gate is not optional: a v1 collector signing against a v2
//! verifier silently halted the earn loop on this chain for 28 hours. Before a
//! public testnet the cost of getting this wrong is a version bump; after, it is
//! a coordinated chain reset.

use std::fmt;
use std::sync::OnceLock;

use crate::crypto::domain_signing::{
    CHAIN_ID_ENV, DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID, NETWORK_ID_ENV, push_length_prefixed,
    signing_chain_id, signing_network_id,
};
use crate::crypto::hash::hash_bytes;

/// Environment variable selecting the active signing protocol version.
///
/// Absent or unparseable means [`SigningProtocolVersion::V1`] — failing closed
/// onto the legacy encoding, never forward onto an unreleased one.
pub const SIGNING_PROTOCOL_VERSION_ENV: &str = "WRFCOIN_SIGNING_PROTOCOL_VERSION";

/// Key under which the genesis block records its chain id.
pub const GENESIS_CHAIN_ID_KEY: &str = "chain_id";
/// Optional faucet sink recorded in block 1. Absence or an invalid address
/// leaves the faucet slice unclaimed; followers never consult local config.
pub const GENESIS_FAUCET_ADDRESS_KEY: &str = "faucet_address";

/// Persisted genesis inputs to ADR-003 emission selection (#3039).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenesisEmissionConfig {
    pub chain_id: String,
    pub faucet_address: Option<crate::common::types::Address>,
    /// Absent on historical 777 records, preserving their serialized bytes.
    pub policy: Option<GenesisTestnetPolicy>,
}

// The enclosing State checkpoint uses a positional binary codec. Appending
// a nested field would consume the next State field on old checkpoints. Keep
// the historical two-field frame and put ONLY opted-in policies in a tagged
// chain-field envelope; legacy chain strings and bytes stay unchanged. This
// envelope is storage-only, never a chain identity or signing preimage.
const GENESIS_POLICY_ENVELOPE: &str = "\0wrfcoin-genesis-policy-v1:";

#[derive(serde::Serialize, serde::Deserialize)]
struct GenesisEmissionWire {
    chain_id: String,
    faucet_address: Option<crate::common::types::Address>,
}

impl serde::Serialize for GenesisEmissionConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let chain_id = match &self.policy {
            None => self.chain_id.clone(),
            Some(policy) => format!("{}{}", GENESIS_POLICY_ENVELOPE,
                serde_json::to_string(&(&self.chain_id, policy)).map_err(serde::ser::Error::custom)?),
        };
        GenesisEmissionWire { chain_id, faucet_address: self.faucet_address }.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for GenesisEmissionConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = GenesisEmissionWire::deserialize(deserializer)?;
        let (chain_id, policy) = if let Some(payload) = wire.chain_id.strip_prefix(GENESIS_POLICY_ENVELOPE) {
            let (chain, policy): (String, GenesisTestnetPolicy) = serde_json::from_str(payload)
                .map_err(serde::de::Error::custom)?;
            if chain.starts_with('\0') || chain == "777" || policy.network_id != "testnet"
                || policy.flat_reward_seconds.is_some_and(|v| v != 600)
                || policy.flat_reward_seconds.is_some() != policy.flat_reward_base_units.is_some()
                || policy.flat_reward_base_units.is_some_and(|v| v == 0 || v > 3_333_333_333 || policy.flat_reward_seconds != Some(600))
                || policy.faucet_allocation > crate::consensus::proof_of_publishing::MAX_SUPPLY {
                return Err(serde::de::Error::custom("invalid genesis policy envelope"));
            }
            (chain, Some(policy))
        } else { (wire.chain_id, None) };
        Ok(Self { chain_id, faucet_address: wire.faucet_address, policy })
    }
}

/// Optional, immutable emission declaration for a fresh testnet genesis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenesisTestnetPolicy {
    pub network_id: String,
    pub flat_reward_seconds: Option<u64>,
    #[serde(default)]
    pub flat_reward_base_units: Option<u64>,
    pub faucet_allocation: u128,
}

pub const GENESIS_FLAT_REWARD_KEY: &str = "testnet_flat_reward_seconds";
pub const GENESIS_FLAT_REWARD_UNITS_KEY: &str = "testnet_flat_reward_base_units";
pub const GENESIS_FAUCET_ALLOCATION_KEY: &str = "testnet_faucet_allocation_base_units";
pub const GENESIS_NODE_EPOCH_KEY: &str = "node_share_epoch_blocks";
pub const GENESIS_NODE_CHALLENGE_KEY: &str = "node_share_challenge_blocks";
pub const GENESIS_GOVERNANCE_KEYS_KEY: &str = "schedule_governance_keys";


impl GenesisEmissionConfig {
    /// Read only the canonical genesis block. Later metadata cannot retune
    /// emission or redirect the sink.
    pub fn from_block(block: &crate::blockchain::block::Block) -> Option<Self> {
        if block.height != 1 {
            return None;
        }
        let chain_id = block.metadata.get(GENESIS_CHAIN_ID_KEY)?.clone();
        // Preserve the historical 777 config and root encoding exactly.
        let policy = if chain_id != "777"
            && block.metadata.get(GENESIS_NETWORK_ID_KEY).map(String::as_str) == Some("testnet")
        {
            Some(GenesisTestnetPolicy {
                network_id: "testnet".into(),
                flat_reward_seconds: block.metadata.get(GENESIS_FLAT_REWARD_KEY)
                    .and_then(|v| v.parse().ok()),
                flat_reward_base_units: block.metadata.get(GENESIS_FLAT_REWARD_UNITS_KEY)
                    .and_then(|v| v.parse().ok()),
                faucet_allocation: block.metadata.get(GENESIS_FAUCET_ALLOCATION_KEY)
                    .and_then(|v| v.parse().ok()).unwrap_or(0),
            })
        } else { None };
        Some(Self {
            chain_id,
            policy,
            faucet_address: block.metadata.get(GENESIS_FAUCET_ADDRESS_KEY)
                .and_then(|address| address.parse().ok()),
        })
    }
}
/// New-chain rules apply to recorded content, never verifier configuration.
pub fn validate_testnet_genesis(block: &crate::blockchain::block::Block) -> Result<(), String> {
    if block.height != 1
        || block.metadata.get(GENESIS_NETWORK_ID_KEY).map(String::as_str) != Some("testnet")
        || block.metadata.get(GENESIS_CHAIN_ID_KEY).map(String::as_str) == Some("777")
    { return Ok(()); }
    let chain = block.metadata.get(GENESIS_CHAIN_ID_KEY)
        .filter(|s| !s.is_empty()).ok_or("testnet genesis requires chain_id")?;
    if chain.len() > 64 || !chain.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err("invalid testnet genesis chain_id".into());
    }
    let schedule = block.metadata.get(GENESIS_ACTIVATION_SCHEDULE_KEY)
        .filter(|s| !s.trim().is_empty()).ok_or("testnet genesis requires an activation schedule")?;
    let parsed = parse_genesis_activation_schedule(schedule).map_err(|e| e.to_string())?;
    if parsed.features.iter().any(|(name, height)| name == "node_share_settlement" && *height < 2) {
        return Err("node_share_settlement must activate after block 1".into());
    }
    if parsed.features.iter().any(|(name, _)| name == "schedule_extension" || name == "node_share_settlement") {
        genesis_schedule_governance_keys(block)?;
    }
    for key in [GENESIS_NODE_EPOCH_KEY, GENESIS_NODE_CHALLENGE_KEY] {
        if block.metadata.get(key).is_some_and(|value| value != "512") {
            return Err("new testnet node timing must be 512 blocks when recorded".into());
        }
    }
    if block.metadata.contains_key(GENESIS_NODE_EPOCH_KEY) != block.metadata.contains_key(GENESIS_NODE_CHALLENGE_KEY) {
        return Err("node epoch and challenge must be declared together".into());
    }
    if let Some(seconds) = block.metadata.get(GENESIS_FLAT_REWARD_KEY) {
        if seconds != "600" { return Err("testnet flat reward tier must be 600 seconds".into()); }
    }
    if block.metadata.contains_key(GENESIS_FLAT_REWARD_KEY) != block.metadata.contains_key(GENESIS_FLAT_REWARD_UNITS_KEY) {
        return Err("flat reward seconds and explicit base-unit amount must be recorded together".into());
    }
    if let Some(raw) = block.metadata.get(GENESIS_FLAT_REWARD_UNITS_KEY) {
        let amount = raw.parse::<u64>().map_err(|_| "invalid flat reward amount")?;
        if amount == 0 || amount > 3_333_333_333 || amount.to_string() != *raw
            || block.metadata.get(GENESIS_FLAT_REWARD_KEY).map(String::as_str) != Some("600") {
            return Err("flat reward amount requires the 600-second tier and a positive bounded budget".into());
        }
    }
    if let Some(raw) = block.metadata.get(GENESIS_FAUCET_ALLOCATION_KEY) {
        let amount = raw.parse::<u128>().map_err(|_| "invalid genesis faucet allocation")?;
        if amount == 0 || amount.to_string() != *raw
            || amount > crate::consensus::proof_of_publishing::MAX_SUPPLY {
            return Err("genesis faucet allocation outside supply bound".into());
        }
    }
    let sink = block.metadata.get(GENESIS_FAUCET_ADDRESS_KEY)
        .and_then(|v| v.parse::<crate::common::types::Address>().ok());
    if sink.is_none() || sink == Some(crate::common::types::Address([0; 20])) {
        return Err("testnet genesis requires a nonzero faucet address".into());
    }
    Ok(())
}

pub fn genesis_schedule_governance_keys(block: &crate::blockchain::block::Block) -> Result<Vec<String>, String> {
    let raw = block.metadata.get(GENESIS_GOVERNANCE_KEYS_KEY)
        .ok_or("testnet genesis requires schedule governance keys")?;
    let keys: Vec<String> = raw.split(',').map(str::to_string).collect();
    if keys.is_empty() || keys.len() > 32 || keys.windows(2).any(|w| w[0] >= w[1])
        || keys.iter().any(|k| k.len() != 64 || !k.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || k.bytes().all(|b| b == b'0') || crate::crypto::signature::PublicKey::from_hex(k).is_err()) {
        return Err("governance keys must be sorted unique nonzero canonical public keys".into());
    }
    Ok(keys)
}

impl GenesisEmissionConfig {
    pub fn emission_partition(&self, height: u64) -> crate::consensus::proof_of_publishing::EmissionPartition {
        use crate::consensus::proof_of_publishing as pop;
        let testnet = self.policy.as_ref().is_some_and(|p| p.network_id == "testnet");
        let reward = if self.policy.as_ref().and_then(|p| p.flat_reward_seconds) == Some(600) {
            // The owner-selected 10-minute tier, held flat from the cut.
            self.policy.as_ref().and_then(|p| p.flat_reward_base_units)
                .expect("validated genesis flat reward requires an explicit base-unit amount")
        } else { pop::adjusted_block_reward(height) };
        pop::EmissionPartition::from_emission(reward as u128,
            if testnet { Some("777") } else { Some(self.chain_id.as_str()) })
    }
}

/// One committed faucet-sink binding for a chain whose genesis recorded no
/// `faucet_address` (#3361 option A). **Reviewable consensus data, not
/// configuration**: a row changes only through a binary release, and takes
/// effect only once its `feature` is scheduled by a governance-signed #3277
/// extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyFaucetBinding {
    /// The feature whose activation puts this row in force. Each rotation
    /// adds a row under a NEW feature name (`…_r2`, …): V6 forbids re-arming
    /// a name, so history stays replayable (#3361 Q3).
    pub feature: ConsensusFeature,
    /// Consensus key: matched against the chain id recorded in block 1
    /// (`State::genesis_emission_config`), never against process config or
    /// the boot-installed extension authority.
    pub genesis_chain_id: &'static str,
    /// The chain instance's block-1 hash. POLICY key only (the #3361 F3 sign
    /// CLI and admission), not read by consensus: the extension that
    /// activates the row is already bound to this hash (V2).
    pub genesis_hash: &'static str,
    /// The sink, as 40 lowercase hex digits without `0x`. `None` is an
    /// explicit unbind: the slice mints zero from the row's activation
    /// (never the zero address).
    pub address: Option<&'static str>,
}

/// Every faucet-sink binding this binary carries, in declaration order.
/// Append-only: the LATEST-declared active row that matches the chain wins.
pub const LEGACY_FAUCET_BINDINGS: &[LegacyFaucetBinding] = &[
    // #3361 owner ruling 2026-09-28 (Q1, Q4): the live public testnet cut on
    // 2026-09-13 without WRFCOIN_GENESIS_FAUCET_ADDRESS; this is the faucet
    // service's existing wallet.
    LegacyFaucetBinding {
        feature: ConsensusFeature::FaucetSinkBinding,
        genesis_chain_id: "777",
        genesis_hash: "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8", // pragma: allowlist secret -- public genesis hash
        address: Some("f72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"), // pragma: allowlist secret -- public faucet wallet ADDRESS, ruled on core4#3361
    },
];

/// Feature-name prefix of the faucet-sink binding family: the bare name and
/// every rotation (`faucet_sink_binding_r2`, …). ADMISSION POLICY (#3361
/// F3), never consensus: a name with this prefix is admitted, included or
/// signed only when a [`LEGACY_FAUCET_BINDINGS`] row carries exactly that
/// name for the node's own chain instance ([`faucet_binding_row_for`]).
pub const FAUCET_SINK_BINDING_PREFIX: &str = "faucet_sink_binding";

/// Why a `faucet_sink_binding*` name is not schedulable on a chain
/// instance (#3361 F3). POLICY only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaucetBindingRowError {
    /// This binary carries no row under that feature name (a future `_rN`,
    /// or a name this binary does not implement).
    NoRow { feature: String },
    /// The node has no block-1 hash to match (no installed authority, or an
    /// extension that names none): fail closed.
    NoChainInstance { feature: String },
    /// The row binds another chain instance.
    OtherChainInstance {
        feature: String,
        row_genesis_hash: &'static str,
        genesis_hash: String,
    },
}

impl fmt::Display for FaucetBindingRowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRow { feature } => write!(
                f,
                "feature {feature} has no faucet-binding row in this binary, so it is not \
                 schedulable on any chain (#3361 F3)"
            ),
            Self::NoChainInstance { feature } => write!(
                f,
                "feature {feature} is schedulable only on the chain instance its faucet-binding \
                 row names, and this node has no block-1 hash to match (#3361 F3)"
            ),
            Self::OtherChainInstance {
                feature,
                row_genesis_hash,
                genesis_hash,
            } => write!(
                f,
                "feature {feature} is bound to the chain whose block 1 is {row_genesis_hash}, \
                 not {genesis_hash}; it is not schedulable here (#3361 F3)"
            ),
        }
    }
}

/// The [`LEGACY_FAUCET_BINDINGS`] row carrying exactly `feature_name`, if
/// its `genesis_hash` is `genesis_hash` (#3361 F3). POLICY only (mempool,
/// producer, the sign CLI and every library signer); consensus never reads
/// a row's hash.
pub fn faucet_binding_row_for(
    feature_name: &str,
    genesis_hash: Option<&str>,
) -> Result<&'static LegacyFaucetBinding, FaucetBindingRowError> {
    faucet_binding_row_in(LEGACY_FAUCET_BINDINGS, feature_name, genesis_hash)
}

/// [`faucet_binding_row_for`] over an explicit row table (the committed
/// table in production; a synthetic one where a test must hold the sink's
/// key).
pub fn faucet_binding_row_in<'a>(
    rows: &'a [LegacyFaucetBinding],
    feature_name: &str,
    genesis_hash: Option<&str>,
) -> Result<&'a LegacyFaucetBinding, FaucetBindingRowError> {
    let row = rows
        .iter()
        .find(|row| row.feature.name() == feature_name)
        .ok_or_else(|| FaucetBindingRowError::NoRow {
            feature: feature_name.to_string(),
        })?;
    let genesis_hash = genesis_hash.ok_or_else(|| FaucetBindingRowError::NoChainInstance {
        feature: feature_name.to_string(),
    })?;
    if row.genesis_hash != genesis_hash {
        return Err(FaucetBindingRowError::OtherChainInstance {
            feature: feature_name.to_string(),
            row_genesis_hash: row.genesis_hash,
            genesis_hash: genesis_hash.to_string(),
        });
    }
    Ok(row)
}

/// #3277 S5: `H_ENABLE` for the live 777/testnet chain — the height at which
/// [`ConsensusFeature::ScheduleExtension`] activates there (owner ruling D3:
/// at least tip + 3000 at the time the B3 node release merges).
///
/// PLACEHOLDER until the team lead reads the live tip right before S5
/// merges. `u64::MAX` is fail-safe: the gate never opens, so a binary that
/// somehow shipped with it behaves exactly like one without the bootstrap.
/// `h_enable_777_is_set_before_merge` fails while it is the placeholder.
pub const H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE: u64 = 29_800;

/// One binary-carried activation for a chain cut before it could schedule
/// the feature in genesis (#3277 S5, design §2 option A′).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyChainBootstrap {
    /// The chain identity the row applies to; also the identity the
    /// schedule-extension authority binds extensions to on that chain.
    pub chain_id: &'static str,
    pub network_id: &'static str,
    /// Lowercase hex of the chain's durable block-1 hash — THE key. A re-cut
    /// chain reusing the id has a different block 1 and gets nothing.
    pub genesis_hash: &'static str,
    pub feature: ConsensusFeature,
    /// The first height at which `feature` applies.
    pub height: u64,
}

/// The per-chain bootstrap table (#3277 S5). The ONLY binary-carried
/// activation on a live chain, existing once, to bootstrap: every later
/// activation goes through a signed extension. Merged into the BASE feature
/// schedule beside genesis wherever the base is resolved from block 1.
/// Append-only.
pub const LEGACY_CHAIN_BOOTSTRAP: &[LegacyChainBootstrap] = &[LegacyChainBootstrap {
    chain_id: "777",
    network_id: "testnet",
    genesis_hash: "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8", // pragma: allowlist secret -- public genesis hash
    feature: ConsensusFeature::ScheduleExtension,
    height: H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE,
}];

/// The bootstrap rows for the chain whose block 1 hashes to `block1_hash`
/// (lowercase hex; a `0x` prefix and case are tolerated).
pub fn legacy_chain_bootstrap_for(block1_hash: &str) -> Vec<&'static LegacyChainBootstrap> {
    bootstrap_rows_for(LEGACY_CHAIN_BOOTSTRAP, block1_hash)
}

fn bootstrap_rows_for<'a>(
    rows: &'a [LegacyChainBootstrap],
    block1_hash: &str,
) -> Vec<&'a LegacyChainBootstrap> {
    let wanted = block1_hash.trim().trim_start_matches("0x").to_ascii_lowercase();
    rows.iter().filter(|row| row.genesis_hash == wanted).collect()
}

/// `genesis` with this chain's [`LEGACY_CHAIN_BOOTSTRAP`] rows merged in
/// (#3277 S5). Pure. A row whose feature genesis already schedules is an
/// error, never an override: the bootstrap exists only for chains whose
/// genesis could not schedule it.
pub fn with_legacy_chain_bootstrap(
    genesis: FeatureSchedule,
    block1_hash: &str,
) -> Result<FeatureSchedule, ActivationTableError> {
    with_bootstrap_rows(genesis, block1_hash, LEGACY_CHAIN_BOOTSTRAP)
}

/// [`with_legacy_chain_bootstrap`] over an explicit table, so the merge is
/// testable at heights the committed table does not use.
pub fn with_bootstrap_rows(
    genesis: FeatureSchedule,
    block1_hash: &str,
    table: &[LegacyChainBootstrap],
) -> Result<FeatureSchedule, ActivationTableError> {
    let rows = bootstrap_rows_for(table, block1_hash);
    if rows.is_empty() {
        return Ok(genesis);
    }
    let mut entries = genesis.entries().to_vec();
    for row in rows {
        entries.push((row.feature.name().to_string(), row.height));
    }
    FeatureSchedule::from_entries(entries)
}

/// The faucet sink in force at `height` (#3361): the genesis-recorded
/// `faucet_address` if any; otherwise the latest-declared row of `bindings`
/// whose `genesis_chain_id` matches and whose feature `is_active` at
/// `height` (an unbind row, or an unparseable address, resolves to `None`
/// rather than falling through to an older row); otherwise `None`.
///
/// Pure: the caller passes the recorded genesis config, the height the block
/// is applied at, and the feature predicate, so producer, applier and replay
/// resolve identically. Consensus passes [`LEGACY_FAUCET_BINDINGS`] and
/// [`feature_active`].
pub fn resolve_faucet_sink(
    config: Option<&GenesisEmissionConfig>,
    height: u64,
    bindings: &[LegacyFaucetBinding],
    is_active: impl Fn(ConsensusFeature, u64) -> bool,
) -> Option<crate::common::types::Address> {
    let config = config?;
    if let Some(address) = config.faucet_address {
        return Some(address);
    }
    bindings
        .iter()
        .rev()
        .filter(|binding| binding.genesis_chain_id == config.chain_id)
        .find(|binding| is_active(binding.feature, height))?
        .address
        .and_then(|address| crate::common::types::Address::from_hex(address).ok())
}

/// Key under which the genesis block records its network id.
pub const GENESIS_NETWORK_ID_KEY: &str = "network_id";
/// The canonical public-testnet predicate (#2523, Frank's #2540 review).
///
/// `network_id == "testnet"`, trimmed and case-insensitive, is already how this
/// repository decides that fail-closed genesis policy applies — see
/// `genesis_producers::network_is_testnet`, which now delegates here rather than
/// keeping a second copy. It is exposed from `shared-protocol` because consensus
/// needs the same answer: `native-chain`'s copy lives in a binary and cannot be
/// reached from the block-1 gate.
///
/// Deliberately a **closed** comparison, not a prefix or contains match. This
/// gates a refusal that stops a genesis cut, so "testnet-2" or "my-testnet" must
/// be a different network rather than silently inheriting public-testnet policy.
pub fn network_is_public_testnet(network_id: Option<&str>) -> bool {
    network_id.is_some_and(|id| id.trim().eq_ignore_ascii_case("testnet"))
}

/// Key under which the genesis block records the chain's signing protocol
/// version (#2123).
///
/// This is the **chain-wide** boundary. A node does not get to decide its own
/// protocol version; it reads the one the chain was created with. See
/// [`resolve_active_version`].
pub const GENESIS_PROTOCOL_VERSION_KEY: &str = "protocol_version";

/// Process-wide active protocol version, installed once at boot.
///
/// Deliberately private: the only writer is
/// [`assert_startup_protocol_compatibility`].
static ACTIVE_VERSION: OnceLock<SigningProtocolVersion> = OnceLock::new();

/// Version of the signature-preimage and block-hash construction.
///
/// This is a *consensus* version, not a software version. Changing it changes
/// the bytes every node signs and hashes, so it moves only at an explicit,
/// coordinated boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SigningProtocolVersion {
    /// Legacy construction. Preimages carry no chain id and no version; block
    /// hashes carry no network magic. Byte-for-byte what the chain does today.
    #[default]
    V1,
    /// Chain-and-version bound construction (#2123). Preimages are
    /// `DOMAIN_TAG || chain_id || network_id || protocol_version || body`;
    /// block hashes fold in network magic (#2066).
    V2,
}

impl SigningProtocolVersion {
    /// Numeric form folded into preimages.
    pub fn as_u32(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }

    /// Parse from a configuration string. Unrecognised input is `None` — the
    /// caller decides whether that is a hard error or a fall back to V1.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "1" | "v1" | "V1" => Some(Self::V1),
            "2" | "v2" | "V2" => Some(Self::V2),
            _ => None,
        }
    }

    /// Process-wide active version.
    ///
    /// **Reads no environment.** It returns whatever
    /// [`SigningProtocolVersion::install_active`] installed, and [`Self::V1`] if
    /// nothing did.
    ///
    /// This is the correction from the review of PR #2232. Previously this
    /// resolved straight from `WRFCOIN_SIGNING_PROTOCOL_VERSION`, which meant a
    /// single node could reach V2 — and change its block hashes — purely from
    /// local configuration, while its peers and external signers stayed V1.
    /// That is the #2076 asymmetry. Now the only route to V2 is
    /// [`assert_startup_protocol_compatibility`], which requires genesis to pin
    /// it, so no node can move itself.
    pub fn active() -> Self {
        *ACTIVE_VERSION.get().unwrap_or(&SigningProtocolVersion::V1)
    }

    /// Process-wide active version, with the uninstalled state representable
    /// (#2275).
    ///
    /// [`SigningProtocolVersion::active`] answers V1 both when V1 was
    /// genuinely installed by the startup gate and when **nothing has been
    /// installed yet** — and callers cannot tell the difference. For most
    /// consumers that is fine. It is not fine for consumers whose answer is
    /// advertised to peers or committed into signed material: a node that runs
    /// them before [`assert_startup_protocol_compatibility`] would advertise
    /// V1 while later signing V2, and a fleet that is uniformly mis-ordered
    /// would agree with itself all the way into the #2076 failure (signatures
    /// that verify nowhere, nothing erroring).
    ///
    /// Correctness-critical consumers (the p2p genesis handshake is the
    /// canonical one) must call this and **refuse to proceed on `None`**
    /// rather than defaulting. Returns `Some` only after
    /// [`SigningProtocolVersion::install_active`] has run.
    pub fn try_active() -> Option<Self> {
        ACTIVE_VERSION.get().copied()
    }

    /// Install the process-wide active version. Called once, at boot, by
    /// [`assert_startup_protocol_compatibility`] after it has verified the
    /// version is chain-wide rather than locally chosen.
    ///
    /// Returns the value actually in force: installing is idempotent and the
    /// **first** install wins, so a later caller cannot move a running node
    /// between versions.
    pub fn install_active(version: Self) -> Self {
        *ACTIVE_VERSION.get_or_init(|| version)
    }
}

/// Why an activation schedule was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationTableError {
    /// The schedule does not start at height 0, so some heights have no answer.
    MissingGenesisEntry { first_height: u64 },
    /// Heights are not strictly increasing, so a height could match twice.
    NonIncreasingHeight { previous: u64, next: u64 },
    /// A later height selects an *older* version. Protocol versions only move
    /// forward; going backwards would re-interpret already-sealed bytes.
    VersionRegression {
        height: u64,
        previous: SigningProtocolVersion,
        next: SigningProtocolVersion,
    },
    /// A feature schedule names a feature this binary registers but has no behaviour for
    /// ([`ConsensusFeature::has_behaviour`], #3430). Refused at genesis resolution so a node
    /// cannot boot a chain whose schedule promises behaviour the binary does not implement.
    FeatureWithoutBehaviour { feature: String },
    /// A feature schedule names the same feature twice (#3092).
    DuplicateFeature { feature: String },
}

impl fmt::Display for ActivationTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingGenesisEntry { first_height } => write!(
                f,
                "activation schedule must start at height 0, but its first entry is at {first_height}"
            ),
            Self::NonIncreasingHeight { previous, next } => write!(
                f,
                "activation heights must strictly increase, but {next} follows {previous}"
            ),
            Self::VersionRegression {
                height,
                previous,
                next,
            } => write!(
                f,
                "activation at height {height} regresses from {previous} to {next}; \
                 protocol versions only move forward"
            ),
            Self::FeatureWithoutBehaviour { feature } => write!(
                f,
                "feature {feature:?} is registered but this binary has no behaviour for it yet; \
                 refusing a schedule that names it"
            ),
            Self::DuplicateFeature { feature } => write!(
                f,
                "feature {feature:?} is scheduled more than once; one feature activates at one height"
            ),
        }
    }
}

impl std::error::Error for ActivationTableError {}

/// A height-indexed protocol-version schedule (#1396).
///
/// This is the mechanism that lets a consensus version change **without a
/// coordinated fleet restart**. [`SigningProtocolVersion::active`] answers the
/// same value for every height in the process, so today a version move requires
/// every node to restart together — and, more subtly, a node replaying history
/// evaluates *old* blocks under the *current* version. That is masked only
/// because V1 → V2 happens at a fresh genesis, where no history exists at the
/// other version. The first activation at a height on a live chain would break
/// WAL replay and reorg re-apply without this.
///
/// **Deliberately not stored in [`crate::blockchain::state::State`].** The
/// schedule is consumed by `Block::calculate_hash`, which is used to compute
/// state; a version derived from the state it helps compute is circular.
/// Reading the *parent's* state escapes the circle but would force every
/// hashing call site to acquire state. The schedule is therefore immutable
/// genesis content, like `chain_id` and `network_id`. A mutable, rotatable
/// thing — the producer set — correctly belongs in `state_root` instead; the
/// two differ for that reason, not by inconsistency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationTable {
    /// Ascending by height, first entry at height 0, versions non-decreasing.
    entries: Vec<(u64, SigningProtocolVersion)>,
}

impl Default for ActivationTable {
    /// The identity schedule: [`SigningProtocolVersion::V1`] from genesis
    /// onward, i.e. byte-for-byte what the chain does with no schedule at all.
    fn default() -> Self {
        Self {
            entries: vec![(0, SigningProtocolVersion::V1)],
        }
    }
}

impl ActivationTable {
    /// Validate and build a schedule.
    ///
    /// An empty input is the [`Default`] identity schedule rather than an
    /// error: "no schedule pinned" and "V1 from genesis" are the same chain.
    pub fn from_entries(
        entries: Vec<(u64, SigningProtocolVersion)>,
    ) -> Result<Self, ActivationTableError> {
        if entries.is_empty() {
            return Ok(Self::default());
        }
        if entries[0].0 != 0 {
            return Err(ActivationTableError::MissingGenesisEntry {
                first_height: entries[0].0,
            });
        }
        for window in entries.windows(2) {
            let (prev_height, prev_version) = window[0];
            let (next_height, next_version) = window[1];
            if next_height <= prev_height {
                return Err(ActivationTableError::NonIncreasingHeight {
                    previous: prev_height,
                    next: next_height,
                });
            }
            if next_version < prev_version {
                return Err(ActivationTableError::VersionRegression {
                    height: next_height,
                    previous: prev_version,
                    next: next_version,
                });
            }
        }
        Ok(Self { entries })
    }

    /// The scheduled steps, ascending by height.
    pub fn entries(&self) -> &[(u64, SigningProtocolVersion)] {
        &self.entries
    }

    /// The first height at which `version` (or a later one) is in force —
    /// the activation height of that version step — or `None` if the
    /// schedule never reaches it (#3092).
    pub fn activation_height_of(&self, version: SigningProtocolVersion) -> Option<u64> {
        self.entries
            .iter()
            .find(|(_, v)| *v >= version)
            .map(|(h, _)| *h)
    }

    /// The version in force at `height`.
    ///
    /// Total: the constructor guarantees an entry at height 0, so every height
    /// has an answer. Integer-only — no wall-clock input, so every node agrees.
    pub fn version_at(&self, height: u64) -> SigningProtocolVersion {
        // Last entry whose activation height is <= `height`.
        match self.entries.binary_search_by_key(&height, |(h, _)| *h) {
            Ok(exact) => self.entries[exact].1,
            // `insertion` is never 0: entries[0].0 == 0 <= height for all height.
            Err(insertion) => self.entries[insertion - 1].1,
        }
    }
}

/// Process-wide activation schedule, installed once at boot from genesis.
static ACTIVE_TABLE: OnceLock<ActivationTable> = OnceLock::new();

/// Install the process-wide activation schedule. First install wins, so a later
/// caller cannot move a running node between schedules — same contract as
/// [`SigningProtocolVersion::install_active`].
pub fn install_activation_table(table: ActivationTable) -> &'static ActivationTable {
    ACTIVE_TABLE.get_or_init(|| table)
}

/// The installed schedule, if the startup gate has run.
///
/// `None` means *no schedule established yet* — distinct from "V1 schedule".
/// Consumers whose answer is advertised to peers or committed into signed
/// material must refuse to proceed on `None` rather than defaulting, exactly as
/// [`SigningProtocolVersion::try_active`] requires (the p2p genesis handshake is
/// the canonical such caller).
pub fn try_activation_table() -> Option<&'static ActivationTable> {
    ACTIVE_TABLE.get()
}

/// The protocol version in force at `height`.
///
/// **V1 byte-identity:** with no schedule installed this falls back to
/// [`SigningProtocolVersion::active`] for every height, so a tree that never
/// installs a schedule behaves byte-for-byte as it does today. That property is
/// the migration's safety net and is pinned by test.
pub fn protocol_version_at(height: u64) -> SigningProtocolVersion {
    match ACTIVE_TABLE.get() {
        Some(table) => table.version_at(height),
        None => SigningProtocolVersion::active(),
    }
}

/// Key under which genesis records the protocol activation schedule (#1396).
///
/// Optional. Absent means "the scalar [`GENESIS_PROTOCOL_VERSION_KEY`] applies
/// at every height", which is exactly the pre-#1396 chain — so an existing
/// genesis needs no rewrite and resolves to identical bytes.
///
/// Format: comma-separated `height:version` pairs, ascending, first at 0 —
/// `0:v1,1000:v2`. Integer heights and the same version spellings
/// [`SigningProtocolVersion::parse`] accepts. Deliberately a flat string: it
/// rides the existing genesis-metadata map with no schema change, and it is
/// short enough for an operator to read off a genesis block by eye.
pub const GENESIS_ACTIVATION_SCHEDULE_KEY: &str = "protocol_activation_schedule";

/// Cut-time input naming the activation schedule block 1 will RECORD (#3092).
///
/// Read once, by the node producing block 1 of an uncut chain, and written
/// into genesis metadata under [`GENESIS_ACTIVATION_SCHEDULE_KEY`] by
/// `stamp_genesis_chain_metadata`. Every later boot — this node's and every
/// peer's — reads the schedule from the block, never from this variable: the
/// same "cut-time input to the writer, never a validation reference" rule the
/// #2523 oracle declaration follows. On a chain that already has blocks the
/// variable is ignored.
pub const GENESIS_ACTIVATION_SCHEDULE_ENV: &str = "WRFCOIN_GENESIS_ACTIVATION_SCHEDULE";

/// A named consensus rule change that activates at a scheduled height (#3092,
/// finishing #1396).
///
/// One variant per scheduled change. A binary that lacks a variant for a
/// feature its genesis schedules cannot follow the chain past that height —
/// which is exactly what [`FeatureSchedule::first_unknown`] reports and #3093
/// turns into a clean halt. Names are stable identifiers (`snake_case`) and
/// are what the genesis string carries, so an old binary can still PARSE a
/// schedule naming a feature it does not implement.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusFeature {
    /// The chain-and-version bound signing construction
    /// ([`SigningProtocolVersion::V2`]). Scheduled through the `vN` version
    /// tokens of the genesis string, never as a feature name, so that
    /// [`protocol_version_at`] and [`feature_active`] have one source.
    SigningV2,
    /// #2226: `DomainSeparated` preimages for every signed object.
    DomainSeparatedPreimages,
    /// #2168: per-account authorization dispatch. From its activation a spend
    /// may be authorized by a rule the ACCOUNT declared, instead of only by
    /// the rule its transaction type implies. Every policy story in epic
    /// #2167 — passkeys, session keys, sponsorship, recovery, delegation —
    /// dispatches through this.
    AccountAuthorizationDispatch,
    /// #2172: guardian-based account recovery
    /// (`shared-protocol/src/accounts/recovery.rs`). Gates every
    /// policy/request/execute action in that module and the
    /// `StateTransition::ApplyAccountRecovery` variant that delegates to it,
    /// so `State::account_recovery` is provably empty on any chain that has
    /// not scheduled this feature.
    SocialRecovery,
    /// #2170: temporary, scoped session keys
    /// (`shared-protocol/src/accounts/session_keys.rs`). Gates every
    /// register/revoke/use action in that module and the
    /// `StateTransition::ApplySessionKeyAction` variant that delegates to
    /// it, so `State::session_keys` is provably empty on any chain that has
    /// not scheduled this feature.
    SessionKeys,
    /// #2143: per-block verification-work budget. From its activation a block
    /// whose transactions force more than `block_verify_budget` verification
    /// units on a validator is invalid, so a proposer cannot make every node
    /// burn unbounded CPU on a legal-looking block.
    VerifyUnitPricing,
    /// #2169: a transparent transfer may be authorised by a WebAuthn P-256
    /// passkey assertion bound to the debited account
    /// (`accounts::passkey`). Before activation such a transfer is refused
    /// exactly as it is today; the ed25519 path is untouched at every height.
    PasskeyP256Auth,
    /// #2171: paymaster / gas sponsorship with its own abuse bounds
    /// (`shared-protocol/src/accounts/paymaster.rs`). Gates every
    /// stake/unstake/sponsor action in that module and the
    /// `StateTransition::ApplyPaymasterAction` variant that delegates to
    /// it, so `State::paymasters` is provably empty on any chain that has
    /// not scheduled this feature.
    Paymaster,
    /// #3131: the reward-tier gross-outlier guard
    /// (`weather::reference::ReferenceComparison::tier_with_gross_guard`).
    /// From its activation, a reading whose single Outlier variable is
    /// beyond that variable's `gross_max` tiers `Outlier` (0.1×) instead of
    /// the #3098 fold's `Degraded` (0.5×). The fold runs in the oracle
    /// attester, not in block application: nodes store the signed confidence
    /// they receive. The node serves this feature's state to attesters on
    /// `GET /transactions/oracle/pending`, so honest attesters switch rules at
    /// one agreed height instead of on redeploy.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule, so
    /// on that chain this is inactive at every height and never triggers the
    /// #3093 halt. It is activated by a #3277 live schedule extension (story
    /// S8; planned for E0 together with #3270), or by a future genesis.
    RewardGrossOutlierGuard,
    /// #3285: the attester pairs a reading's sea-level pressure with the
    /// reference station's altimeter setting (NWS `barometricPressure`)
    /// instead of the raw `pressure_hpa`
    /// (`weather::station_reference::pair_station_reference_sea_level`).
    /// From its activation, when the reveal `data` carries
    /// `pressure_sea_level_hpa` with `pressure_kind: "station"` and the
    /// station's registration carries `elevation_meters`, the attester
    /// recomputes the altimeter setting from the raw `pressure_hpa` and that
    /// elevation, and pairs the submitted sea-level value only if the two
    /// agree within 1 hPa. A `"relative"` pressure is already sea-level and is
    /// never reduced again. Otherwise (owner rulings): a station WITH a
    /// registered elevation gets no raw pressure residual (no claim, no known
    /// kind, or a mismatch), and a published, checkable `pressure_hpa` is
    /// scored as an unverified `Outlier` variable in the tier fold, so
    /// withholding the claim never tiers better than honesty; a station
    /// without an elevation keeps the raw pairing. The attester records why. The value travels only as an
    /// extra key in the reveal JSON (hashed by `compute_commit_id` like every
    /// other key); `WeatherObservation`, `State` and the checkpoint layout are
    /// unchanged. Like #3131, the pairing runs in the oracle attester, not in
    /// block application: nodes store the signed confidence they receive. The
    /// node serves this feature's state on `GET /transactions/oracle/pending`
    /// so honest attesters switch at one agreed height instead of on redeploy.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule, so
    /// on that chain this is inactive at every height and never triggers the
    /// #3093 halt. #3277 ruling D7 fixes the first live schedule extension as
    /// #3131 + #3270 only, so this is activated by a later #3277 extension, or
    /// by a future genesis.
    ReferenceSeaLevelPressure,
    /// #3214: a data-quality `SlashingTrigger` raised by `ApplyReveal` is
    /// bound to the station's registered, funded owner account
    /// (`State::station_slash_identity`) instead of the `station_id`
    /// placeholder, so the finalization debit reaches a real account. From
    /// its activation, a violating reveal whose station has no resolvable,
    /// funded owner fails closed with `StateError::SlashingApplication` and
    /// enqueues nothing. Before activation `ApplyReveal` is the pre-#3214
    /// path byte for byte: the trigger keeps `station_id` as its identity.
    ///
    /// Ships UNSCHEDULED (owner decision 2026-09-25): the live 777/testnet
    /// genesis records no schedule, so on that chain this is inactive at
    /// every height and never triggers the #3093 halt. It appears in no
    /// genesis, default schedule or cut pin; activation is an owner
    /// governance decision (a #3277 live schedule extension or a future
    /// genesis).
    StationOwnerSlashIdentity,
    /// #3361 option A (F1): the ADR-003 faucet slice of a chain whose genesis
    /// recorded no `faucet_address` is paid to the address its
    /// [`LEGACY_FAUCET_BINDINGS`] row commits, from this feature's
    /// activation. The address is committed per-chain data compiled into the
    /// binary; the governance-signed #3277 schedule extension only decides
    /// WHEN it activates. Resolved inside the existing `ApplyFaucetReward`
    /// arm by [`resolve_faucet_sink`] — no transaction type, `State` field,
    /// fold section, checkpoint or snapshot change. A genesis-recorded
    /// address always wins.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule, so
    /// on that chain this is inactive at every height and never triggers the
    /// #3093 halt. It appears in no genesis, default schedule or cut pin; the
    /// owner ruled (2026-09-28, #3361 Q2) that it is activated by its own
    /// #3277 extension, E0f, after E0.
    FaucetSinkBinding,
    /// #3270: halt-aware weather forward-skew allowance. From its activation
    /// a reading may be as far ahead of the station's last accepted reading as
    /// the LARGER of the block-count allowance (`elapsed_blocks × target block
    /// time + grace`, the only rule before activation) and the consensus
    /// block-time gap between the block that accepted that reading and the
    /// current block (`+ grace`). Both timestamps are block-header timestamps,
    /// never the wall clock. On cadence the block-count term dominates and
    /// nothing changes; after a halt the block-time term lets honest readings
    /// through instead of rejecting them for ~halt_hours blocks. Records the
    /// accepting block's timestamp per station in
    /// `State::station_weather_accepted_block_unix`, which is provably empty on
    /// any chain that has not scheduled this feature.
    WeatherSkewBlockTimeAllowance,
    /// #3277 S5: the live feature-schedule extension TRANSACTION itself (V1).
    /// Before its activation every `FeatureScheduleExtension` is invalid; from
    /// it, governance-signed extensions append future activations to the
    /// schedule (design §1.2–§1.5). A new chain schedules it in its genesis
    /// string; the legacy live chain gets it only from
    /// [`LEGACY_CHAIN_BOOTSTRAP`], keyed by its block-1 hash.
    ScheduleExtension,
    /// #3244: scorer-side persistence-relative skill and digest-bound strategy
    /// metadata. Nodes continue trusting authorized oracle receipts; the
    /// forecast tool follows the existing active_features view. UNSCHEDULED
    /// and not live-schedulable pending reviewed activation/scorer rollout.
    ForecastSkillBaseline,
    /// #3345: a station's owner changes only with the CURRENT owner's
    /// consent. From its activation a governance-signed
    /// `StationRegistration` whose `owner_address` differs from the owner
    /// already registered for that `station_id` is refused as an inert,
    /// deterministic no-op (`StateTransition::ApplyStationRegistrationWithOwnerConsent`):
    /// governance alone can no longer reassign a station, and so can no
    /// longer redirect its rewards (payee = registered owner since #2743).
    /// Same-owner re-registrations (metadata corrections) apply exactly as
    /// before. The shared owner-ledger extractor handles owner-signed
    /// transfers and recovery under this gate, as specified in
    /// `docs/architecture/station-ownership-transfer.md`.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule,
    /// so on that chain this is inactive at every height and never triggers
    /// the #3093 halt. It appears in no genesis, default schedule or cut pin;
    /// activation is an owner governance decision (a #3277 live schedule
    /// extension or a future genesis).
    StationOwnerConsent,
    /// #3347: rewards honour the station's `is_active` flag. From its
    /// activation a contribution or forecast reveal whose station is not
    /// registered-and-active is recorded as a zero-amount decision (the same
    /// fail-closed shape as the #2743 unresolvable-owner refusal), and a
    /// deferred (#2464) reward of such a station is not settled while it
    /// stays inactive. Before activation `is_active` is read by the reward
    /// path only through the data-desert multiplier, byte for byte as on main
    /// — an inactive Real station IS paid there
    /// (`tests/station_active_reward_gate_3347.rs` characterises that).
    /// The owner-signed report-stolen freeze and self-(de)activation that
    /// drive this flag are handled by the shared owner-ledger extractor under
    /// `station_owner_actions`, as specified in
    /// `docs/architecture/station-freeze-and-deactivation.md`.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule,
    /// so on that chain this is inactive at every height and never triggers
    /// the #3093 halt. It appears in no genesis, default schedule or cut pin;
    /// activation is an owner governance decision (a #3277 live schedule
    /// extension or a future genesis).
    StationActiveRewardGate,
    /// #3347: owner-signed station actions — report stolen/lost (freeze),
    /// unfreeze, self-deactivate, reactivate — applied to the per-station
    /// owner ledger (`State::station_owner_ledger`). From its activation a
    /// frozen or owner-deactivated station earns nothing (planned rewards are
    /// held and released on unfreeze/reactivation), and a FROZEN station's
    /// reveals are neither rewarded nor penalised (ZK-receipt slashing,
    /// #3214 data-quality slashing and the collusion reputation penalty are
    /// skipped; owner ruling Q5 2026-09-27). Scheduled together with
    /// `station_owner_consent` (they share the ledger nonce); design:
    /// `docs/architecture/station-freeze-and-deactivation.md`.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule,
    /// so on that chain this is inactive at every height and never triggers
    /// the #3093 halt. It appears in no genesis, default schedule or cut pin;
    /// activation is an owner governance decision (a #3277 live schedule
    /// extension or a future genesis).
    StationOwnerActions,
    /// #3345 (permanent fix of the registration-authority gap): a station
    /// registration must carry a SIGNED `expected_nonce`, bound into the
    /// governance preimage, that equals the station's current owner-ledger
    /// nonce (0 when it has no entry). From activation:
    /// - the v1 governance-signed registration format is REFUSED entirely
    ///   (inert), so no already-mined v1 payload can be applied again;
    /// - a v2 registration (`tx_kind = station_registration_v2`) applies only
    ///   when `expected_nonce` matches, and applying it ADVANCES the station's
    ///   `owner_nonce` (the same per-station counter owner actions, transfers
    ///   and recovery consume), so the same signed payload cannot apply twice.
    ///   An owner action in between invalidates an in-flight registration.
    ///
    /// Appended LAST so earlier enum wire ordinals are unchanged. Before
    /// activation a v2 payload is inert and v1 behaves exactly as on main.
    ///
    /// Ships UNSCHEDULED: the live 777/testnet genesis records no schedule, so
    /// on that chain this is inactive at every height and never triggers the
    /// #3093 halt; activation is an owner governance decision (the format-11
    /// upgrade).
    StationRegistrationNonce,
    /// #3430: signed Stake/Unstake submission with a persistent account nonce
    /// (design: `docs/decisions/stake-submission-round11.md`, map:
    /// `docs/decisions/stake-submission-implementation-map.md`). REGISTRATION ONLY in this slice:
    /// the variant, its stable name and its `KNOWN_FEATURES` pairing exist so the binary can name
    /// the feature; no behaviour is wired to it yet, it is NOT schedulable by a live extension,
    /// and it appears in no genesis string, default schedule or live-schedule file. With it
    /// unscheduled (every chain today, including 777) the B3.1 refusal and the historical
    /// Stake/Unstake behaviour are byte-identical to before. The behaviour lands in a later gated
    /// slice (`ProcessStakeTransaction`); a schedule must never name this feature before that
    /// slice is in the same release.
    StakeSubmissionNonce,
    /// #3398 Option C (B4): the committed slashing-evidence report path. From
    /// its activation, a staked reporter may carry a `SlashEvidenceReport`
    /// transaction that debits the station owner snapshotted at commit time
    /// for a physical-range violation the producer pruned from a block.
    ///
    /// This slice registers the gate only. Capture, verification and apply
    /// land in later slices, so while this is active the report type is still
    /// refused (refuse by default, `SP_SLASH_EVIDENCE_APPLY_PENDING`); with it
    /// inactive the refusal is byte-identical to the type-only slice (#3422).
    /// It is NOT yet schedulable by a #3277 extension: that is flipped in the
    /// slice that implements apply, so an early schedule entry cannot arm a
    /// half-built path.
    ///
    /// Ships UNSCHEDULED and appears in no genesis, default schedule or cut
    /// pin; on the live 777/testnet chain it is inactive at every height.
    SlashEvidenceReports,
    /// #2848 slice 1: contracts only. No state, emission, transaction or replay
    /// behaviour; refuse genesis and live scheduling until all slices land.
    NodeShareSettlement,
    /// #3290: protocol-level forecaster key binding. From its activation a
    /// `ForecastCommit` is valid only when its sender and every output name
    /// the 20-byte address derived from the commit's `auth_pubkey`
    /// (`Transaction::forecast_commit_key_bound`). Before activation a forecast
    /// commit's authorization proves possession of SOME key, and any key can
    /// name any forecaster; that is the byte-identical behaviour while this is
    /// inactive. Enforced through one height-aware check
    /// (`Transaction::verify_commit_authorization_at_height`) at mempool
    /// admission, in the shared producer/sync block classifier (an unbound
    /// commit invalidates the block) and in transition extraction.
    /// Contribution commits are untouched at every height.
    ///
    /// Appended LAST so earlier enum wire ordinals are unchanged. Ships
    /// UNSCHEDULED and NOT live-schedulable: it appears in no genesis, default
    /// schedule or cut pin, and activation is a separate, owner-approved step.
    ForecasterKeyBinding,
    /// #3565 (white paper §4.6 / §9.2): one accepted `ContributionReveal` per
    /// registered station (the chain's device identity) per block, plus a
    /// late-data pay reduction keyed on the observation's age at its
    /// including block (`late_data_multiplier_bps`). Both read only chain data:
    /// the reveal's committed `timestamp` and the block HEADER timestamp,
    /// never a node clock. A second reveal for the same station in one block is
    /// refused by `classify_block_transaction` (so `verify_block` refuses the
    /// block); the producer defers it to a later block rather than evicting it.
    /// A contribution commit for a registered station must be authorized by its
    /// registered owner key (`State` RegisterCommit), so only the owner can
    /// occupy the slot. No mempool rule (the mempool cannot check ownership).
    /// NOT schedulable by a live extension in this slice; activation is a
    /// separate owner decision.
    /// Unscheduled, every path is byte-identical to before.
    DeviceSubmissionLimit,
    /// #3591: the low-quality-streak replay (`derive_low_quality_streak_transitions`
    /// in the transition extractor) applies the block's `BeginNodeServiceBlock`
    /// between the governance and launch transitions, the order the producer
    /// applies them (`apply_launch_state_transitions_on_state` begins the node
    /// service block before the launch batch). This changes derived streak
    /// transitions, and so `state_root`, on some blocks; hence the gate.
    ///
    /// Appended LAST so earlier enum wire ordinals are unchanged. Ships
    /// UNSCHEDULED and NOT live-schedulable; inactive, the replay is byte for
    /// byte the pre-#3591 one.
    StreakReplayNodeService,
}

/// Activation admission contract: Refs #3527, all five hardening items, plus
/// S6a operator/gossip readiness and S6b registered production parity evidence.
/// No host setting or issue-status lookup can override an incomplete binary.
/// S6e admission requires the reviewed S6a–S6d prerequisite merge gate.
/// This flag installs no schedule and enables no feature by itself.
pub const NODE_SHARE_3527_AND_S6_COMPLETE: bool = true;

pub fn node_share_activation_ready(_node_genesis_hash: Option<&str>) -> bool {
    NODE_SHARE_3527_AND_S6_COMPLETE
}

impl ConsensusFeature {
    /// Every feature this binary implements, in declaration order.
    pub const ALL: &'static [ConsensusFeature] = &[
        Self::SigningV2,
        Self::DomainSeparatedPreimages,
        Self::AccountAuthorizationDispatch,
        Self::SocialRecovery,
        Self::SessionKeys,
        Self::VerifyUnitPricing,
        Self::PasskeyP256Auth,
        Self::Paymaster,
        Self::RewardGrossOutlierGuard,
        Self::ReferenceSeaLevelPressure,
        Self::StationOwnerSlashIdentity,
        Self::FaucetSinkBinding,
        Self::WeatherSkewBlockTimeAllowance,
        Self::ScheduleExtension,
        Self::ForecastSkillBaseline,
        Self::StationOwnerConsent,
        Self::StationActiveRewardGate,
        Self::StationOwnerActions,
        Self::StationRegistrationNonce,
        Self::StakeSubmissionNonce,
        Self::SlashEvidenceReports,
        Self::NodeShareSettlement,
        Self::ForecasterKeyBinding,
        Self::DeviceSubmissionLimit,
        Self::StreakReplayNodeService,
    ];

    /// The stable name the genesis schedule carries.
    pub const fn name(self) -> &'static str {
        match self {
            Self::SigningV2 => "signing_v2",
            Self::DomainSeparatedPreimages => "domain_separated_preimages",
            Self::AccountAuthorizationDispatch => "account_authorization_dispatch",
            Self::SocialRecovery => "social_recovery",
            Self::SessionKeys => "session_keys",
            Self::VerifyUnitPricing => "verify_unit_pricing",
            Self::PasskeyP256Auth => "passkey_p256_auth",
            Self::Paymaster => "paymaster",
            Self::RewardGrossOutlierGuard => "reward_gross_outlier_guard",
            Self::ReferenceSeaLevelPressure => "reference_sea_level_pressure",
            Self::StationOwnerSlashIdentity => "station_owner_slash_identity",
            Self::FaucetSinkBinding => "faucet_sink_binding",
            Self::WeatherSkewBlockTimeAllowance => "weather_skew_block_time_allowance",
            Self::StationOwnerConsent => "station_owner_consent",
            Self::StationActiveRewardGate => "station_active_reward_gate",
            Self::StationOwnerActions => "station_owner_actions",
            Self::StationRegistrationNonce => "station_registration_nonce",
            Self::ScheduleExtension => crate::governance::schedule_extension::SCHEDULE_EXTENSION_FEATURE,
            Self::ForecastSkillBaseline => "forecast_skill_baseline",
            Self::StakeSubmissionNonce => "stake_submission_nonce",
            Self::SlashEvidenceReports => "slash_evidence_reports",
            Self::NodeShareSettlement => "node_share_settlement",
            Self::ForecasterKeyBinding => "forecaster_key_binding",
            Self::DeviceSubmissionLimit => "device_submission_limit",
            Self::StreakReplayNodeService => "streak_replay_node_service",
        }
    }

    /// Whether this binary lets a live schedule extension (#3277) name this
    /// feature: mempool and producer admission and the signing CLI refuse an
    /// extension naming a feature for which this is `false`.
    ///
    /// ADMISSION POLICY, NOT CONSENSUS (design §1.3): a block that already
    /// carries such an extension is still valid, exactly like one naming a
    /// feature this binary lacks, so flipping a flag in a later release never
    /// forks a node that disagrees. Exhaustive on purpose: a new feature
    /// must state whether it may be scheduled live.
    ///
    /// `false` today:
    /// - `signing_v2`: a signing version is scheduled as a version step,
    ///   never as a feature name (V5 refuses the name outright).
    /// - `paymaster`: waits on #3169/#3170 plus its wiring (design §3, D4).
    /// - `social_recovery`, `session_keys`: nothing emits their transitions
    ///   and their known blockers are open (design §3A items 8–9); each is
    ///   flipped by the release that wires it.
    /// - `slash_evidence_reports` (#3398): the gate is registered ahead of the
    ///   capture/verify/apply slices; flipped by the slice that implements apply.
    ///
    /// Depends on the node's chain instance (`node_genesis_hash`, the
    /// lowercase hex of its durable block-1 hash; `None` when it has none):
    /// - `faucet_sink_binding` (#3361 F3): schedulable ONLY on the chain
    ///   instance its [`LEGACY_FAUCET_BINDINGS`] row names
    ///   ([`faucet_binding_row_for`]); refused on every other chain and on a
    ///   node with no block-1 hash. Its signing additionally needs the faucet
    ///   key's proof of possession (the sign CLI and library signers).
    /// - `schedule_extension`: the gate itself comes only from genesis or
    ///   [`LEGACY_CHAIN_BOOTSTRAP`]; never schedulable by an extension.
    pub fn schedulable(self, node_genesis_hash: Option<&str>) -> bool {
        match self {
            Self::FaucetSinkBinding => {
                faucet_binding_row_for(self.name(), node_genesis_hash).is_ok()
            },
            Self::SigningV2
            | Self::Paymaster
            | Self::SocialRecovery
            | Self::SessionKeys
            | Self::ForecastSkillBaseline
            // Scheduled by genesis or the per-chain bootstrap, never by an
            // extension (V6 refuses it: it is always already scheduled).
            | Self::ScheduleExtension
            // #3430: registered but behaviourless until its gated slice lands; a later reviewed
            // change decides when governance may schedule it (B4, owner-gated).
            | Self::StakeSubmissionNonce
            // Refusal behaviour exists, but evidence apply is not implemented.
            | Self::SlashEvidenceReports
            // #3290: behaviour is wired and tested, but turning it on is a
            // separate, owner-approved release step.
            | Self::ForecasterKeyBinding
            // #3565: behaviour is wired, but activation is an owner decision.
            | Self::DeviceSubmissionLimit
            // #3591: wired and tested; scheduling is a separate owner step.
            | Self::StreakReplayNodeService => false,
            Self::NodeShareSettlement => node_share_activation_ready(node_genesis_hash),
            Self::DomainSeparatedPreimages
            | Self::AccountAuthorizationDispatch
            | Self::VerifyUnitPricing
            | Self::PasskeyP256Auth
            | Self::RewardGrossOutlierGuard
            | Self::ReferenceSeaLevelPressure
            | Self::StationOwnerSlashIdentity
            | Self::WeatherSkewBlockTimeAllowance
            | Self::StationOwnerConsent
            | Self::StationActiveRewardGate
            | Self::StationOwnerActions
            | Self::StationRegistrationNonce => true,
        }
    }

    /// Whether this binary implements behaviour for the feature, as opposed to only registering
    /// its name. Genesis resolution refuses a schedule naming a feature for which this is `false`
    /// ([`resolve_feature_schedule`]). Exhaustive on purpose: the behaviour slice of a new
    /// feature flips its own arm. Registration only (#3430): `stake_submission_nonce`.
    pub const fn has_behaviour(self) -> bool {
        match self {
            // S1 implements deterministic refusal even if a genesis names the gate.
            Self::SlashEvidenceReports => true,
            Self::StakeSubmissionNonce => false,
            Self::NodeShareSettlement => true,
            // #3290: admission, block classification and extraction all
            // enforce the binding once active.
            Self::ForecasterKeyBinding => true,
            // #3565: refusal + late-data reduction are implemented and gated.
            Self::DeviceSubmissionLimit => true,
            // #3591: the extractor's streak replay reads it.
            Self::StreakReplayNodeService => true,
            Self::SigningV2
            | Self::DomainSeparatedPreimages
            | Self::AccountAuthorizationDispatch
            | Self::SocialRecovery
            | Self::SessionKeys
            | Self::VerifyUnitPricing
            | Self::PasskeyP256Auth
            | Self::Paymaster
            | Self::RewardGrossOutlierGuard
            | Self::ReferenceSeaLevelPressure
            | Self::StationOwnerSlashIdentity
            | Self::FaucetSinkBinding
            | Self::WeatherSkewBlockTimeAllowance
            | Self::ScheduleExtension
            | Self::ForecastSkillBaseline
            // #3345/#3347 (merged in #3433): each of these three has real, feature-gated behaviour in
            // this binary, so a genesis schedule may name them and must not be refused.
            // StationOwnerConsent: refuses a non-consenting owner change via
            // ApplyStationRegistrationWithOwnerConsent (state.rs).
            | Self::StationOwnerConsent
            // StationActiveRewardGate: records a zero-amount decision for an unregistered/inactive
            // station in the reward path (proof_of_publishing.rs, feature_active gate).
            | Self::StationActiveRewardGate
            // StationOwnerActions: applies owner-signed freeze/unfreeze/(de)activate through the
            // shared owner-ledger extractor (station_owner.rs, feature_active gate).
            | Self::StationOwnerActions
            // #3486: StationRegistrationNonce has real gated behaviour in this binary (state.rs, transaction.rs):
            // once active a v1 registration is refused and a v2 registration applies only when its
            // expected_nonce matches, advancing the station's owner_nonce. A genesis schedule may name it.
            | Self::StationRegistrationNonce => true,
        }
    }

    /// Resolve a schedule name to a feature this binary implements.
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
}

#[cfg(test)]
mod forecast_skill_baseline_tests {
    use super::{ConsensusFeature, FeatureSchedule};

    #[test]
    fn feature_is_known_but_unscheduled_and_not_live_schedulable() {
        let feature = ConsensusFeature::ForecastSkillBaseline;
        assert_eq!(
            ConsensusFeature::parse("forecast_skill_baseline"),
            Some(feature)
        );
        assert!(super::super::protocol_support::KNOWN_FEATURES.contains(&feature.name()));
        assert!(!feature.schedulable(None));
        assert!(!feature.schedulable(Some("synthetic-genesis")));
        let schedule = FeatureSchedule::default();
        for height in [0, 100, u64::MAX] {
            assert!(!schedule.active(feature, height));
        }
    }

    #[test]
    fn appended_feature_preserves_every_legacy_enum_wire_ordinal() {
        for (index, feature) in ConsensusFeature::ALL.iter().enumerate() {
            assert_eq!(
                bincode::serialize(feature).unwrap(),
                (index as u32).to_le_bytes()
            );
        }
        assert_eq!(
            ConsensusFeature::ALL[13],
            ConsensusFeature::ScheduleExtension
        );
    }
}

impl fmt::Display for ConsensusFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The genesis activation string, parsed but not yet validated as a
/// schedule: the `vN` version steps and the named feature activations it
/// carries, in input order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedActivationSchedule {
    /// `height:vN` entries.
    pub versions: Vec<(u64, SigningProtocolVersion)>,
    /// `height:<feature_name>` entries. Names are NOT resolved against
    /// [`ConsensusFeature`] here, so a schedule naming a feature this binary
    /// does not know still parses (#3093 needs that).
    pub features: Vec<(String, u64)>,
}

pub(crate) fn is_version_token(token: &str) -> bool {
    let digits = token
        .strip_prefix('v')
        .or_else(|| token.strip_prefix('V'))
        .unwrap_or(token);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn is_feature_name(token: &str) -> bool {
    let mut chars = token.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Parse the genesis activation string into version steps and feature
/// activations (#3092): `0:v1,1000:v2,5000:domain_separated_preimages`.
///
/// A token that looks like a version (`vN` / `N`) MUST be a version this
/// binary knows ([`ActivationScheduleError::UnknownVersion`], the pre-#3092
/// rule, unchanged). Any other token must be a lowercase identifier and is a
/// feature name — known or not. `signing_v2` is refused as a feature name
/// because the signing version is scheduled with `v2`.
pub fn parse_genesis_activation_schedule(
    raw: &str,
) -> Result<ParsedActivationSchedule, ActivationScheduleError> {
    let mut parsed = ParsedActivationSchedule::default();
    for field in raw.split(',') {
        let field = field.trim();
        if field.is_empty() {
            return Err(ActivationScheduleError::Malformed {
                raw: raw.to_string(),
                reason: "empty entry",
            });
        }
        let Some((height_raw, token_raw)) = field.split_once(':') else {
            return Err(ActivationScheduleError::Malformed {
                raw: raw.to_string(),
                reason: "entry is not height:version",
            });
        };
        let height = height_raw.trim().parse::<u64>().map_err(|_| {
            ActivationScheduleError::Malformed {
                raw: raw.to_string(),
                reason: "height is not an unsigned integer",
            }
        })?;
        let token = token_raw.trim();
        if is_version_token(token) {
            let version = SigningProtocolVersion::parse(token).ok_or_else(|| {
                ActivationScheduleError::UnknownVersion {
                    raw: token.to_string(),
                }
            })?;
            parsed.versions.push((height, version));
        } else if token == ConsensusFeature::SigningV2.name() {
            return Err(ActivationScheduleError::Malformed {
                raw: raw.to_string(),
                reason: "the signing protocol version is scheduled as v2, not as a feature name",
            });
        } else if is_feature_name(token) {
            parsed.features.push((token.to_string(), height));
        } else {
            return Err(ActivationScheduleError::Malformed {
                raw: raw.to_string(),
                reason: "token is neither a version (vN) nor a feature name ([a-z][a-z0-9_]*)",
            });
        }
    }
    Ok(parsed)
}

/// A height-indexed schedule of named consensus features (#3092), the
/// generalisation of [`ActivationTable`] from one question ("which signing
/// preimage?") to any scheduled rule change.
///
/// Genesis content, like the table: immutable, hashed into block 1, consulted
/// by producer, applier and replay through [`feature_active`]. Entries are
/// kept by NAME so a schedule can name a feature this binary lacks; see
/// [`FeatureSchedule::first_unknown`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureSchedule {
    /// `(feature name, activation height)`, sorted by height then name; each
    /// name at most once.
    entries: Vec<(String, u64)>,
}

impl FeatureSchedule {
    /// Validate and build: every feature at most once. Any height (including
    /// 0, a genesis-time feature) is valid.
    pub fn from_entries(mut entries: Vec<(String, u64)>) -> Result<Self, ActivationTableError> {
        entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        for window in entries.windows(2) {
            if window[0].0 == window[1].0 {
                return Err(ActivationTableError::DuplicateFeature {
                    feature: window[0].0.clone(),
                });
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in &entries {
            if !seen.insert(name.clone()) {
                return Err(ActivationTableError::DuplicateFeature {
                    feature: name.clone(),
                });
            }
        }
        Ok(Self { entries })
    }

    /// Read the schedule out of a genesis block (#3092). Absent key → only
    /// the genesis-time signing version contributes (a genesis pinning `v2`
    /// activates [`ConsensusFeature::SigningV2`] at 0); everything else is
    /// inactive.
    pub fn from_genesis(
        block: &crate::blockchain::block::Block,
    ) -> Result<Self, ActivationScheduleError> {
        let pairs: Vec<(&str, &str)> = block
            .metadata
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        Self::from_genesis_metadata(pairs)
    }

    /// [`FeatureSchedule::from_genesis`] over already-flattened metadata.
    pub fn from_genesis_metadata<'a, I>(metadata: I) -> Result<Self, ActivationScheduleError>
    where
        I: IntoIterator<Item = (&'a str, &'a str)> + Clone,
    {
        let genesis_version = genesis_protocol_version(metadata.clone())
            .map_err(|e| ActivationScheduleError::GenesisVersion(e.to_string()))?
            .unwrap_or_default();
        let table = resolve_activation_schedule(metadata.clone(), genesis_version)?;
        resolve_feature_schedule(metadata, &table)
    }

    /// The scheduled entries, sorted by height then name.
    pub fn entries(&self) -> &[(String, u64)] {
        &self.entries
    }

    /// Height at which `name` activates, if scheduled.
    pub fn activation_height(&self, name: &str) -> Option<u64> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, h)| *h)
    }

    /// Whether `feature` is in force at `height`: scheduled, and
    /// `height >= activation`. The activation height is the FIRST height at
    /// which the feature applies. Unscheduled → `false`.
    pub fn active(&self, feature: ConsensusFeature, height: u64) -> bool {
        self.activation_height(feature.name())
            .is_some_and(|activation| height >= activation)
    }

    /// The lowest-height scheduled feature that is NOT in `known` — the first
    /// activation this binary cannot follow (#3093). `None` when every
    /// scheduled feature is known.
    pub fn first_unknown(&self, known: &[ConsensusFeature]) -> Option<(String, u64)> {
        self.entries
            .iter()
            .find(|(name, _)| !known.iter().any(|k| k.name() == name))
            .cloned()
    }
}

/// Resolve the feature schedule this chain runs, from genesis metadata plus
/// the already-resolved version table (#3092).
///
/// Pure — no globals. [`ConsensusFeature::SigningV2`] is derived from `table`
/// (the first height at or above which [`SigningProtocolVersion::V2`] is in
/// force), so the feature schedule and [`protocol_version_at`] have one
/// source and cannot disagree. Named features come from the genesis string.
pub fn resolve_feature_schedule<'a, I>(
    genesis_metadata: I,
    table: &ActivationTable,
) -> Result<FeatureSchedule, ActivationScheduleError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut entries = Vec::new();
    for (key, value) in genesis_metadata {
        if key == GENESIS_ACTIVATION_SCHEDULE_KEY {
            entries = parse_genesis_activation_schedule(value)?.features;
            break;
        }
    }
    if let Some(height) = table.activation_height_of(SigningProtocolVersion::V2) {
        entries.push((ConsensusFeature::SigningV2.name().to_string(), height));
    }
    // #3430: a registered feature with no behaviour must not be bootable from a genesis schedule.
    if let Some((name, _)) = entries
        .iter()
        .find(|(name, _)| ConsensusFeature::parse(name).is_some_and(|f| !f.has_behaviour()))
    {
        return Err(ActivationScheduleError::Invalid(
            ActivationTableError::FeatureWithoutBehaviour {
                feature: name.clone(),
            },
        ));
    }
    FeatureSchedule::from_entries(entries).map_err(ActivationScheduleError::Invalid)
}

/// Process-wide feature schedule, installed once at boot beside
/// [`ACTIVE_TABLE`].
static ACTIVE_FEATURES: OnceLock<FeatureSchedule> = OnceLock::new();

/// The raw genesis schedule string this process resolved from (or, on an
/// uncut chain, the cut-time input block 1 must record). `None` when genesis
/// carries no schedule.
static ACTIVE_SCHEDULE_RAW: OnceLock<Option<String>> = OnceLock::new();

/// Install the process-wide feature schedule. First install wins — same
/// contract as [`install_activation_table`].
pub fn install_feature_schedule(schedule: FeatureSchedule) -> &'static FeatureSchedule {
    ACTIVE_FEATURES.get_or_init(|| schedule)
}

/// The installed BASE feature schedule (genesis; from S5 also the per-chain
/// bootstrap), if the startup gate has run. Applied schedule extensions are
/// NOT part of it — see [`effective_feature_schedule`].
///
/// When the node booted on an empty volume, the base was first resolved from
/// cut-time configuration; once block 1 has been synced it is re-resolved
/// from block 1 ([`reresolve_base_feature_schedule_from_block1`], #3277 F4)
/// and that resolution wins from then on.
pub fn try_feature_schedule() -> Option<&'static FeatureSchedule> {
    BASE_FROM_BLOCK1.get().or_else(|| ACTIVE_FEATURES.get())
}

/// #3277 F4: the base schedule re-resolved from the synced block 1 of a node
/// that booted on an empty volume. Set at most once.
static BASE_FROM_BLOCK1: OnceLock<FeatureSchedule> = OnceLock::new();

/// Whether the installed base came from cut-time configuration (the uncut
/// boot path) rather than from a durable block 1.
static BASE_IS_PROVISIONAL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether the base schedule still awaits re-resolution from block 1 (the
/// node booted on an empty volume and has not yet re-resolved).
pub fn base_feature_schedule_is_provisional() -> bool {
    BASE_IS_PROVISIONAL.load(std::sync::atomic::Ordering::SeqCst) && BASE_FROM_BLOCK1.get().is_none()
}

/// #3277 F4: once block 1 is durable on a node that booted on an empty
/// volume, re-resolve the base feature schedule from block 1's own metadata,
/// so consensus no longer depends on the cut-time configuration the node was
/// started with. First call wins; a no-op when the base already came from
/// block 1. Returns the schedule now in force.
///
/// Scope: the FEATURE schedule only. The signing-version table stays as
/// installed at boot (its refresh is S11's).
pub fn reresolve_base_feature_schedule_from_block1(
    block1: &crate::blockchain::block::Block,
) -> Result<&'static FeatureSchedule, ActivationScheduleError> {
    if !BASE_IS_PROVISIONAL.load(std::sync::atomic::Ordering::SeqCst) {
        return try_feature_schedule().ok_or(ActivationScheduleError::Malformed {
            raw: String::new(),
            reason: "no base feature schedule installed",
        });
    }
    let resolved = FeatureSchedule::from_genesis(block1)?;
    // #3277 S5: the bootstrap is part of the base wherever the base comes
    // from block 1, so a node that synced the live chain from an empty
    // volume opens the extension gate at the same height as its peers.
    let block1_hash = block1.hash.unwrap_or_else(|| block1.calculate_hash()).to_hex();
    let resolved = with_legacy_chain_bootstrap(resolved, &block1_hash)
        .map_err(ActivationScheduleError::Invalid)?;
    Ok(BASE_FROM_BLOCK1.get_or_init(|| resolved))
}

/// Feature activations carried by APPLIED schedule extensions (#3277 S3),
/// refreshed from the canonical `State` at every point where it changes
/// (block apply, reorg, boot load, snapshot import). Replaceable, unlike the
/// base: it is a function of chain state, not of genesis.
///
/// Determinism: every entry activates at least `MIN_RUNUP_BLOCKS` (>
/// `MAX_REORG_DEPTH`) after the block that carried it, so any entry active at
/// an evaluable height is in the common ancestor of every competing branch;
/// entries that differ between branches all answer `false` until the refresh
/// replaces them (design §1.5).
static EXTENSION_OVERLAY: std::sync::RwLock<Vec<(String, u64)>> =
    std::sync::RwLock::new(Vec::new());

/// Replace the extension overlay with `entries` (`(feature, activation)`).
/// Called with the canonical state's applied extensions.
pub fn refresh_feature_schedule_extensions(mut entries: Vec<(String, u64)>) {
    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut slot = EXTENSION_OVERLAY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *slot = entries;
}

/// The current extension overlay, sorted by height then name.
pub fn feature_schedule_extension_entries() -> Vec<(String, u64)> {
    EXTENSION_OVERLAY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn extension_overlay_active(name: &str, height: u64) -> bool {
    EXTENSION_OVERLAY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|(n, activation)| n == name && height >= *activation)
}

/// The effective schedule: base (genesis ∪ bootstrap) ∪ applied extensions.
/// V6 guarantees the two are disjoint by name; an overlay entry that somehow
/// repeats a base name is ignored rather than allowed to move it.
pub fn effective_feature_schedule() -> FeatureSchedule {
    let mut entries: Vec<(String, u64)> = try_feature_schedule()
        .map(|base| base.entries().to_vec())
        .unwrap_or_default();
    for (name, height) in feature_schedule_extension_entries() {
        if !entries.iter().any(|(n, _)| *n == name) {
            entries.push((name, height));
        }
    }
    FeatureSchedule::from_entries(entries).unwrap_or_default()
}

/// Record the raw schedule string genesis carries (or will carry). First
/// install wins.
pub fn install_genesis_activation_schedule_string(raw: Option<String>) {
    let _ = ACTIVE_SCHEDULE_RAW.get_or_init(|| raw);
}

/// The raw schedule string, for `stamp_genesis_chain_metadata` to write into
/// block 1 of an uncut chain and for operator-facing reporting.
pub fn try_genesis_activation_schedule_string() -> Option<&'static str> {
    ACTIVE_SCHEDULE_RAW.get().and_then(|raw| raw.as_deref())
}

/// Whether `feature` is in force at `height` on this node (#3092) — the one
/// question every consensus rule change asks, on producer, applier and
/// replay alike.
///
/// [`ConsensusFeature::SigningV2`] is answered from the version table
/// ([`protocol_version_at`]), so the two can never disagree: a schedule that
/// pins `v2` at H answers `true` from H under both names. Every other
/// feature is answered from the installed [`FeatureSchedule`]; with none
/// installed it is inactive, so a tree that never installs one behaves as it
/// does today.
pub fn feature_active(feature: ConsensusFeature, height: u64) -> bool {
    match feature {
        ConsensusFeature::SigningV2 => protocol_version_at(height) >= SigningProtocolVersion::V2,
        other => match try_feature_schedule() {
            // A name the base schedules is answered by the base alone: an
            // extension can never move or re-arm it (V6, enforced here too).
            Some(base) if base.activation_height(other.name()).is_some() => {
                base.active(other, height)
            },
            _ => extension_overlay_active(other.name(), height),
        },
    }
}

/// Whether `feature` is scheduled at ANY height on this node — the
/// height-free question mempool admission asks. Answers from the same source
/// as [`feature_active`]: the base schedule (genesis ∪ bootstrap) when it
/// names the feature, otherwise the applied schedule-extension overlay. So a
/// feature activated by a governance extension (not genesis) counts as
/// scheduled once the extension is applied, exactly when `feature_active`
/// can first answer `true` for it.
///
/// Policy only: no block-validity or state-transition rule reads this.
pub fn feature_scheduled(feature: ConsensusFeature) -> bool {
    match feature {
        ConsensusFeature::SigningV2 => protocol_version_at(u64::MAX) >= SigningProtocolVersion::V2,
        other => {
            try_feature_schedule().is_some_and(|base| base.activation_height(other.name()).is_some())
                || EXTENSION_OVERLAY
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .any(|(n, _)| n == other.name())
        },
    }
}

/// Why a genesis activation schedule was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationScheduleError {
    /// The string is not `height:version[,height:version…]`.
    Malformed { raw: String, reason: &'static str },
    /// A version token no parser recognises. Fails closed rather than guessing.
    UnknownVersion { raw: String },
    /// Parsed, but not a valid schedule.
    Invalid(ActivationTableError),
    /// The schedule and the scalar `protocol_version` disagree about genesis.
    /// One chain cannot start at two versions.
    GenesisDisagreement {
        schedule_at_zero: SigningProtocolVersion,
        genesis_version: SigningProtocolVersion,
    },
    /// The scalar genesis `protocol_version` could not be read (#3092,
    /// `FeatureSchedule::from_genesis`).
    GenesisVersion(String),
}

impl fmt::Display for ActivationScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { raw, reason } => {
                write!(f, "malformed activation schedule {raw:?}: {reason}")
            },
            Self::UnknownVersion { raw } => {
                write!(f, "unknown protocol version {raw:?} in activation schedule")
            },
            Self::Invalid(inner) => write!(f, "invalid activation schedule: {inner}"),
            Self::GenesisDisagreement {
                schedule_at_zero,
                genesis_version,
            } => write!(
                f,
                "activation schedule starts at {schedule_at_zero} but genesis \
                 pins {genesis_version}; one chain cannot start at two versions"
            ),
            Self::GenesisVersion(reason) => {
                write!(f, "genesis protocol version unreadable: {reason}")
            },
        }
    }
}

impl std::error::Error for ActivationScheduleError {}

/// Parse a schedule string's VERSION steps: `0:v1,1000:v2` (#1396).
///
/// Since #3092 the same string may also carry named feature activations;
/// those are parsed by [`parse_genesis_activation_schedule`] and ignored
/// here. A string with no version steps is the identity table.
pub fn parse_activation_schedule(raw: &str) -> Result<ActivationTable, ActivationScheduleError> {
    let parsed = parse_genesis_activation_schedule(raw)?;
    ActivationTable::from_entries(parsed.versions).map_err(ActivationScheduleError::Invalid)
}

/// Read the schedule out of genesis metadata, if it records one.
pub fn genesis_activation_schedule<'a, I>(
    metadata: I,
) -> Result<Option<ActivationTable>, ActivationScheduleError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    for (key, value) in metadata {
        if key == GENESIS_ACTIVATION_SCHEDULE_KEY {
            return parse_activation_schedule(value).map(Some);
        }
    }
    Ok(None)
}

/// Resolve the schedule this chain runs, given the already-resolved scalar
/// genesis version.
///
/// Pure — no globals, no environment — so the startup gate can check it before
/// arming anything. Absent schedule ⇒ the scalar at every height, i.e. exactly
/// the pre-#1396 chain.
pub fn resolve_activation_schedule<'a, I>(
    genesis_metadata: I,
    resolved_version: SigningProtocolVersion,
) -> Result<ActivationTable, ActivationScheduleError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut parsed: Option<ParsedActivationSchedule> = None;
    for (key, value) in genesis_metadata {
        if key == GENESIS_ACTIVATION_SCHEDULE_KEY {
            parsed = Some(parse_genesis_activation_schedule(value)?);
            break;
        }
    }
    // #3092: a schedule carrying only named features (no `vN` steps) says
    // nothing about the signing version, so the scalar applies at every
    // height exactly as if the key were absent.
    let table = match parsed {
        None => None,
        Some(p) if p.versions.is_empty() => None,
        Some(p) => Some(
            ActivationTable::from_entries(p.versions).map_err(ActivationScheduleError::Invalid)?,
        ),
    };
    match table {
        None => ActivationTable::from_entries(vec![(0, resolved_version)])
            .map_err(ActivationScheduleError::Invalid),
        Some(table) => {
            let at_zero = table.version_at(0);
            if at_zero != resolved_version {
                return Err(ActivationScheduleError::GenesisDisagreement {
                    schedule_at_zero: at_zero,
                    genesis_version: resolved_version,
                });
            }
            Ok(table)
        },
    }
}

impl fmt::Display for SigningProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.as_u32())
    }
}

/// The network a signature or block belongs to.
///
/// Sourced from configuration, never from a constant — #2065 was caused exactly
/// by hardwired identifiers. [`DEFAULT_CHAIN_ID`] / [`DEFAULT_NETWORK_ID`]
/// remain only as the unconfigured fallback, and
/// [`ChainIdentity::is_default_fallback`] lets a caller detect that it is
/// running on them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainIdentity {
    pub chain_id: String,
    pub network_id: String,
}

impl ChainIdentity {
    pub fn new(chain_id: impl Into<String>, network_id: impl Into<String>) -> Self {
        Self {
            chain_id: chain_id.into(),
            network_id: network_id.into(),
        }
    }

    /// Resolve from deployment configuration (#2034 environment variables).
    pub fn from_config() -> Self {
        Self {
            chain_id: signing_chain_id(),
            network_id: signing_network_id(),
        }
    }

    /// Process-wide identity, resolved once. See [`SigningProtocolVersion::active`].
    pub fn active() -> &'static ChainIdentity {
        static ACTIVE: OnceLock<ChainIdentity> = OnceLock::new();
        ACTIVE.get_or_init(Self::from_config)
    }

    /// Whether both identifiers are the unconfigured mainnet fallbacks.
    ///
    /// A testnet node running on these is misconfigured in the direction that
    /// matters: it will produce and accept mainnet-identified signatures.
    pub fn is_default_fallback(&self) -> bool {
        self.chain_id == DEFAULT_CHAIN_ID && self.network_id == DEFAULT_NETWORK_ID
    }

    /// Read the identity recorded in a genesis block's metadata.
    ///
    /// `None` when genesis predates this record — every chain created before
    /// #2123 is in that state, so the caller must treat it as "unknown", not
    /// "mismatched".
    pub fn from_genesis_metadata<'a, I>(metadata: I) -> Option<Self>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut chain_id = None;
        let mut network_id = None;
        for (k, v) in metadata {
            match k {
                GENESIS_CHAIN_ID_KEY => chain_id = Some(v.to_string()),
                GENESIS_NETWORK_ID_KEY => network_id = Some(v.to_string()),
                _ => {},
            }
        }
        Some(Self {
            chain_id: chain_id?,
            network_id: network_id?,
        })
    }

    /// Four-byte network magic derived from the identity (#2066).
    ///
    /// Derived rather than assigned so a new network cannot forget to pick one,
    /// and so two networks cannot collide unless their identifiers collide.
    pub fn network_magic(&self) -> [u8; 4] {
        let mut material = Vec::with_capacity(64);
        push_length_prefixed(&mut material, b"wrfcoin.network_magic.v1");
        push_length_prefixed(&mut material, self.chain_id.as_bytes());
        push_length_prefixed(&mut material, self.network_id.as_bytes());
        let digest = hash_bytes(&material);
        let bytes = digest.as_bytes();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

impl fmt::Display for ChainIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.chain_id, self.network_id)
    }
}

/// Why a node refused to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainIdentityError {
    /// Runtime configuration disagrees with the chain the node is joining.
    GenesisMismatch {
        configured: ChainIdentity,
        genesis: ChainIdentity,
    },
    /// The configured chain id is empty or whitespace.
    EmptyIdentifier { field: &'static str },
}

impl fmt::Display for ChainIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GenesisMismatch {
                configured,
                genesis,
            } => write!(
                f,
                "chain identity mismatch: configured {configured}, genesis {genesis} — \
                 refusing to start. A node signing under one chain id against a chain \
                 created under another produces signatures no peer will accept."
            ),
            Self::EmptyIdentifier { field } => {
                write!(f, "chain identity field `{field}` is empty")
            },
        }
    }
}

impl std::error::Error for ChainIdentityError {}

/// Why a node refused to start over protocol-version activation (#2123).
///
/// Every variant is a **refusal to start**. That is deliberate: the alternative
/// to a loud refusal here is the #2076 failure — a v1 signer against a v2
/// verifier, where nothing errors and signatures simply stop verifying, which
/// is indistinguishable from invalid signatures. A node that will not boot is
/// diagnosable in seconds; a chain that silently halts took 28 hours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationError {
    /// Local configuration asked for V2 but genesis does not pin V2.
    ///
    /// **This is the variant that closes the review finding.** Without it, a
    /// single node with `WRFCOIN_SIGNING_PROTOCOL_VERSION=2` runs V2 while its
    /// peers and external signers stay V1 — exactly the #2076 asymmetry.
    UnilateralActivation {
        configured: SigningProtocolVersion,
        genesis: Option<SigningProtocolVersion>,
    },
    /// Local configuration disagrees with the version genesis pins.
    VersionMismatch {
        configured: SigningProtocolVersion,
        genesis: SigningProtocolVersion,
    },
    /// Genesis records a protocol version this build does not understand.
    UnknownGenesisVersion { raw: String },
    /// V2 was requested while the chain identity is still the unconfigured
    /// mainnet fallback.
    DefaultIdentityAtV2 { identity: ChainIdentity },
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnilateralActivation {
                configured,
                genesis,
            } => write!(
                f,
                "refusing to start: {SIGNING_PROTOCOL_VERSION_ENV} requests {configured} but \
                 genesis pins {} — a node cannot activate a protocol version by itself. \
                 Activation is chain-wide: re-create the chain with \
                 `{GENESIS_PROTOCOL_VERSION_KEY}` in genesis metadata. Starting anyway would \
                 reproduce #2076 (a v1 signer against a v2 verifier, silently halting the chain).",
                match genesis {
                    Some(v) => v.to_string(),
                    None => "nothing".to_string(),
                }
            ),
            Self::VersionMismatch {
                configured,
                genesis,
            } => write!(
                f,
                "refusing to start: {SIGNING_PROTOCOL_VERSION_ENV} requests {configured} but \
                 genesis pins {genesis} — this node would sign under a different version than \
                 the chain it is joining"
            ),
            Self::UnknownGenesisVersion { raw } => write!(
                f,
                "refusing to start: genesis pins protocol version {raw:?}, which this build \
                 does not understand — upgrade the node rather than guessing"
            ),
            Self::DefaultIdentityAtV2 { identity } => write!(
                f,
                "refusing to start: protocol v2 requested with the unconfigured mainnet \
                 fallback identity {identity} — set {CHAIN_ID_ENV} and {NETWORK_ID_ENV} \
                 explicitly. At v2 the identity is inside every preimage, so a fallback \
                 identity silently produces mainnet-identified signatures."
            ),
        }
    }
}

impl std::error::Error for ActivationError {}

/// Read the protocol version genesis pins, if any.
///
/// `Ok(None)` means genesis predates #2123 — every chain in existence today.
/// `Err` means genesis pins something this build cannot parse, which must be a
/// refusal rather than a fallback: guessing would put this node on a different
/// encoding from the chain.
pub fn genesis_protocol_version<'a, I>(
    metadata: I,
) -> Result<Option<SigningProtocolVersion>, ActivationError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    for (key, value) in metadata {
        if key == GENESIS_PROTOCOL_VERSION_KEY {
            return match SigningProtocolVersion::parse(value) {
                Some(v) => Ok(Some(v)),
                None => Err(ActivationError::UnknownGenesisVersion {
                    raw: value.to_string(),
                }),
            };
        }
    }
    Ok(None)
}

/// Resolve the version this node will actually run, from the **chain-wide**
/// boundary rather than local configuration.
///
/// # The rule
///
/// Genesis decides. Local configuration may only *agree*; disagreeing is a
/// refusal to start. In particular, `WRFCOIN_SIGNING_PROTOCOL_VERSION=2` on a
/// chain whose genesis does not pin v2 **refuses**, so no single node can move
/// itself to v2 while its peers stay behind.
///
/// | genesis | configured | result |
/// |---|---|---|
/// | none | none / v1 | v1 — every chain that exists today |
/// | none | **v2** | **refuse** (`UnilateralActivation`) |
/// | v1 | v2 | refuse (`VersionMismatch`) |
/// | v2 | v1 | refuse (`VersionMismatch`) |
/// | v2 | none / v2 | v2, if the identity is not the default fallback |
pub fn resolve_active_version(
    genesis: Option<SigningProtocolVersion>,
    configured: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
) -> Result<SigningProtocolVersion, ActivationError> {
    let effective = match (genesis, configured) {
        // Pre-#2123 genesis. Only v1 is coherent; asking for v2 is unilateral.
        (None, None) | (None, Some(SigningProtocolVersion::V1)) => SigningProtocolVersion::V1,
        (None, Some(configured)) => {
            return Err(ActivationError::UnilateralActivation {
                configured,
                genesis: None,
            });
        },
        // Genesis pins a version: configuration must agree or stay silent.
        (Some(pinned), None) => pinned,
        (Some(pinned), Some(configured)) if configured == pinned => pinned,
        (Some(pinned), Some(configured)) => {
            return Err(ActivationError::VersionMismatch {
                configured,
                genesis: pinned,
            });
        },
    };

    // At v2 the identity is inside every preimage, so running on the
    // unconfigured mainnet fallback would silently mint mainnet-identified
    // signatures on some other network.
    if effective >= SigningProtocolVersion::V2 && identity.is_default_fallback() {
        return Err(ActivationError::DefaultIdentityAtV2 {
            identity: identity.clone(),
        });
    }

    Ok(effective)
}


/// Resolve the signing version for a chain that **has no blocks yet** (#2545).
///
/// # Why this is separate from [`resolve_active_version`]
///
/// That function takes `genesis: Option<SigningProtocolVersion>`, and `None`
/// conflates two states that must not be conflated:
///
/// | state | meaning | correct behaviour at V2 |
/// |---|---|---|
/// | blocks exist, genesis records no version | pre-#2123 legacy chain | **refuse** — the #2076 protection |
/// | **no blocks at all** | the chain is not cut yet; configuration *defines* what genesis will record | **allow** — this is the cut |
///
/// Collapsing them is why the #2034 cut could not activate V2: a fresh chain
/// has no genesis to pin a version, so `WRFCOIN_SIGNING_PROTOCOL_VERSION=2`
/// returned `UnilateralActivation` and the node refused to start — with a
/// remedy ("re-create the chain with `protocol_version` in genesis metadata")
/// that no production code could carry out, because nothing wrote genesis
/// metadata at all.
///
/// On an uncut chain the operator is not overriding a chain-wide decision;
/// they are **making** it. The obligation that follows is that block 1 must
/// then RECORD the version, so every later boot reads a pinned value instead
/// of defaulting — otherwise this merely moves the silence one restart later.
///
/// [`resolve_active_version`] is deliberately left untouched: every chain that
/// already exists keeps exactly today's behaviour.
pub fn resolve_version_for_uncut_chain(
    configured: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
) -> Result<SigningProtocolVersion, ActivationError> {
    let effective = configured.unwrap_or(SigningProtocolVersion::V1);

    // Unchanged and still enforced: at V2 the identity is inside every
    // preimage, so cutting on the unconfigured mainnet fallback would mint
    // mainnet-identified signatures on some other network.
    if effective >= SigningProtocolVersion::V2 && identity.is_default_fallback() {
        return Err(ActivationError::DefaultIdentityAtV2 {
            identity: identity.clone(),
        });
    }

    Ok(effective)
}

/// Resolve BOTH halves of the protocol boundary for an uncut chain (#1396).
///
/// # Why this returns a pair
///
/// Arming a node means installing two things: the scalar
/// [`SigningProtocolVersion`] and the [`ActivationTable`] that answers
/// `protocol_version_at(height)`. [`assert_startup_protocol_compatibility`] —
/// the path for a chain that already has blocks — installs both. The uncut path
/// originally installed only the version (#2556), and because a missing table is
/// represented as `None` rather than a default, p2p startup refused with "no
/// protocol activation schedule installed" (#2500) and **every node of a freshly
/// cut chain came up with networking silently disabled**. It failed open: one
/// ERROR line, then the node served traffic with `P2P: disabled` and exit 0.
///
/// The two PRs landed the same day and neither accounted for the other. Rather
/// than re-document the ordering and hope, this returns the version and the
/// schedule **together**, so a caller cannot obtain one without the other. That
/// is the difference between a rule and an invariant.
///
/// On an uncut chain there is no genesis metadata to read a schedule from, so
/// the table is the identity schedule answering `resolved` at every height —
/// byte-for-byte what a chain with no schedule does.
pub fn resolve_uncut_chain_protocol(
    configured: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
) -> Result<(SigningProtocolVersion, ActivationTable), Box<dyn std::error::Error + Send + Sync>> {
    let resolved = resolve_version_for_uncut_chain(configured, identity)?;
    let schedule = resolve_activation_schedule(std::iter::empty(), resolved)?;
    Ok((resolved, schedule))
}

/// Everything an uncut chain's boot must arm, resolved together (#3092).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncutChainProtocol {
    pub version: SigningProtocolVersion,
    pub table: ActivationTable,
    pub features: FeatureSchedule,
    /// The schedule string block 1 must record, if the cut declares one.
    pub schedule_raw: Option<String>,
}

/// [`resolve_uncut_chain_protocol`] plus the cut-time activation schedule
/// (#3092): `schedule_raw` is the operator's declaration
/// ([`GENESIS_ACTIVATION_SCHEDULE_ENV`]) of what block 1 will record. It is
/// validated exactly as a genesis-borne schedule would be — including
/// agreement with the resolved version at height 0 — so a cut cannot record
/// a schedule the very next boot would refuse.
///
/// Pure: the caller supplies the string, so tests never touch the process
/// environment.
pub fn resolve_uncut_chain_protocol_with_schedule(
    configured: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
    schedule_raw: Option<&str>,
) -> Result<UncutChainProtocol, Box<dyn std::error::Error + Send + Sync>> {
    if identity.network_id == "testnet" && identity.chain_id != "777"
        && schedule_raw.is_none_or(|raw| raw.trim().is_empty()) {
        return Err("new testnet cut requires WRFCOIN_GENESIS_ACTIVATION_SCHEDULE".into());
    }
    let version = resolve_version_for_uncut_chain(configured, identity)?;
    let schedule_raw = schedule_raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_string);
    let metadata: Vec<(&str, &str)> = schedule_raw
        .as_deref()
        .map(|raw| vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw)])
        .unwrap_or_default();
    let table = resolve_activation_schedule(metadata.clone(), version)?;
    let features = resolve_feature_schedule(metadata, &table)?;
    Ok(UncutChainProtocol {
        version,
        table,
        features,
        schedule_raw,
    })
}

/// Arm every process-wide protocol global for an uncut chain, in the fixed
/// order the cut path documents: version, table, features, raw string.
pub fn install_uncut_chain_protocol(resolved: UncutChainProtocol) -> SigningProtocolVersion {
    let armed = SigningProtocolVersion::install_active(resolved.version);
    install_activation_table(resolved.table);
    // #3277 F4: this base comes from cut-time configuration, not a durable
    // block 1; mark it for re-resolution once block 1 is synced.
    BASE_IS_PROVISIONAL.store(true, std::sync::atomic::Ordering::SeqCst);
    install_feature_schedule(resolved.features);
    install_genesis_activation_schedule_string(resolved.schedule_raw);
    armed
}

/// The single startup gate a node binary must call before producing blocks.
///
/// Checks, in order: the chain identity matches genesis, then the protocol
/// version is chain-wide rather than locally chosen. Returns the version the
/// node must run under.
///
/// Call this once at boot and **refuse to start on `Err`**.
pub fn assert_startup_protocol_compatibility<'a, I>(
    genesis_metadata: I,
    configured_version: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
) -> Result<SigningProtocolVersion, Box<dyn std::error::Error + Send + Sync>>
where
    I: IntoIterator<Item = (&'a str, &'a str)> + Clone,
{
    assert_startup_protocol_compatibility_for_block1(
        genesis_metadata,
        configured_version,
        identity,
        None,
    )
}

/// Arm the process-wide protocol globals from a chain's durable block 1 alone
/// (#3277 S5), for offline tools such as `replay_verify` that have no
/// configured identity to check: the version block 1 records, its activation
/// table, and its feature schedule with the chain's [`LEGACY_CHAIN_BOOTSTRAP`]
/// rows merged in. Resolves everything before arming anything; first install
/// wins, as at boot.
pub fn install_protocol_from_block1(
    block1: &crate::blockchain::block::Block,
) -> Result<SigningProtocolVersion, Box<dyn std::error::Error + Send + Sync>> {
    validate_testnet_genesis(block1)?;
    let pairs: Vec<(&str, &str)> = block1
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let version = genesis_protocol_version(pairs.clone())?.unwrap_or_default();
    let table = resolve_activation_schedule(pairs.clone(), version)?;
    let features = resolve_feature_schedule(pairs.clone(), &table)?;
    let block1_hash = block1.hash.unwrap_or_else(|| block1.calculate_hash()).to_hex();
    let features = with_legacy_chain_bootstrap(features, &block1_hash)?;
    let schedule_raw = pairs
        .iter()
        .find(|(key, _)| *key == GENESIS_ACTIVATION_SCHEDULE_KEY)
        .map(|(_, value)| value.to_string());
    let armed = SigningProtocolVersion::install_active(version);
    install_activation_table(table);
    install_feature_schedule(features);
    install_genesis_activation_schedule_string(schedule_raw);
    Ok(armed)
}

/// [`assert_startup_protocol_compatibility`] for a chain whose durable block
/// 1 hashes to `block1_hash` (#3277 S5): the installed BASE feature schedule
/// is genesis plus this chain's [`LEGACY_CHAIN_BOOTSTRAP`] rows, merged
/// before anything is armed (a refused merge arms nothing). `None` — an
/// uncut chain, or a caller without block 1 — merges nothing.
pub fn assert_startup_protocol_compatibility_for_block1<'a, I>(
    genesis_metadata: I,
    configured_version: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
    block1_hash: Option<&str>,
) -> Result<SigningProtocolVersion, Box<dyn std::error::Error + Send + Sync>>
where
    I: IntoIterator<Item = (&'a str, &'a str)> + Clone,
{
    // Resolve WITHOUT arming, so a refused activation schedule cannot leave a
    // version installed — the same invariant the inner function states for its
    // own checks, extended to cover the schedule (#1396).
    let resolved = assert_startup_protocol_compatibility_with_installer(
        genesis_metadata.clone(),
        configured_version,
        identity,
        |version| version,
    )?;
    let schedule = resolve_activation_schedule(genesis_metadata.clone(), resolved)?;
    // #3092: the feature schedule is resolved BEFORE anything is armed for the
    // same reason as the table — a refused schedule must leave nothing
    // installed.
    let features = resolve_feature_schedule(genesis_metadata.clone(), &schedule)?;
    let features = match block1_hash {
        Some(hash) => with_legacy_chain_bootstrap(features, hash)?,
        None => features,
    };
    let schedule_raw = genesis_metadata
        .into_iter()
        .find(|(key, _)| *key == GENESIS_ACTIVATION_SCHEDULE_KEY)
        .map(|(_, value)| value.to_string());

    // Every check has passed: arm all, version first so that anything reading
    // `active()` during table install sees the value the table agrees with.
    let armed = SigningProtocolVersion::install_active(resolved);
    install_activation_table(schedule);
    install_feature_schedule(features);
    install_genesis_activation_schedule_string(schedule_raw);
    Ok(armed)
}

/// Testable startup seam: production supplies the process-wide installer,
/// while unit tests can observe the resolved value without arming `ACTIVE_VERSION`.
fn assert_startup_protocol_compatibility_with_installer<'a, I, F>(
    genesis_metadata: I,
    configured_version: Option<SigningProtocolVersion>,
    identity: &ChainIdentity,
    install: F,
) -> Result<SigningProtocolVersion, Box<dyn std::error::Error + Send + Sync>>
where
    I: IntoIterator<Item = (&'a str, &'a str)> + Clone,
    F: FnOnce(SigningProtocolVersion) -> SigningProtocolVersion,
{
    let genesis_identity = ChainIdentity::from_genesis_metadata(genesis_metadata.clone());
    assert_chain_identity_matches_genesis(identity, genesis_identity.as_ref())?;
    let genesis_version = genesis_protocol_version(genesis_metadata)?;
    let resolved = resolve_active_version(genesis_version, configured_version, identity)?;
    // Install only after every check has passed, so a refused startup can never
    // leave a version armed.
    Ok(install(resolved))
}

/// Startup assertion: the configured identity must match the genesis identity.
///
/// `genesis` is `None` for chains created before #2123 recorded the identity in
/// genesis metadata. That is accepted — it is unknown, not mismatched — because
/// treating it as a mismatch would refuse to start every existing node.
///
/// Call this once at boot and refuse to start on `Err`.
pub fn assert_chain_identity_matches_genesis(
    configured: &ChainIdentity,
    genesis: Option<&ChainIdentity>,
) -> Result<(), ChainIdentityError> {
    if configured.chain_id.trim().is_empty() {
        return Err(ChainIdentityError::EmptyIdentifier { field: "chain_id" });
    }
    if configured.network_id.trim().is_empty() {
        return Err(ChainIdentityError::EmptyIdentifier {
            field: "network_id",
        });
    }

    match genesis {
        None => Ok(()),
        Some(genesis) if genesis == configured => Ok(()),
        Some(genesis) => Err(ChainIdentityError::GenesisMismatch {
            configured: configured.clone(),
            genesis: genesis.clone(),
        }),
    }
}

/// Build a chain-and-version bound preimage (#2123).
///
/// Layout, every component length-prefixed so concatenation is unambiguous:
///
/// ```text
/// V1: DOMAIN_TAG || body
/// V2: DOMAIN_TAG || chain_id || network_id || protocol_version || body
/// ```
///
/// At V1 this is byte-identical to the unbound construction, which is what
/// makes the version gate a no-op until it is deliberately flipped.
pub fn domain_preimage_bound(
    tag: &[u8],
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
    canonical_body: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(48 + tag.len() + canonical_body.len());
    push_length_prefixed(&mut buf, tag);
    if version >= SigningProtocolVersion::V2 {
        push_length_prefixed(&mut buf, identity.chain_id.as_bytes());
        push_length_prefixed(&mut buf, identity.network_id.as_bytes());
        buf.extend_from_slice(&version.as_u32().to_le_bytes());
    }
    push_length_prefixed(&mut buf, canonical_body);
    buf
}

#[cfg(test)]
mod activation_table_tests {
    use super::*;

    fn table(entries: &[(u64, SigningProtocolVersion)]) -> ActivationTable {
        ActivationTable::from_entries(entries.to_vec()).expect("valid schedule")
    }

    /// The identity schedule answers V1 everywhere. This is the property the
    /// whole migration rests on: a chain that pins nothing behaves exactly as
    /// it does today.
    #[test]
    fn default_schedule_is_v1_at_every_height() {
        let t = ActivationTable::default();
        for h in [0u64, 1, 2, 999, 1_000_000, u64::MAX] {
            assert_eq!(
                t.version_at(h),
                SigningProtocolVersion::V1,
                "identity schedule must answer V1 at height {h}"
            );
        }
    }

    /// An empty input is the identity schedule, not an error: "nothing pinned"
    /// and "V1 from genesis" describe the same chain.
    #[test]
    fn empty_entries_build_the_identity_schedule() {
        assert_eq!(
            ActivationTable::from_entries(Vec::new()).expect("empty is valid"),
            ActivationTable::default()
        );
    }

    /// A single-entry schedule — what the #2034 cut pins — answers that version
    /// at every height, so it is behaviourally identical to installing the
    /// version process-globally.
    #[test]
    fn single_entry_schedule_answers_that_version_everywhere() {
        let t = table(&[(0, SigningProtocolVersion::V2)]);
        for h in [0u64, 1, 12_345, u64::MAX] {
            assert_eq!(t.version_at(h), SigningProtocolVersion::V2);
        }
    }

    /// The boundary is the point of the whole mechanism: the activation height
    /// itself is the FIRST height at the new version, not the last at the old.
    #[test]
    fn activation_boundary_is_inclusive_at_the_activation_height() {
        let t = table(&[(0, SigningProtocolVersion::V1), (100, SigningProtocolVersion::V2)]);
        assert_eq!(t.version_at(0), SigningProtocolVersion::V1);
        assert_eq!(t.version_at(99), SigningProtocolVersion::V1, "last V1 height");
        assert_eq!(t.version_at(100), SigningProtocolVersion::V2, "first V2 height");
        assert_eq!(t.version_at(101), SigningProtocolVersion::V2);
        assert_eq!(t.version_at(u64::MAX), SigningProtocolVersion::V2);
    }

    /// A schedule that does not start at 0 leaves early heights unanswerable.
    #[test]
    fn rejects_a_schedule_that_does_not_start_at_genesis() {
        assert_eq!(
            ActivationTable::from_entries(vec![(5, SigningProtocolVersion::V1)]),
            Err(ActivationTableError::MissingGenesisEntry { first_height: 5 })
        );
    }

    /// Duplicate or descending heights would make the lookup ambiguous.
    #[test]
    fn rejects_non_increasing_heights() {
        assert_eq!(
            ActivationTable::from_entries(vec![
                (0, SigningProtocolVersion::V1),
                (0, SigningProtocolVersion::V2),
            ]),
            Err(ActivationTableError::NonIncreasingHeight {
                previous: 0,
                next: 0
            })
        );
    }

    // ---- genesis pinning (#1396 C1, second half) ----

    #[test]
    fn parses_a_multi_entry_schedule() {
        let t = parse_activation_schedule("0:v1,1000:v2").expect("well-formed");
        assert_eq!(t.version_at(999), SigningProtocolVersion::V1);
        assert_eq!(t.version_at(1000), SigningProtocolVersion::V2);
    }

    #[test]
    fn parsing_tolerates_surrounding_whitespace() {
        assert_eq!(
            parse_activation_schedule(" 0 : v1 , 10 : v2 ").expect("well-formed"),
            parse_activation_schedule("0:v1,10:v2").expect("well-formed")
        );
    }

    /// Fail closed on an unrecognised version rather than guessing — the same
    /// posture `genesis_protocol_version` takes for the scalar.
    #[test]
    fn rejects_an_unknown_version_token() {
        assert_eq!(
            parse_activation_schedule("0:v1,10:v9"),
            Err(ActivationScheduleError::UnknownVersion {
                raw: "v9".to_string()
            })
        );
    }

    #[test]
    fn rejects_malformed_entries() {
        for (raw, reason) in [
            ("0:v1,", "empty entry"),
            ("0", "entry is not height:version"),
            ("x:v1", "height is not an unsigned integer"),
        ] {
            assert_eq!(
                parse_activation_schedule(raw),
                Err(ActivationScheduleError::Malformed {
                    raw: raw.to_string(),
                    reason
                }),
                "input {raw:?}"
            );
        }
    }

    /// Schedule validation is not bypassed by coming from genesis.
    #[test]
    fn rejects_a_parsed_but_invalid_schedule() {
        assert_eq!(
            parse_activation_schedule("5:v1"),
            Err(ActivationScheduleError::Invalid(
                ActivationTableError::MissingGenesisEntry { first_height: 5 }
            ))
        );
    }

    /// **Backwards compatibility.** A genesis with no schedule key resolves to
    /// the scalar version at every height — byte-identical to the pre-#1396
    /// chain, so no existing genesis needs rewriting.
    #[test]
    fn absent_schedule_resolves_to_the_scalar_at_every_height() {
        let metadata: Vec<(&str, &str)> = vec![("chain_id", "wrfcoin-testnet")];
        let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V2)
            .expect("absent schedule is valid");
        for h in [0u64, 1, 5_000, u64::MAX] {
            assert_eq!(table.version_at(h), SigningProtocolVersion::V2);
        }
    }

    #[test]
    fn genesis_schedule_is_read_and_honoured() {
        let metadata = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, "0:v1,100:v2")];
        let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1)
            .expect("schedule agrees with genesis at height 0");
        assert_eq!(table.version_at(99), SigningProtocolVersion::V1);
        assert_eq!(table.version_at(100), SigningProtocolVersion::V2);
    }

    /// A schedule that starts at a different version than the scalar would give
    /// one chain two genesis versions. Refuse, do not pick a winner.
    #[test]
    fn rejects_a_schedule_that_disagrees_with_the_scalar_at_genesis() {
        let metadata = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, "0:v1,100:v2")];
        assert_eq!(
            resolve_activation_schedule(metadata, SigningProtocolVersion::V2),
            Err(ActivationScheduleError::GenesisDisagreement {
                schedule_at_zero: SigningProtocolVersion::V1,
                genesis_version: SigningProtocolVersion::V2,
            })
        );
    }

    /// Going backwards would re-interpret bytes that are already sealed.
    #[test]
    fn rejects_a_version_regression() {
        assert_eq!(
            ActivationTable::from_entries(vec![
                (0, SigningProtocolVersion::V2),
                (10, SigningProtocolVersion::V1),
            ]),
            Err(ActivationTableError::VersionRegression {
                height: 10,
                previous: SigningProtocolVersion::V2,
                next: SigningProtocolVersion::V1,
            })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn testnet() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-testnet", "testnet")
    }

    const LIVE_777_BLOCK1: &str =
        "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash

    /// #3277 S5: the bootstrap is keyed by the block-1 hash and nothing else.
    /// The live chain's block 1 (any spelling) gets `schedule_extension` at
    /// `H_ENABLE`; a re-cut chain reusing 777/testnet (another block 1) gets
    /// nothing, and neither does every other chain.
    #[test]
    fn legacy_bootstrap_is_keyed_by_the_block1_hash_3277() {
        let genesis = FeatureSchedule::from_entries(vec![("verify_unit_pricing".into(), 40)]).unwrap();
        for spelling in [
            LIVE_777_BLOCK1.to_string(),
            format!("0x{LIVE_777_BLOCK1}"),
            LIVE_777_BLOCK1.to_ascii_uppercase(),
        ] {
            let merged = with_legacy_chain_bootstrap(genesis.clone(), &spelling).unwrap();
            assert_eq!(
                merged.activation_height("schedule_extension"),
                Some(H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE),
                "{spelling}"
            );
            assert_eq!(merged.activation_height("verify_unit_pricing"), Some(40), "genesis kept");
        }
        let recut = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
        assert_eq!(with_legacy_chain_bootstrap(genesis.clone(), recut).unwrap(), genesis);
        assert_eq!(with_legacy_chain_bootstrap(genesis.clone(), "").unwrap(), genesis);

        // One row per chain, for the gate only.
        assert_eq!(LEGACY_CHAIN_BOOTSTRAP.len(), 1);
        let row = LEGACY_CHAIN_BOOTSTRAP[0];
        assert_eq!(
            (row.chain_id, row.network_id, row.genesis_hash, row.feature),
            ("777", "testnet", LIVE_777_BLOCK1, ConsensusFeature::ScheduleExtension)
        );

        // A genesis that already schedules the gate is refused, never overridden.
        let scheduled = FeatureSchedule::from_entries(vec![("schedule_extension".into(), 9)]).unwrap();
        assert!(with_legacy_chain_bootstrap(scheduled, LIVE_777_BLOCK1).is_err());

        // The merge at an explicit height (the committed row's is a placeholder).
        let rows = [LegacyChainBootstrap { height: 7, ..row }];
        let merged = with_bootstrap_rows(genesis, LIVE_777_BLOCK1, &rows).unwrap();
        assert!(!merged.active(ConsensusFeature::ScheduleExtension, 6));
        assert!(merged.active(ConsensusFeature::ScheduleExtension, 7));
    }

    /// #3277 S5 guard (team-lead ruling 2026-09-28): `H_ENABLE` for 777 is
    /// read from the live tip right before S5 merges (D3: >= tip + 3000). This
    /// test is RED while the placeholder is in place, so the PR cannot go
    /// green with it.
    #[test]
    fn h_enable_777_is_set_before_merge() {
        assert_ne!(
            H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE,
            u64::MAX,
            "H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE is still the placeholder: set it to the \
             owner-read height (>= live tip + 3000, ruling D3) before merging #3277 S5"
        );
        assert!(
            H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE > 21_000,
            "H_ENABLE must be above the live chain's tip"
        );
    }

    /// #3277 S4: which features a live schedule extension may name. Pinned so
    /// flipping one is a reviewed, deliberate change (it is admission policy,
    /// so a flip never forks, but it decides what governance can schedule).
    /// #3430: `stake_submission_nonce` is registered and nothing more. It resolves by name, is
    /// not schedulable on any chain instance, is in no bootstrap row, and is inactive in an empty
    /// schedule at every height, so registering it changes no existing chain's behaviour.
    #[test]
    fn stake_submission_nonce_is_registered_off_and_unscheduled_3430() {
        let feature = ConsensusFeature::StakeSubmissionNonce;
        assert_eq!(feature.name(), "stake_submission_nonce");
        assert_eq!(
            ConsensusFeature::parse("stake_submission_nonce"),
            Some(feature)
        );
        assert!(ConsensusFeature::ALL.contains(&feature));
        let row_chain = LEGACY_FAUCET_BINDINGS[0].genesis_hash;
        let other = "cd".repeat(32);
        for node in [None, Some(row_chain), Some(other.as_str())] {
            assert!(!feature.schedulable(node), "{node:?}");
        }
        assert!(
            LEGACY_CHAIN_BOOTSTRAP
                .iter()
                .all(|row| row.feature != feature),
            "no chain bootstraps it"
        );
        let empty = FeatureSchedule::default();
        for height in [0, 1, 32_500, u64::MAX] {
            assert!(!empty.active(feature, height), "height {height}");
        }
    }

    /// #3430: while `stake_submission_nonce` has no behaviour, a genesis schedule naming it is
    /// refused at resolution (and through `from_genesis_metadata`), at any height; a schedule
    /// naming behaviour-bearing features still resolves. The behaviour slice flips
    /// `has_behaviour`, removing the refusal.
    #[test]
    fn genesis_schedule_naming_a_behaviourless_feature_is_refused_3430() {
        let refused = |raw: &str| {
            let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw)];
            let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
                .expect("activation table");
            (
                resolve_feature_schedule(pairs.clone(), &table),
                FeatureSchedule::from_genesis_metadata(pairs),
            )
        };
        for raw in [
            "0:v1,0:stake_submission_nonce",
            "0:v1,32500:stake_submission_nonce",
        ] {
            let (direct, via_genesis) = refused(raw);
            for result in [direct, via_genesis] {
                assert_eq!(
                    result,
                    Err(ActivationScheduleError::Invalid(
                        ActivationTableError::FeatureWithoutBehaviour {
                            feature: "stake_submission_nonce".into()
                        }
                    )),
                    "{raw}"
                );
            }
        }
        // Control: a feature with behaviour still resolves from the same string shape.
        let (direct, via_genesis) = refused("0:v1,32500:weather_skew_block_time_allowance");
        assert!(direct.is_ok() && via_genesis.is_ok());
        // Only the registered-but-behaviourless feature is refused.
        for feature in ConsensusFeature::ALL {
            assert_eq!(
                feature.has_behaviour(),
                !matches!(feature, ConsensusFeature::StakeSubmissionNonce),
                "{feature}"
            );
        }
    }

    #[test]
    fn schedulable_features_are_pinned_3277() {
        // On a chain instance no faucet-binding row names (and on a node
        // with no block-1 hash): the S4 table.
        let other = "cd".repeat(32);
        let mut expected = S4_SCHEDULABLE.to_vec();
        expected.push("node_share_settlement");
        for node in [None, Some(other.as_str())] {
            let schedulable: Vec<&str> = ConsensusFeature::ALL
                .iter()
                .filter(|f| f.schedulable(node))
                .map(|f| f.name())
                .collect();
            assert_eq!(schedulable, expected, "{node:?}");
        }
        for held in [
            ConsensusFeature::SigningV2,
            ConsensusFeature::Paymaster,
            ConsensusFeature::SocialRecovery,
            ConsensusFeature::SessionKeys,
            ConsensusFeature::FaucetSinkBinding,
            ConsensusFeature::ScheduleExtension,
            ConsensusFeature::StakeSubmissionNonce,
            ConsensusFeature::DeviceSubmissionLimit,
            ConsensusFeature::StreakReplayNodeService,
        ] {
            assert!(!held.schedulable(None), "{held} must not be schedulable yet");
        }
        // #3361 F3: on the chain instance the 777 row names, and only there,
        // the faucet binding joins the table.
        let row_chain = LEGACY_FAUCET_BINDINGS[0].genesis_hash;
        let schedulable: Vec<&str> = ConsensusFeature::ALL
            .iter()
            .filter(|f| f.schedulable(Some(row_chain)))
            .map(|f| f.name())
            .collect();
        let mut expected = S4_SCHEDULABLE.to_vec();
        // In `ALL` order: the faucet binding precedes #3270's feature.
        expected.insert(7, "faucet_sink_binding");
        expected.push("node_share_settlement");
        assert_eq!(schedulable, expected);
    }

    const S4_SCHEDULABLE: [&str; 12] = [
        "domain_separated_preimages",
        "account_authorization_dispatch",
        "verify_unit_pricing",
        "passkey_p256_auth",
        "reward_gross_outlier_guard",
        "reference_sea_level_pressure",
        "station_owner_slash_identity",
        // #3270 S9: E0 schedules it (design R6, ruling D7).
        "weather_skew_block_time_allowance",
        "station_owner_consent",
        "station_active_reward_gate",
        "station_owner_actions",
        "station_registration_nonce",
    ];

    /// #3361 F3: the row gate. A `faucet_sink_binding*` name is schedulable
    /// only under a row carrying exactly that name, and only on the chain
    /// instance the row names; a node with no block-1 hash fails closed.
    #[test]
    fn faucet_binding_row_gate_matches_name_and_chain_instance_3361() {
        let row = &LEGACY_FAUCET_BINDINGS[0];
        assert_eq!(faucet_binding_row_for("faucet_sink_binding", Some(row.genesis_hash)), Ok(row));
        assert_eq!(
            faucet_binding_row_for("faucet_sink_binding", None),
            Err(FaucetBindingRowError::NoChainInstance {
                feature: "faucet_sink_binding".into()
            })
        );
        let other = "cd".repeat(32);
        let err = faucet_binding_row_for("faucet_sink_binding", Some(&other)).unwrap_err();
        assert!(matches!(err, FaucetBindingRowError::OtherChainInstance { .. }), "{err:?}");
        assert!(err.to_string().contains("not schedulable here"), "{err}");
        // Case and prefix spellings are not the canonical hash.
        for spelling in [row.genesis_hash.to_ascii_uppercase(), format!("0x{}", row.genesis_hash)] {
            assert!(faucet_binding_row_for("faucet_sink_binding", Some(&spelling)).is_err());
        }
        for name in ["faucet_sink_binding_r2", "faucet_sink_binding_", "faucet_sink_bindings"] {
            assert_eq!(
                faucet_binding_row_for(name, Some(row.genesis_hash)),
                Err(FaucetBindingRowError::NoRow { feature: name.into() }),
                "{name}"
            );
        }
        assert!("faucet_sink_binding_r2".starts_with(FAUCET_SINK_BINDING_PREFIX));
        assert_eq!(FAUCET_SINK_BINDING_PREFIX, ConsensusFeature::FaucetSinkBinding.name());
    }

    fn mainnet() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-main", "mainnet")
    }

    fn key() -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(2123);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        (sk, pk)
    }

    const TAG: &[u8] = b"wrfcoin.test_object.v1";

    // ---- cross-chain-id replay ----

    #[test]
    fn identical_body_under_two_chain_ids_produces_different_signatures() {
        let (sk, _) = key();
        let body = b"identical-body";

        let on_testnet = domain_preimage_bound(TAG, &testnet(), SigningProtocolVersion::V2, body);
        let on_mainnet = domain_preimage_bound(TAG, &mainnet(), SigningProtocolVersion::V2, body);

        assert_ne!(on_testnet, on_mainnet);
        assert_ne!(sk.sign(&on_testnet).to_hex(), sk.sign(&on_mainnet).to_hex());
    }

    #[test]
    fn a_testnet_signature_does_not_verify_on_mainnet_and_vice_versa() {
        // The property the whole story exists for: a signature harvested from
        // the testnet must be worthless on mainnet.
        let (sk, pk) = key();
        let body = b"transfer-1000-wrf";

        let testnet_preimage =
            domain_preimage_bound(TAG, &testnet(), SigningProtocolVersion::V2, body);
        let mainnet_preimage =
            domain_preimage_bound(TAG, &mainnet(), SigningProtocolVersion::V2, body);

        let testnet_sig = sk.sign(&testnet_preimage);
        let mainnet_sig = sk.sign(&mainnet_preimage);

        assert!(pk.verify(&testnet_preimage, &testnet_sig));
        assert!(pk.verify(&mainnet_preimage, &mainnet_sig));

        assert!(
            !pk.verify(&mainnet_preimage, &testnet_sig),
            "a testnet signature must not verify on mainnet"
        );
        assert!(
            !pk.verify(&testnet_preimage, &mainnet_sig),
            "a mainnet signature must not verify on testnet"
        );
    }

    #[test]
    fn network_id_alone_separates_two_chains_sharing_a_chain_id() {
        let a = ChainIdentity::new("wrfcoin-main", "mainnet");
        let b = ChainIdentity::new("wrfcoin-main", "shadow");
        assert_ne!(
            domain_preimage_bound(TAG, &a, SigningProtocolVersion::V2, b"x"),
            domain_preimage_bound(TAG, &b, SigningProtocolVersion::V2, b"x")
        );
    }

    // ---- cross-version replay ----

    #[test]
    fn protocol_version_change_invalidates_a_signature() {
        let (sk, pk) = key();
        let body = b"same-body-same-chain";

        let v1 = domain_preimage_bound(TAG, &mainnet(), SigningProtocolVersion::V1, body);
        let v2 = domain_preimage_bound(TAG, &mainnet(), SigningProtocolVersion::V2, body);

        assert_ne!(v1, v2);

        let v1_sig = sk.sign(&v1);
        assert!(pk.verify(&v1, &v1_sig));
        assert!(
            !pk.verify(&v2, &v1_sig),
            "a v1 signature must not verify under v2 — this is the #2076 failure mode"
        );
    }

    #[test]
    fn v1_is_byte_identical_to_the_unbound_construction() {
        // The gate: merging #2123 must not change a single byte until V2 is
        // deliberately selected. Chain identity is ignored entirely at V1.
        use crate::crypto::domain_signing::domain_preimage;
        let body = b"body";
        assert_eq!(
            domain_preimage_bound(TAG, &testnet(), SigningProtocolVersion::V1, body),
            domain_preimage(TAG, body)
        );
        assert_eq!(
            domain_preimage_bound(TAG, &mainnet(), SigningProtocolVersion::V1, body),
            domain_preimage_bound(TAG, &testnet(), SigningProtocolVersion::V1, body),
            "identity must not affect V1 bytes"
        );
    }

    // ---- coordinated activation (#2123, review of PR #2232) ----

    fn genesis_meta(version: Option<&str>) -> Vec<(&str, &str)> {
        let mut m = vec![("chain_id", "wrfcoin-testnet"), ("network_id", "testnet")];
        if let Some(v) = version {
            m.push((GENESIS_PROTOCOL_VERSION_KEY, v));
        }
        m
    }

    #[test]
    fn a_single_node_cannot_activate_v2_by_local_configuration() {
        // THE finding. `WRFCOIN_SIGNING_PROTOCOL_VERSION=2` on a chain whose
        // genesis does not pin v2 must REFUSE, not quietly run v2 while peers
        // and external signers stay v1. That asymmetry is #2076 exactly.
        let err = resolve_active_version(None, Some(SigningProtocolVersion::V2), &testnet())
            .expect_err("unilateral v2 must refuse to start");
        assert_eq!(
            err,
            ActivationError::UnilateralActivation {
                configured: SigningProtocolVersion::V2,
                genesis: None,
            }
        );
    }

    #[test]
    fn existing_chains_still_start_at_v1() {
        // Pre-#2123 genesis records no version. Every node in existence is in
        // this state, so it must remain a clean start — a fail-closed gate that
        // refuses everyone is not a gate, it is an outage.
        assert_eq!(
            resolve_active_version(None, None, &testnet()),
            Ok(SigningProtocolVersion::V1)
        );
        assert_eq!(
            resolve_active_version(None, Some(SigningProtocolVersion::V1), &testnet()),
            Ok(SigningProtocolVersion::V1)
        );
    }

    #[test]
    fn genesis_pinned_v2_activates_without_any_local_setting() {
        // The coordinated path: the chain decides, the node follows. No env var
        // needed, which is what makes activation chain-wide rather than local.
        assert_eq!(
            resolve_active_version(Some(SigningProtocolVersion::V2), None, &testnet()),
            Ok(SigningProtocolVersion::V2)
        );
    }

    #[test]
    fn a_v1_node_refuses_to_join_a_v2_chain() {
        // The other half of the straddle: a lagging node must not sign v1
        // against a v2 chain.
        assert_eq!(
            resolve_active_version(
                Some(SigningProtocolVersion::V2),
                Some(SigningProtocolVersion::V1),
                &testnet()
            ),
            Err(ActivationError::VersionMismatch {
                configured: SigningProtocolVersion::V1,
                genesis: SigningProtocolVersion::V2,
            })
        );
    }

    #[test]
    fn a_v2_node_refuses_to_join_a_v1_chain() {
        assert_eq!(
            resolve_active_version(
                Some(SigningProtocolVersion::V1),
                Some(SigningProtocolVersion::V2),
                &testnet()
            ),
            Err(ActivationError::VersionMismatch {
                configured: SigningProtocolVersion::V2,
                genesis: SigningProtocolVersion::V1,
            })
        );
    }

    #[test]
    fn v2_refuses_to_start_on_the_default_fallback_identity() {
        // At v2 the identity is inside every preimage, so a testnet node left on
        // the mainnet fallback would mint mainnet-identified signatures.
        let fallback = ChainIdentity::new(DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID);
        assert_eq!(
            resolve_active_version(Some(SigningProtocolVersion::V2), None, &fallback),
            Err(ActivationError::DefaultIdentityAtV2 { identity: fallback })
        );
    }

    #[test]
    fn v1_on_the_default_identity_is_still_allowed() {
        // The fallback is only dangerous once the identity is inside preimages.
        // Refusing it at v1 would break every existing mainnet node.
        let fallback = ChainIdentity::new(DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID);
        assert_eq!(
            resolve_active_version(None, None, &fallback),
            Ok(SigningProtocolVersion::V1)
        );
    }

    #[test]
    fn an_unparseable_genesis_version_refuses_rather_than_falling_back() {
        // Falling back to v1 here would put this node on a different encoding
        // from the chain, silently. Refusing is the only safe answer.
        let err = genesis_protocol_version(genesis_meta(Some("v9")))
            .expect_err("an unknown genesis version must refuse");
        assert_eq!(
            err,
            ActivationError::UnknownGenesisVersion {
                raw: "v9".to_string()
            }
        );
    }

    #[test]
    fn genesis_version_is_read_when_present_and_absent() {
        assert_eq!(
            genesis_protocol_version(genesis_meta(Some("2"))),
            Ok(Some(SigningProtocolVersion::V2))
        );
        assert_eq!(genesis_protocol_version(genesis_meta(None)), Ok(None));
    }

    #[test]
    fn startup_gate_accepts_a_coherent_v2_deployment() {
        let installed = std::cell::Cell::new(None);
        let version = assert_startup_protocol_compatibility_with_installer(
            genesis_meta(Some("2")),
            Some(SigningProtocolVersion::V2),
            &testnet(),
            |resolved| {
                installed.set(Some(resolved));
                resolved
            },
        )
        .expect("a coherent v2 deployment must start");
        assert_eq!(version, SigningProtocolVersion::V2);
        assert_eq!(installed.get(), Some(SigningProtocolVersion::V2));
        assert_eq!(
            SigningProtocolVersion::try_active(),
            None,
            "unit tests must not install the process-wide startup version"
        );
    }

    #[test]
    fn startup_gate_refuses_a_mixed_fleet_before_block_production() {
        // The end-to-end proof the review asked for: a node configured for v2
        // against a chain that is not v2 does not reach block production.
        let err = assert_startup_protocol_compatibility(
            genesis_meta(None),
            Some(SigningProtocolVersion::V2),
            &testnet(),
        )
        .expect_err("a mixed v1/v2 fleet must refuse to start");
        let rendered = err.to_string();
        assert!(
            rendered.contains("cannot activate a protocol version by itself"),
            "the refusal must name the cause, got: {rendered}"
        );
        assert!(
            rendered.contains("2076"),
            "the refusal should cite the precedent"
        );
    }

    #[test]
    fn startup_gate_still_catches_identity_mismatch() {
        // The identity check from the original submission, now reachable
        // through the single gate a node actually calls.
        let err = assert_startup_protocol_compatibility(genesis_meta(None), None, &mainnet())
            .expect_err("configured mainnet against testnet genesis must refuse");
        assert!(err.to_string().contains("chain identity mismatch"));
    }

    // ---- startup assertion ----

    #[test]
    fn startup_refuses_on_genesis_chain_id_mismatch() {
        let err = assert_chain_identity_matches_genesis(&testnet(), Some(&mainnet()))
            .expect_err("mismatched identity must refuse to start");
        assert_eq!(
            err,
            ChainIdentityError::GenesisMismatch {
                configured: testnet(),
                genesis: mainnet(),
            }
        );
    }

    #[test]
    fn startup_refuses_when_only_network_id_differs() {
        let configured = ChainIdentity::new("wrfcoin-main", "mainnet");
        let genesis = ChainIdentity::new("wrfcoin-main", "testnet");
        assert!(matches!(
            assert_chain_identity_matches_genesis(&configured, Some(&genesis)),
            Err(ChainIdentityError::GenesisMismatch { .. })
        ));
    }

    #[test]
    fn startup_accepts_matching_identity() {
        assert!(assert_chain_identity_matches_genesis(&testnet(), Some(&testnet())).is_ok());
    }

    #[test]
    fn startup_accepts_genesis_without_a_recorded_identity() {
        // Every chain created before #2123 is in this state. Refusing here
        // would refuse to start every existing node.
        assert!(assert_chain_identity_matches_genesis(&testnet(), None).is_ok());
    }

    #[test]
    fn startup_refuses_empty_identifiers() {
        assert_eq!(
            assert_chain_identity_matches_genesis(&ChainIdentity::new("  ", "testnet"), None),
            Err(ChainIdentityError::EmptyIdentifier { field: "chain_id" })
        );
        assert_eq!(
            assert_chain_identity_matches_genesis(&ChainIdentity::new("wrfcoin", ""), None),
            Err(ChainIdentityError::EmptyIdentifier {
                field: "network_id"
            })
        );
    }

    #[test]
    fn genesis_identity_reads_from_metadata() {
        let metadata = [
            ("chain_id", "wrfcoin-testnet"),
            ("network_id", "testnet"),
            ("unrelated", "ignored"),
        ];
        assert_eq!(
            ChainIdentity::from_genesis_metadata(metadata),
            Some(testnet())
        );
    }

    #[test]
    fn genesis_identity_is_none_when_either_key_is_absent() {
        assert_eq!(
            ChainIdentity::from_genesis_metadata([("chain_id", "wrfcoin-testnet")]),
            None
        );
        assert_eq!(
            ChainIdentity::from_genesis_metadata([("network_id", "testnet")]),
            None
        );
        assert_eq!(ChainIdentity::from_genesis_metadata([]), None);
    }

    // ---- network magic ----

    #[test]
    fn network_magic_differs_between_networks() {
        assert_ne!(testnet().network_magic(), mainnet().network_magic());
    }

    #[test]
    fn network_magic_is_deterministic() {
        assert_eq!(testnet().network_magic(), testnet().network_magic());
    }

    // ---- version parsing ----

    #[test]
    fn version_parsing_accepts_documented_forms_and_rejects_others() {
        assert_eq!(
            SigningProtocolVersion::parse("1"),
            Some(SigningProtocolVersion::V1)
        );
        assert_eq!(
            SigningProtocolVersion::parse("v2"),
            Some(SigningProtocolVersion::V2)
        );
        assert_eq!(
            SigningProtocolVersion::parse(" V2 "),
            Some(SigningProtocolVersion::V2)
        );
        assert_eq!(SigningProtocolVersion::parse("3"), None);
        assert_eq!(SigningProtocolVersion::parse(""), None);
        assert_eq!(SigningProtocolVersion::parse("latest"), None); // secret-leak-ok: public protocol-version test contains no secret material
    }

    #[test]
    fn version_default_is_v1() {
        // Fail closed. A typo in a deployment variable must not activate a fork.
        assert_eq!( // secret-leak-ok: public protocol-version test contains no secret material
            SigningProtocolVersion::default(),
            SigningProtocolVersion::V1
        );
    }

    #[test]
    fn default_fallback_identity_is_detectable() {
        assert!(ChainIdentity::new(DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID).is_default_fallback());
        assert!(!testnet().is_default_fallback());
    }

    // ── #2545: an uncut chain may choose its version; an existing one may not ──

    fn testnet_identity() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-testnet", "testnet")
    }

    /// THE POINT OF #2545. A chain with no blocks may be cut at V2.
    #[test]
    fn an_uncut_chain_may_be_cut_at_v2() {
        let v = resolve_version_for_uncut_chain(
            Some(SigningProtocolVersion::V2),
            &testnet_identity(),
        )
        .expect("an uncut chain's configuration DEFINES genesis");
        assert_eq!(v, SigningProtocolVersion::V2);
    }

    /// THE PROTECTION THAT MUST SURVIVE (#2076). A chain that HAS blocks and
    /// whose genesis pins nothing still refuses V2 — unchanged behaviour.
    ///
    /// This is the RED control for the whole change: if #2545 had been fixed by
    /// relaxing `resolve_active_version` instead of separating the two states,
    /// this test would fail and a v1 signer could run against v2 verifiers.
    #[test]
    fn an_existing_chain_pinning_nothing_still_refuses_v2() {
        let err = resolve_active_version(
            None, // genesis records no version, but blocks EXIST
            Some(SigningProtocolVersion::V2),
            &testnet_identity(),
        )
        .expect_err("#2076: a node must not activate v2 by itself on an existing chain");
        assert!(matches!(err, ActivationError::UnilateralActivation { .. }));
    }

    /// An uncut chain still may not be cut at V2 on the default identity —
    /// that guard is unchanged, and without it V2 would mint
    /// mainnet-identified signatures on some other network.
    #[test]
    fn an_uncut_chain_still_refuses_v2_on_the_default_identity() {
        let err = resolve_version_for_uncut_chain(
            Some(SigningProtocolVersion::V2),
            &ChainIdentity::new(DEFAULT_CHAIN_ID, DEFAULT_NETWORK_ID),
        )
        .expect_err("v2 on the unconfigured fallback identity must still refuse");
        assert!(matches!(err, ActivationError::DefaultIdentityAtV2 { .. }));
    }

    /// Silence still means V1 — cutting does not opt anyone into V2 by accident.
    #[test]
    fn an_uncut_chain_defaults_to_v1_when_unconfigured() {
        assert_eq!(
            resolve_version_for_uncut_chain(None, &testnet_identity()).unwrap(),
            SigningProtocolVersion::V1
        );
    }

}

/// #3361 option A (F1): the faucet sink binding's committed data, resolver
/// precedence, and the guards that keep it inert until scheduled.
#[cfg(test)]
mod faucet_sink_binding_tests_3361 {
    use super::*;
    use crate::common::types::Address;

    const SINK: Address = Address([0x39; 20]);

    fn config(chain_id: &str, faucet_address: Option<Address>) -> GenesisEmissionConfig {
        GenesisEmissionConfig { chain_id: chain_id.into(), faucet_address , ..Default::default() }
    }

    fn row(
        feature: ConsensusFeature,
        chain: &'static str,
        address: Option<&'static str>,
    ) -> LegacyFaucetBinding {
        LegacyFaucetBinding { feature, genesis_chain_id: chain, genesis_hash: "ab", address }
    }

    fn hex(byte: u8) -> &'static str {
        Box::leak(format!("{byte:02x}").repeat(20).into_boxed_str())
    }

    /// The committed row, pinned byte for byte against the owner's ruling
    /// (#3361 Q1/Q4, 2026-09-28) and against the chain's own records.
    #[test]
    fn the_committed_777_row_is_the_owner_ruled_binding() {
        assert_eq!(
            LEGACY_FAUCET_BINDINGS,
            &[LegacyFaucetBinding {
                feature: ConsensusFeature::FaucetSinkBinding,
                genesis_chain_id: "777",
                genesis_hash: "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8", // pragma: allowlist secret -- public genesis hash
                address: Some("f72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"), // pragma: allowlist secret -- public faucet wallet ADDRESS
            }]
        );
        let bound = LEGACY_FAUCET_BINDINGS[0];
        let address = Address::from_hex(bound.address.unwrap()).unwrap();
        assert_ne!(address, Address([0; 20]), "never the zero address");
        assert_eq!(address.to_string(), format!("0x{}", bound.address.unwrap()));
        assert_eq!(bound.feature.name(), "faucet_sink_binding");
        assert_eq!(ConsensusFeature::parse("faucet_sink_binding"), Some(bound.feature));
        // Every row: a known feature, a parseable sink (or explicit unbind),
        // a 64-hex policy hash, and one row per feature name.
        for (i, binding) in LEGACY_FAUCET_BINDINGS.iter().enumerate() {
            assert!(ConsensusFeature::ALL.contains(&binding.feature));
            if let Some(address) = binding.address {
                assert_eq!(address.len(), 40);
                assert_eq!(address, address.to_ascii_lowercase());
                Address::from_hex(address).unwrap();
            }
            assert_eq!(binding.genesis_hash.len(), 64);
            assert!(binding.genesis_hash.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(
                LEGACY_FAUCET_BINDINGS[..i].iter().all(|earlier| earlier.feature != binding.feature),
                "a rotation is a new feature name, never a re-used one"
            );
        }
    }

    /// Precedence (#3361): genesis address, else the latest-declared active
    /// matching row, else none.
    #[test]
    fn resolver_precedence_is_genesis_then_latest_active_row_then_none() {
        let first = ConsensusFeature::FaucetSinkBinding;
        let second = ConsensusFeature::StationOwnerSlashIdentity; // stand-in for a future `_r2`
        let rows = [row(first, "777", Some(hex(0xa1))), row(second, "777", Some(hex(0xb2)))];
        let at = |active: Vec<ConsensusFeature>| {
            move |feature: ConsensusFeature, _height: u64| active.contains(&feature)
        };
        let a1 = Address([0xa1; 20]);
        let b2 = Address([0xb2; 20]);

        // No recorded genesis config: nothing to key on, nothing paid.
        assert_eq!(resolve_faucet_sink(None, 5, &rows, at(vec![first, second])), None);
        // Genesis wins over every active row.
        assert_eq!(
            resolve_faucet_sink(Some(&config("777", Some(SINK))), 5, &rows, at(vec![first, second])),
            Some(SINK)
        );
        // Unscheduled: none.
        assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 5, &rows, at(vec![])), None);
        assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 5, &rows, at(vec![first])), Some(a1));
        // The LATEST-declared active row wins, whichever activated first.
        assert_eq!(
            resolve_faucet_sink(Some(&config("777", None)), 5, &rows, at(vec![first, second])),
            Some(b2)
        );
        assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 5, &rows, at(vec![second])), Some(b2));
        // Keyed on the recorded chain id: another chain never matches.
        for other in ["778", "wrfcoin-main", "777 ", ""] {
            assert_eq!(
                resolve_faucet_sink(Some(&config(other, None)), 5, &rows, at(vec![first, second])),
                None,
                "{other:?}"
            );
        }
        // Height is forwarded to the predicate as given.
        let gated = |feature: ConsensusFeature, height: u64| feature == first && height >= 10;
        assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 9, &rows, gated), None);
        assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 10, &rows, gated), Some(a1));
    }

    /// An unbind row, or a row whose address does not parse, resolves to
    /// NONE — it never falls through to an older row, and never to the zero
    /// address.
    #[test]
    fn an_unbind_or_malformed_latest_row_mints_to_nobody() {
        let first = ConsensusFeature::FaucetSinkBinding;
        let second = ConsensusFeature::StationOwnerSlashIdentity;
        let both = |_: ConsensusFeature, _: u64| true;
        for latest in [None, Some("not-an-address"), Some("f72059")] {
            let rows = [row(first, "777", Some(hex(0xa1))), row(second, "777", latest)];
            assert_eq!(resolve_faucet_sink(Some(&config("777", None)), 5, &rows, both), None, "{latest:?}");
        }
        // A row for another chain declared later does not shadow this chain's.
        let rows = [row(first, "777", Some(hex(0xa1))), row(second, "778", None)];
        assert_eq!(
            resolve_faucet_sink(Some(&config("777", None)), 5, &rows, both),
            Some(Address([0xa1; 20]))
        );
    }

    /// The production binding is inert unless a schedule names it: with no
    /// extension naming it, it is inactive at every height, and the live
    /// chain's shape resolves to no sink.
    #[test]
    fn the_binding_is_unscheduled_and_resolves_nothing_by_default() {
        assert!(
            !feature_schedule_extension_entries().iter().any(|(n, _)| n == "faucet_sink_binding")
        );
        assert!(
            try_feature_schedule()
                .is_none_or(|base| base.activation_height("faucet_sink_binding").is_none())
        );
        for height in [0, 1, 2, 23_000, 43_000, u64::MAX] {
            assert!(!feature_active(ConsensusFeature::FaucetSinkBinding, height));
            assert_eq!(
                resolve_faucet_sink(
                    Some(&config("777", None)),
                    height,
                    LEGACY_FAUCET_BINDINGS,
                    feature_active
                ),
                None
            );
        }
    }

    /// The resolver is a function of recorded state and the effective
    /// schedule only: it reads no environment, process config, clock, or the
    /// boot-installed extension authority (a node whose authority was not yet
    /// installed would otherwise mint differently from its peers).
    #[test]
    fn the_resolver_reads_no_environment_config_or_authority() {
        let this = include_str!("chain_binding.rs");
        let state = include_str!("../blockchain/state.rs");
        // The item's text up to its closing brace at `indent`.
        let body = |source: &'static str, signature: &str, indent: &str| -> &'static str {
            let start = source.find(signature).unwrap_or_else(|| panic!("{signature} not found"));
            let rest = &source[start..];
            &rest[..rest.find(&format!("\n{indent}}}")).unwrap()]
        };
        for (what, text) in [
            ("resolve_faucet_sink", body(this, "pub fn resolve_faucet_sink(", "")),
            ("effective_faucet_address", body(state, "pub fn effective_faucet_address(", "    ")),
        ] {
            for forbidden in [
                "env::", "env!(", "var(", "Config::", "settings", "OnceLock", "authority", "SystemTime", "Instant",
                "genesis_hash",
            ] {
                assert!(!text.contains(forbidden), "{what} must not read `{forbidden}`:\n{text}");
            }
        }
        // And the arm pays whatever the resolver returns — no second source.
        let arm = body(state, "StateTransition::ApplyFaucetReward => {", "            ");
        assert!(arm.contains("self.effective_faucet_address()"), "{arm}");
        assert!(!arm.contains("faucet_address)"), "the arm must not read the genesis field directly");
    }
}
