//! Weather-station registration artifact builder (#2537).
//!
//! #2569 landed the `StationRegistration` transaction type and its consensus
//! application, but nothing that could *produce* one outside a test: every
//! construction site was `#[cfg(test)]`. This is the operator-facing producer,
//! deliberately modelled on `oracle build-registrations` (#2530).
//!
//! `build-registration` is offline. It reads an owner key or a supplied owner
//! proof plus a governance key and writes a
//! `station_registration_artifact_v1` document — no node contact, and no
//! wall-clock read reaches the signed bytes (`--submitted-at` is pinned by the
//! caller). Two operators running it on identical inputs get a byte-identical
//! transaction, including its id.
//!
//! Signing order is load-bearing and enforced here: the owner possession proof
//! is produced first because `owner_pubkey` is folded into the governance
//! preimage. Governance therefore authorizes *this owner key for this station*,
//! and a harvested governance signature cannot be re-paired with a different
//! owner's proof.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::DateTime;
use clap::Subcommand;
use serde::Deserialize;
use serde_json::{Value, json};

use wrfcoin_cli::constants::MAX_STATION_ARTIFACT_BYTES;
use wrfcoin_cli::errors::{CliError, CliErrorCode};
use wrfcoin_shared_protocol::blockchain::transaction::{
    ComputeReceipt, OracleAttestationPayload, StationRegistrationPayload, Transaction,
    commit_authorization_message_with_domain, station_ownership_message_with_domain,
};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    CHAIN_ID_ENV, NETWORK_ID_ENV, is_authorized_oracle_governance, signing_chain_id,
    signing_network_id,
};
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, validate_station_id,
};

fn blake3_hex(bytes: &[u8]) -> String {
    hash_bytes(bytes).to_hex()
}

pub const STATION_ARTIFACT_VERSION: &str = "station_registration_artifact_v1";

/// The self-verification internal-defect diagnostic (#2744 / #2880 review).
///
/// The exact diagnostic is retained for operators and wrapped in a typed
/// `CliError`, so its prose cannot select a remediation hint.
pub(crate) const INTERNAL_SELF_VERIFY_DEFECT_MSG: &str = concat!(
    "WRFCOIN_CLI_INTERNAL_DEFECT: self-verification failed: the governance key is ",
    "allowlisted and the signatures were verified against the same ",
    "--chain-id/--network-id values they were signed with, so a signature produced ",
    "by this build did not verify against its own inputs. This is a defect in the ",
    "CLI, not an operator configuration error — report it rather than re-deriving ",
    "keys."
);

#[derive(Subcommand)]
pub enum StationCommands {
    /// Owner-signed station actions (transfer, accept, report-stolen, unfreeze,
    /// deactivate, reactivate, recover-file/-accept/-cancel/-execute): #3345 / #3347.
    #[clap(flatten)]
    Owner(super::station_owner::StationOwnerCommands),

    /// Build a signed station-registration transaction offline.
    BuildRegistration {
        /// Station identifier as it will appear in `station_registry` and in
        /// every commit/reveal this station submits.
        #[clap(long, required_unless_present = "batch", conflicts_with = "batch")]
        station_id: Option<String>,

        /// Contributor address that owns the station and receives its rewards.
        #[clap(long, required_unless_present = "batch", conflicts_with = "batch")]
        owner_address: Option<String>,

        /// Owner's ed25519 private key file — proves possession.
        #[clap(long, required_unless_present_any = ["owner_pubkey", "batch"],
            conflicts_with_all = ["owner_pubkey", "owner_signature", "batch"])]
        owner_key: Option<PathBuf>,

        /// Owner's public ed25519 key (hex); requires --owner-signature.
        #[clap(long, requires = "owner_signature", conflicts_with = "batch")]
        owner_pubkey: Option<String>,

        /// Pre-made ownership signature (hex) over the exact chain/network/id/address.
        #[clap(long, requires = "owner_pubkey", conflicts_with = "batch")]
        owner_signature: Option<String>,

        /// JSON array of registration rows; see station-registration-batch.md.
        /// All rows are checked before governance signing or writing the artifact.
        #[clap(long, conflicts_with_all = ["elevation_meters", "hardware_model",
            "provenance_class", "data_quality_tier"])]
        batch: Option<PathBuf>,

        /// Oracle-governance private key file. Must correspond to a pubkey in
        /// the node's `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS` allowlist. This is the
        /// reward-authorising signature.
        #[clap(long)]
        governance_key: PathBuf,

        /// Chain id the registration is signed for. Required, never inferred:
        /// it is folded into the signature preimage but not stored in the
        /// payload, so a wrong value produces a registration that fails on
        /// every peer with no diagnostic.
        #[clap(long)]
        chain_id: String,

        /// Network id.
        #[clap(long, default_value = "testnet")]
        network_id: String,

        #[clap(
            long,
            allow_hyphen_values = true,
            required_unless_present = "batch",
            conflicts_with = "batch"
        )]
        latitude: Option<f64>,

        #[clap(
            long,
            allow_hyphen_values = true,
            required_unless_present = "batch",
            conflicts_with = "batch"
        )]
        longitude: Option<f64>,

        #[clap(long, allow_hyphen_values = true)]
        elevation_meters: Option<f64>,

        #[clap(long, default_value = "reference-station")]
        hardware_model: String,

        /// Reward classification. Only `real` is reward-eligible; `synthetic`
        /// and `unclassified` earn nothing, by consensus, at seven enforcement
        /// sites. This is the field the governance signature authorizes.
        #[clap(long, default_value = "real")]
        provenance_class: String,

        /// Data quality tier: unverified | bronze | silver | gold.
        #[clap(long, default_value = "gold")]
        data_quality_tier: String,

        /// Transaction timestamp (unix seconds). Pinned rather than read from
        /// the clock so the artifact is reproducible; the submission route
        /// rejects anything more than half the assembly window from now.
        #[clap(long, required_unless_present = "batch", conflicts_with = "batch")]
        submitted_at: Option<i64>,

        /// Build a v2 (nonce-bound) registration for the station's current
        /// `owner_nonce` (read it from the node's owner-ledger route; 0 for a
        /// station with no ledger entry). Required once
        /// `station_registration_nonce` is active; v1 (omit this) is refused
        /// from then on. In a --batch, give `expected_nonce` per row instead.
        #[clap(long, conflicts_with = "batch")]
        expected_nonce: Option<u64>,

        /// Output artifact path.
        #[clap(long)]
        out: PathBuf,
    },

    /// Build the commit and reveal request bodies for one observation.
    ///
    /// Emits both, because they share a `commit_id` that must be derived
    /// **once**: SHA3-256 over the 5-field preimage
    /// `data || nonce || station_id || timestamp || contribution_type`, with
    /// `data` serialized as sorted-key JSON. Deriving it twice by hand is how
    /// the reveal stops matching the commit.
    BuildObservation {
        #[clap(long)]
        station_id: String,

        /// Contributor address — this is the account whose balance must move.
        #[clap(long)]
        contributor: String,

        /// Key that signs the commit authorization (#1848). Any keypair may
        /// sign a commit; reward eligibility is decided by the station's
        /// registered provenance class, not by this signer.
        #[clap(long)]
        signer_key: PathBuf,

        #[clap(long)]
        chain_id: String,

        #[clap(long, default_value = "testnet")]
        network_id: String,

        #[clap(long)]
        nonce: String,

        #[clap(long)]
        timestamp: i64,

        /// Observation JSON object.
        #[clap(long)]
        data: String,

        /// Optional worker key. When supplied, a SIGNED ComputeReceipt is
        /// attached to the reveal body.
        ///
        /// This is the escape the zero-fee weather gate is built around: at a
        /// non-zero `min_work_factor` a zero-fee reveal is dropped by the
        /// producer self-screen unless it carries a receipt whose signature
        /// verifies and whose `work_factor` clears the floor. Nothing in either
        /// repo ships a producer for these — every constructor in the workspace
        /// is `#[cfg(test)]` — so this flag exists to demonstrate that the gate
        /// is the ONLY thing standing between a real observation and a reward
        /// at mainnet posture.
        #[clap(long)]
        compute_receipt_key: Option<PathBuf>,

        /// `work_factor` to claim on the receipt. Must be >=
        /// `effective_min_work_factor(height)` or the reveal is still dropped.
        #[clap(long, default_value_t = 1000)]
        work_factor: u64,

        #[clap(long)]
        out: PathBuf,
    },

    /// Build a signed oracle attestation body for a commit id.
    ///
    /// The attestation signing message is a DIFFERENT recipe from the commit-id
    /// preimage — it is the domain-separated
    /// `OracleAttestationPayload::canonical_signing_message`. Signing the
    /// commit-id preimage produces a signature the node rejects.
    BuildAttestation {
        #[clap(long)]
        commit_id: String,

        /// Oracle identity — must be a member of the genesis oracle set.
        #[clap(long)]
        oracle_id: String,

        /// That oracle's private key file.
        #[clap(long)]
        oracle_key: PathBuf,

        #[clap(long)]
        timestamp: i64,

        #[clap(long)]
        nonce: String,

        #[clap(long)]
        out: PathBuf,
    },
}

pub(crate) fn read_key(path: &PathBuf) -> Result<PrivateKey> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read key file {}", path.display()))?;
    PrivateKey::from_hex(raw.trim())
        .map_err(|e| anyhow!("key file {} is not valid ed25519 hex: {e}", path.display()))
}

/// Replace `value` with its `serde_json` wire fixed point (#3333).
///
/// The station preimage encodes each coordinate's raw IEEE-754 bits, and every
/// verifier (node admission, producer, P2P applier, WAL replay, and this
/// CLI's self-check) recovers those bits by parsing `tx.data` with
/// `serde_json`. Its default float parser is not correctly rounded (the
/// `float_roundtrip` feature is off workspace-wide), so for some inputs, such as
/// `-92.04194742607375`, the parsed value is one ulp away from what clap
/// parsed. The signature then fails on every peer.
///
/// Iterating serialize→parse until the bits stop changing yields a value that
/// IS its own parse image, so the bits signed here are the bits every peer
/// verifies. The change is at most one ulp (about 1e-14 degrees, sub-nanometre).
/// Verification, the preimage layout and the txid rules are untouched. This is
/// the same pattern as `wire_stable_confidence` in `oracle/mechanical.rs`.
/// Removing the parse dependency from the preimage itself is a consensus
/// change, left for a scheduled feature.
// determinism-ok: builder-side only; this makes the signed bits equal to the node's own serde_json parse image, and no float arithmetic is performed
fn wire_stable_f64(field: &str, value: f64) -> Result<f64> {
    let mut current = value;
    for _ in 0..8 {
        let printed = serde_json::to_string(&current)
            .map_err(|e| anyhow!("{field} {current} cannot be serialized: {e}"))?;
        // determinism-ok: deliberately mirrors the node's serde_json parse; matching it is the point
        let parsed: f64 = serde_json::from_str(&printed)
            .map_err(|e| anyhow!("{field} {current} does not parse back as a number: {e}"))?;
        if parsed.to_bits() == current.to_bits() {
            return Ok(current);
        }
        current = parsed;
    }
    Err(anyhow!(
        "{field} {value} has no stable JSON representation (#3333); round it \
         (for example to 9 decimal places) and rebuild"
    ))
}

fn parse_provenance(raw: &str) -> Result<ContributionProvenanceClass> {
    raw.parse::<ContributionProvenanceClass>()
        .map_err(|e| anyhow!("{e}"))
}

fn parse_tier(raw: &str) -> Result<DataQualityTier> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "unverified" => Ok(DataQualityTier::Unverified),
        "bronze" => Ok(DataQualityTier::Bronze),
        "silver" => Ok(DataQualityTier::Silver),
        "gold" => Ok(DataQualityTier::Gold),
        other => Err(anyhow!(
            "unknown data quality tier '{other}' (expected unverified|bronze|silver|gold)"
        )),
    }
}

/// Batch rows use the same fields as single-registration flags. Unknown fields
/// are rejected so an export/schema mismatch cannot silently change signed data.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationRow {
    station_id: String,
    owner_address: String,
    owner_pubkey: Option<String>,
    owner_signature: Option<String>,
    latitude: f64,
    longitude: f64,
    elevation_meters: Option<f64>,
    #[serde(default = "default_hardware")]
    hardware_model: String,
    #[serde(default = "default_provenance")]
    provenance_class: String,
    #[serde(default = "default_tier")]
    data_quality_tier: String,
    submitted_at: i64,
    /// v2 only: the station's current owner-ledger nonce this registration is
    /// signed for (`ConsensusFeature::StationRegistrationNonce`). Absent = v1.
    #[serde(default)]
    expected_nonce: Option<u64>,
}

fn default_hardware() -> String {
    "reference-station".into()
}
fn default_provenance() -> String {
    "real".into()
}
fn default_tier() -> String {
    "gold".into()
}

struct PreparedRegistration {
    payload: StationRegistrationPayload,
    submitted_at: i64,
    coordinates_wire_adjusted: bool,
}

fn prepare_registration(
    row: &RegistrationRow,
    owner_key: Option<&PathBuf>,
    chain_id: &str,
    network_id: &str,
) -> Result<PreparedRegistration> {
    if row.submitted_at <= 0 || DateTime::from_timestamp(row.submitted_at, 0).is_none() {
        return Err(anyhow!(
            "submitted_at must be a representable positive unix timestamp"
        ));
    }
    validate_station_id(&row.station_id).map_err(|e| anyhow!("station payload is invalid: {e}"))?;
    let class = parse_provenance(&row.provenance_class)?;
    let tier = parse_tier(&row.data_quality_tier)?;
    let mut payload = StationRegistrationPayload::new(
        row.station_id.clone(),
        row.owner_address.clone(),
        row.latitude,
        row.longitude,
        row.hardware_model.clone(),
        tier,
        class,
    );
    payload.elevation_meters = row.elevation_meters;
    if let Some(nonce) = row.expected_nonce {
        // v2: the nonce is bound into the governance preimage, so it is set
        // BEFORE anything is signed.
        payload = payload.with_expected_nonce(nonce);
    }
    // Structural validity before any signing, so a bad coordinate is a
    // clear error here rather than an opaque authorization failure on
    // every peer.
    payload
        .validate()
        .map_err(|e| anyhow!("station payload is invalid: {e}"))?;

    // #3333: sign the value every peer will PARSE, not the value clap
    // parsed. See `wire_stable_f64`.
    let input_bits = (
        payload.latitude.to_bits(),
        payload.longitude.to_bits(),
        payload.elevation_meters.map(f64::to_bits),
    );
    payload.latitude = wire_stable_f64("latitude", payload.latitude)?;
    payload.longitude = wire_stable_f64("longitude", payload.longitude)?;
    payload.elevation_meters = payload
        .elevation_meters
        .map(|e| wire_stable_f64("elevation_meters", e))
        .transpose()?;
    let coordinates_wire_adjusted = input_bits
        != (
            payload.latitude.to_bits(),
            payload.longitude.to_bits(),
            payload.elevation_meters.map(f64::to_bits),
        );
    payload
        .validate()
        .map_err(|e| anyhow!("station payload is invalid: {e}"))?;

    // Owner first: validate the supplied proof before reading/signing with governance.
    let owner_pk = match (
        owner_key,
        row.owner_pubkey.as_deref(),
        row.owner_signature.as_deref(),
    ) {
        (Some(path), None, None) => {
            let owner_sk = read_key(path)?;
            payload.sign_ownership_with_domain(chain_id, network_id, &owner_sk);
            PublicKey::from_private_key(&owner_sk)
        },
        (None, Some(public_hex), Some(signature_hex)) => {
            let pk = PublicKey::from_hex(public_hex)
                .map_err(|_| anyhow!("owner_pubkey must be a valid ed25519 public key in hex"))?;
            let signature = Signature::from_hex(signature_hex)
                .map_err(|_| anyhow!("owner_signature must be a valid ed25519 signature in hex"))?;
            let message = station_ownership_message_with_domain(
                chain_id,
                network_id,
                &row.station_id,
                &row.owner_address,
            );
            if !pk.verify(&message, &signature) {
                return Err(anyhow!(
                    "owner signature does not verify for this chain/network/station/address"
                ));
            }
            payload.owner_pubkey = Some(pk.to_hex());
            payload.owner_signature = Some(signature.to_hex());
            pk
        },
        _ => {
            return Err(anyhow!(
                "supply either --owner-key or both owner_pubkey and owner_signature"
            ));
        },
    };
    // Mirror the exact consensus address predicate (#2742).
    if !payload.owner_address_is_bound_to_owner_pubkey() {
        let derived = format!("0x{}", owner_pk.to_address().trim_start_matches("wrf")); // secret-leak-ok: only a public address derived from the public owner key.
        return Err(anyhow!(
            "owner_address field is not derived from the owner public key (#2742): {}. \
             Use owner_address {derived} — SHA3-256 of the raw ed25519 public key, \
             first 20 bytes (PublicKey::to_address); CLI wallet-file 'Wrf…' names \
             use SHA-256 and are not on-chain addresses.",
            row.owner_address
        ));
    }
    Ok(PreparedRegistration {
        payload,
        submitted_at: row.submitted_at,
        coordinates_wire_adjusted,
    })
}

fn prepare_batch(
    path: &PathBuf,
    chain_id: &str,
    network_id: &str,
) -> Result<Vec<PreparedRegistration>> {
    let mut prepared_rows = Vec::new();
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read batch {}", path.display()))?;
    // Parse rows separately so a malformed row does not hide later failures.
    let rows: Vec<Value> =
        serde_json::from_str(&raw).map_err(|e| anyhow!("--batch must be a JSON array: {e}"))?;
    if rows.is_empty() {
        return Err(anyhow!("--batch must contain at least one row"));
    }
    let mut failures = Vec::new();
    let mut ids = rustc_hash::FxHashSet::default();
    for (index, value) in rows.into_iter().enumerate() {
        let result = serde_json::from_value::<RegistrationRow>(value)
            .map_err(anyhow::Error::from)
            .and_then(|row| {
                if !ids.insert(row.station_id.clone()) {
                    return Err(anyhow!("duplicate station_id in batch"));
                }
                prepare_registration(&row, None, chain_id, network_id)
            });
        match result {
            Ok(prepared) => prepared_rows.push(prepared),
            Err(e) => failures.push(format!("row {}: {e}", index + 1)),
        }
    }
    if !failures.is_empty() {
        return Err(anyhow!(
            "batch rejected; nothing written:\n{}",
            failures.join("\n")
        ));
    }
    Ok(prepared_rows)
}

/// Publish a complete artifact by same-directory rename. A failed write or
/// sync leaves the previous artifact intact; a crash before rename can leave
/// an unused temp file, but never truncates the destination.
pub(crate) fn atomic_artifact_write(
    out: &Path,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> Result<()> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".wrfcoin-station-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    struct TemporaryArtifact(PathBuf);
    impl Drop for TemporaryArtifact {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    // Arm cleanup only after create_new succeeded, so an improbable collision
    // can never remove an existing file.
    let _cleanup = TemporaryArtifact(temporary.clone());
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, out)?;
    Ok(())
}

pub async fn handle_station_command(action: &StationCommands) -> Result<Value> {
    match action {
        StationCommands::Owner(command) => {
            super::station_owner::handle_station_owner_command(command).await
        },
        StationCommands::BuildRegistration {
            station_id,
            owner_address,
            owner_key,
            owner_pubkey,
            owner_signature,
            batch,
            governance_key,
            chain_id,
            network_id,
            latitude,
            longitude,
            elevation_meters,
            hardware_model,
            provenance_class,
            data_quality_tier,
            submitted_at,
            expected_nonce,
            out,
        } => {
            let prepared_rows = if let Some(path) = batch {
                prepare_batch(path, chain_id, network_id)?
            } else {
                let row = RegistrationRow {
                    station_id: station_id
                        .clone()
                        .ok_or_else(|| anyhow!("--station-id required"))?,
                    owner_address: owner_address
                        .clone()
                        .ok_or_else(|| anyhow!("--owner-address required"))?,
                    owner_pubkey: owner_pubkey.clone(),
                    owner_signature: owner_signature.clone(),
                    latitude: latitude.ok_or_else(|| anyhow!("--latitude required"))?,
                    longitude: longitude.ok_or_else(|| anyhow!("--longitude required"))?,
                    elevation_meters: *elevation_meters,
                    hardware_model: hardware_model.clone(),
                    provenance_class: provenance_class.clone(),
                    data_quality_tier: data_quality_tier.clone(),
                    submitted_at: submitted_at.ok_or_else(|| anyhow!("--submitted-at required"))?,
                    expected_nonce: *expected_nonce,
                };
                vec![prepare_registration(
                    &row,
                    owner_key.as_ref(),
                    chain_id,
                    network_id,
                )?]
            };
            // No governance key is read and no artifact is touched until every row passes.
            let gov_sk = read_key(governance_key)?;
            let mut transactions = Vec::new();
            let mut summaries = Vec::new();
            for prepared in prepared_rows {
                let payload = &prepared.payload;
                let mut summary = json!({
                    "station_id": payload.station_id,
                    "owner_address": payload.owner_address,
                    "provenance_class": payload.provenance_class.as_str(),
                    "coordinates_wire_adjusted": prepared.coordinates_wire_adjusted,
                });
                let mut tx = Transaction::new_station_registration_with_domain(
                    chain_id,
                    network_id,
                    prepared.payload,
                    &gov_sk,
                );
                // `Transaction::new` stamps `timestamp` from the wall clock, which
                // would make the artifact irreproducible and the txid unstable.
                // Pin it to the declared `submitted_at` and re-derive the id.
                tx.timestamp =
                    DateTime::from_timestamp(prepared.submitted_at, 0).ok_or_else(|| {
                        anyhow!("submitted_at is outside the supported timestamp range")
                    })?;
                tx.set_id();

                // #2744: verify against the SAME chain identity the signatures
                // folded in. Before this, signing used the flags while
                // self-verification resolved WRFCOIN_CHAIN_ID/WRFCOIN_NETWORK_ID
                // from the environment — two sources of truth for one value — so
                // correct flags with an unset env failed here, and the error
                // blamed the governance key and hinted at the API server.
                if !tx.verify_station_registration_authorization_with_domain(chain_id, network_id) {
                    // With the domain unified, a chain-identity mismatch can no
                    // longer cause this. Name the check that actually failed
                    // instead of guessing (#2744, same hazard family as #2713).
                    if !is_authorized_oracle_governance(
                        &PublicKey::from_private_key(&gov_sk).to_hex(),
                    ) {
                        return Err(anyhow!(
                            "self-verification failed: the governance key is not in the \
                         WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS allowlist. Export the allowlist \
                         containing this key's PUBLIC hex and rebuild. (This is an offline \
                         check — no API server is involved.)"
                        ));
                    }
                    return Err(CliError::new(
                        CliErrorCode::InternalError,
                        INTERNAL_SELF_VERIFY_DEFECT_MSG,
                    )
                    .into());
                }

                summary["transaction_id"] = json!(tx.id.map(|id| id.to_hex()));
                summaries.push(summary);
                transactions.push(tx);
            }
            // The flags are the single source of truth for the signed domain.
            // If the process environment ALSO declares a chain identity and it
            // disagrees, the build is still correct — but a node booted from
            // this environment would reject the artifact, so name both values
            // instead of letting the mismatch surface later as an opaque
            // authorization failure (#2744 deliverable: say *domain mismatch*
            // and show the two values that differed).
            let env_chain = signing_chain_id();
            let env_network = signing_network_id();
            let env_domain_mismatch = (std::env::var_os(CHAIN_ID_ENV).is_some()
                || std::env::var_os(NETWORK_ID_ENV).is_some())
                && (env_chain != *chain_id || env_network != *network_id);
            let domain_warning = env_domain_mismatch.then(|| {
                format!(
                    "domain mismatch between flags and environment: signed with \
                     --chain-id {chain_id} --network-id {network_id}, but this process \
                     environment resolves {env_chain}/{env_network}. The flags win — the \
                     artifact is bound to {chain_id}/{network_id} — and a node running \
                     under this environment's identity would reject it."
                )
            });
            if let Some(warning) = &domain_warning {
                eprintln!("warning: {warning}");
            }

            let artifact = if batch.is_some() {
                json!({
                    "version": STATION_ARTIFACT_VERSION, "chain_id": chain_id,
                    "network_id": network_id, "transactions": transactions,
                })
            } else {
                json!({
                    "version": STATION_ARTIFACT_VERSION, "chain_id": chain_id,
                    "network_id": network_id, "submitted_at": submitted_at,
                    "transactions": transactions,
                })
            };
            let rendered = serde_json::to_string_pretty(&artifact)? + "\n";
            if rendered.len() > MAX_STATION_ARTIFACT_BYTES {
                return Err(anyhow!(
                    "station artifact is {} bytes; the launch API ceiling is {} bytes. \
                     Split --batch into smaller JSON arrays and rebuild; nothing written.",
                    rendered.len(),
                    MAX_STATION_ARTIFACT_BYTES
                ));
            }
            atomic_artifact_write(out, |file| file.write_all(rendered.as_bytes()))
                .with_context(|| format!("cannot write artifact {}", out.display()))?;
            if batch.is_some() {
                Ok(json!({
                    "artifact": out.display().to_string(), "registrations": summaries,
                    "self_verified": true, "domain_warning": domain_warning,
                }))
            } else {
                let mut summary = summaries.remove(0);
                summary["artifact"] = json!(out.display().to_string());
                summary["self_verified"] = json!(true);
                summary["domain_warning"] = json!(domain_warning);
                Ok(summary)
            }
        },

        StationCommands::BuildObservation {
            station_id,
            contributor,
            signer_key,
            chain_id,
            network_id,
            nonce,
            timestamp,
            data,
            compute_receipt_key,
            work_factor,
            out,
        } => {
            let signer = read_key(signer_key)?;
            let payload: Value = serde_json::from_str(data)
                .map_err(|e| anyhow!("--data must be a JSON object: {e}"))?;
            if !payload.is_object() {
                return Err(anyhow!("--data must be a JSON object"));
            }

            // Derived once, by the protocol's own function, and reused by both
            // bodies below.
            let commit_id = Transaction::compute_commit_id(
                &payload,
                nonce,
                station_id,
                *timestamp,
                "contribution",
            );

            let auth_message = commit_authorization_message_with_domain(
                chain_id,
                network_id,
                station_id,
                &commit_id,
                "contribution",
            );
            let auth_signature = signer.sign(&auth_message).to_hex();
            let auth_pubkey = PublicKey::from_private_key(&signer).to_hex();

            let commit_body = json!({
                "contributor": contributor,
                "station_id": station_id,
                "commit_hash": commit_id,
                "auth_pubkey": auth_pubkey,
                "auth_signature": auth_signature,
            });
            let mut reveal_body = json!({
                "contributor": contributor,
                "station_id": station_id,
                "payload": payload,
                "nonce": nonce,
                "commit_hash": commit_id,
                "timestamp": timestamp,
            });

            // Optional signed ComputeReceipt. `worker_id` IS the verifying
            // public key — `has_valid_signature` reads the key out of that
            // field — so the worker key and the identity are the same thing.
            if let Some(worker_key_path) = compute_receipt_key {
                let worker_sk = read_key(worker_key_path)?;
                let worker_id = PublicKey::from_private_key(&worker_sk).to_hex();
                let mut receipt = ComputeReceipt {
                    task_id: format!("task-{commit_id}"),
                    worker_id,
                    work_hash: blake3_hex(commit_id.as_bytes()),
                    work_factor: *work_factor,
                    completed_at: *timestamp,
                    nonce: format!("cr-{nonce}"),
                    signature: None,
                };
                let msg = receipt.signing_message();
                receipt.signature = Some(worker_sk.sign(msg.as_bytes()).to_hex());
                if !receipt.has_valid_signature() {
                    return Err(anyhow!(
                        "self-verification failed: the compute receipt does not verify.                          The signing message folds the chain identity from the process                          environment, so WRFCOIN_CHAIN_ID / WRFCOIN_NETWORK_ID must match                          the node."
                    ));
                }
                reveal_body["compute_receipt"] = serde_json::to_value(&receipt)?;
            }

            let artifact = json!({
                "version": "station_observation_v1",
                "commit_id": commit_id,
                "commit_body": commit_body,
                "reveal_body": reveal_body,
            });
            fs::write(out, serde_json::to_string_pretty(&artifact)? + "\n")
                .with_context(|| format!("cannot write {}", out.display()))?;

            Ok(json!({
                "artifact": out.display().to_string(),
                "commit_id": commit_id,
                "station_id": station_id,
                "contributor": contributor,
            }))
        },

        StationCommands::BuildAttestation {
            commit_id,
            oracle_id,
            oracle_key,
            timestamp,
            nonce,
            out,
        } => {
            let sk = read_key(oracle_key)?;
            let mut payload = OracleAttestationPayload {
                commit_id: commit_id.clone(),
                oracle_id: oracle_id.clone(),
                approved: true,
                signature: None,
                confidence: Some(1.0),
                timestamp: *timestamp,
                nonce: Some(nonce.clone()),
            };
            // The domain-separated attestation recipe — NOT the commit-id
            // preimage.
            let signature = sk.sign(payload.signing_message().as_bytes()).to_string();
            payload.signature = Some(signature.clone());

            let body = json!({
                "oracle_id": oracle_id,
                "commit_hash": commit_id,
                "approved": true,
                "signature": signature,
                "confidence": 1.0,
                "timestamp": timestamp,
                "nonce": nonce,
            });
            fs::write(out, serde_json::to_string_pretty(&body)? + "\n")
                .with_context(|| format!("cannot write {}", out.display()))?;

            Ok(json!({
                "artifact": out.display().to_string(),
                "commit_id": commit_id,
                "oracle_id": oracle_id,
            }))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::INTERNAL_SELF_VERIFY_DEFECT_MSG;
    use wrfcoin_cli::errors::{CliError, CliErrorCode, INTERNAL_ERROR_SENTINEL};

    #[test]
    fn atomic_artifact_keeps_old_output_after_partial_write_failure() {
        use super::atomic_artifact_write;
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("station-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let out = dir.join("artifact.json");
        std::fs::write(&out, "previous complete artifact").unwrap();
        let failure = atomic_artifact_write(&out, |file| {
            file.write_all(b"truncated")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(failure.is_err());
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "previous complete artifact"
        );
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "temporary file cleaned"
        );
        atomic_artifact_write(&out, |file| file.write_all(b"new complete artifact")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "new complete artifact"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #2880 review regression: the REAL internal-defect message must emit
    /// `INTERNAL_ERROR` (exit 9) with the internal-defect hint. The exact
    /// production constant is carried by the typed error and survives context
    /// wrapping; its prose is not consulted.
    #[test]
    fn internal_defect_message_classifies_as_internal_error() {
        assert!(
            INTERNAL_SELF_VERIFY_DEFECT_MSG.contains(INTERNAL_ERROR_SENTINEL),
            "the message must carry the explicit sentinel — that is the \
             stable channel, not its prose"
        );
        // The trap this guards: the prose still legitimately contains the
        // word "configuration", which is exactly what misrouted it before.
        assert!(INTERNAL_SELF_VERIFY_DEFECT_MSG.contains("configuration"));

        let error: anyhow::Error =
            CliError::new(CliErrorCode::InternalError, INTERNAL_SELF_VERIFY_DEFECT_MSG).into();
        let code = CliErrorCode::from_error(&error);
        assert_eq!(code, CliErrorCode::InternalError);
        assert_eq!(code.code_str(), "INTERNAL_ERROR");
        assert_eq!(code.exit_code(), 9);
        assert!(
            code.hint().contains("defect in the CLI itself"),
            "hint must own the defect, got: {}",
            code.hint()
        );
        assert!(
            !code.hint().contains("setup init"),
            "hint must never send an operator to repair configuration for a \
             CLI defect"
        );
        // And context-wrapping must not change the classification.
        let wrapped = error.context("station build failed");
        assert_eq!(
            CliErrorCode::from_error(&wrapped),
            CliErrorCode::InternalError
        );
    }

    /// #3333: `build-registration` failed its own self-verification for some
    /// full-precision coordinates. The first pair below is the live repro from
    /// the 2026-09-25 re-registration; the second is a control that always
    /// passed.
    ///
    /// Mechanism: clap parses `--latitude` with Rust's correctly rounded
    /// `str::parse::<f64>`, and those bits were signed. Every verifier (this
    /// CLI's self-check, node admission, producer, P2P applier, WAL replay)
    /// re-parses `tx.data` with `serde_json`, whose default float parser is not
    /// correctly rounded, so some values come back one ulp off. The preimage
    /// encodes the raw bits, so the governance signature then fails.
    ///
    /// This drives the real command end to end, then submits the artifact's
    /// bytes to the exact predicate `POST /transactions/station/registrations`
    /// gates on (`verify_station_registration_authorization_at_height`, with
    /// the chain identity from the node's environment and a post-#2742 height).
    #[test]
    fn full_precision_coordinates_build_self_verify_and_admit_3333() {
        use super::{StationCommands, handle_station_command};
        use wrfcoin_shared_protocol::crypto::domain_signing::{CHAIN_ID_ENV, NETWORK_ID_ENV};
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

        const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
        const CHAIN: &str = "wrfcoin-testnet-3333";
        const NETWORK: &str = "testnet";

        // Keep the process-domain guard in synchronous code. The command runs
        // on this test's private runtime, so no MutexGuard crosses an await.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        let _domain = crate::crypto::SIGNING_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let dir = std::env::temp_dir().join(format!("wrfcoin-cli-3333-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let owner_sk = PrivateKey::from_bytes(&[0x33; 32]).expect("owner key");
        let gov_sk = PrivateKey::from_bytes(&[0x44; 32]).expect("governance key");
        let owner_key = dir.join("owner.key");
        let governance_key = dir.join("governance.key");
        std::fs::write(&owner_key, owner_sk.to_hex()).expect("write owner key");
        std::fs::write(&governance_key, gov_sk.to_hex()).expect("write governance key");
        let owner_address = format!(
            "0x{}",
            PublicKey::from_private_key(&owner_sk)
                .to_address()
                .trim_start_matches("wrf")
        ); // secret-leak-ok: formats only the public address derived from a throwaway test key's public key; no private-key bytes reach this value.

        // Restore even when an assertion or artifact read panics, before releasing
        // the signing-domain lock shared with snapshot readers.
        struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                for (k, v) in &self.0 {
                    match v {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
        let saved: Vec<(&str, Option<std::ffi::OsString>)> =
            [GOVERNANCE_ENV, CHAIN_ID_ENV, NETWORK_ID_ENV]
                .into_iter()
                .map(|k| (k, std::env::var_os(k)))
                .collect();
        let _restore = RestoreEnv(saved);
        std::env::set_var(
            GOVERNANCE_ENV,
            PublicKey::from_private_key(&gov_sk).to_hex(),
        );
        std::env::set_var(CHAIN_ID_ENV, CHAIN);
        std::env::set_var(NETWORK_ID_ENV, NETWORK);

        let cases = [
            ("failing", 46.83197584106581_f64, -92.04194742607375_f64),
            ("control", 46.837746027514605_f64, -92.0278590954643_f64),
        ];
        let mut failures = Vec::new();
        for (label, latitude, longitude) in cases {
            let out = dir.join(format!("{label}.json"));
            let result = runtime.block_on(handle_station_command(
                &StationCommands::BuildRegistration {
                    station_id: Some(format!("PWS-3333-{label}")),
                    owner_address: Some(owner_address.clone()),
                    owner_key: Some(owner_key.clone()),
                    owner_pubkey: None,
                    owner_signature: None,
                    batch: None,
                    governance_key: governance_key.clone(),
                    chain_id: CHAIN.to_string(),
                    network_id: NETWORK.to_string(),
                    latitude: Some(latitude),
                    longitude: Some(longitude),
                    elevation_meters: Some(242.5),
                    hardware_model: "reference-station".to_string(),
                    provenance_class: "real".to_string(),
                    data_quality_tier: "gold".to_string(),
                    submitted_at: Some(1_790_000_000),
                    expected_nonce: None,
                    out: out.clone(),
                },
            ));
            match result {
                Err(e) => failures.push(format!("{label}: build failed: {e:#}")),
                Ok(summary) => {
                    assert_eq!(summary["self_verified"], true);
                    failures.extend(check_artifact(label, &out, latitude, longitude));
                },
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
        assert!(failures.is_empty(), "#3333 regressions: {failures:#?}");
    }

    /// Node-side checks on a written `build-registration` artifact (#3333).
    fn check_artifact(
        label: &str,
        out: &std::path::Path,
        latitude: f64,
        longitude: f64,
    ) -> Vec<String> {
        use wrfcoin_shared_protocol::blockchain::transaction::Transaction;

        let mut failures = Vec::new();
        let artifact: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out).expect("artifact"))
                .expect("artifact json");
        let tx: Transaction =
            serde_json::from_value(artifact["transactions"][0].clone()).expect("tx");
        // Node admission predicate, on the artifact's bytes.
        if !tx.verify_station_registration_authorization_at_height(u64::MAX) {
            failures.push(format!("{label}: node admission predicate rejects it"));
        }
        if tx.id != Some(tx.calculate_hash()) {
            failures.push(format!("{label}: txid does not recompute"));
        }
        let parsed = tx.parse_station_registration().expect("parses");
        // The coordinate the chain records is the operator's input to within
        // one ulp (sub-nanometre), never a coarser change.
        for (name, input, stored) in [
            ("latitude", latitude, parsed.latitude),
            ("longitude", longitude, parsed.longitude),
        ] {
            let ulps = input.to_bits().abs_diff(stored.to_bits());
            if ulps > 1 {
                failures.push(format!(
                    "{label}: {name} moved {ulps} ulps ({input} -> {stored})"
                ));
            }
        }
        failures
    }
}
