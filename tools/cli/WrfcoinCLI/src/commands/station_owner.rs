//! Station owner-action artifact builders (#3345 / #3347, epic infra#1095).
//!
//! Offline, like `station build-registration`: each command reads a key file,
//! signs the exact preimage core4 verifies (`shared-protocol/src/blockchain/
//! station_owner.rs`, spec `docs/plans/overnight-2026-09-24/r23/
//! station-owner-actions-spec.md`), self-verifies the stateless half, and
//! writes an artifact whose `transactions` array is the body of
//! `POST /transactions/station/owner-actions`. No node contact, no wall-clock
//! read: `--submitted-at` is pinned so the artifact and its txid are
//! reproducible.
//!
//! Two-party flows are split so a private key never travels:
//! - `transfer` (current owner) writes a PROPOSAL; `accept` (new owner)
//!   countersigns it and writes the submit artifact.
//! - `recover-file` is a GOVERNANCE / operator command (an allowlisted
//!   oracle-governance key signs the filing). The new owner's acceptance comes
//!   from `--new-owner-key`, or from a signature produced elsewhere with
//!   `recover-accept` and passed as `--to-pubkey` / `--to-owner-signature`.
//!
//! The ledger nonce each step must carry is read from
//! `GET /transactions/station/registrations/{id}/owner-ledger`; the CLI never
//! guesses it. `--expires-at-height` is a block height (use tip + ~5000; the
//! node refuses anything over tip + 10000).

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use wrfcoin_shared_protocol::blockchain::station_owner::{
    StationOwnerActionPayload, StationOwnerRecoveryCancelPayload,
    StationOwnerRecoveryExecutePayload, StationOwnerRecoveryFilingPayload,
    StationOwnerTransferPayload,
};
use wrfcoin_shared_protocol::blockchain::transaction::{StationOwnerActionKind, Transaction};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

use super::station::{atomic_artifact_write, read_key};
use super::station_owner_exchange::{
    RecoveryBody, decode_recovery_accept, encode_recovery_accept, encode_recovery_request,
};

pub const OWNER_ARTIFACT_VERSION: &str = "station_owner_artifact_v1";
pub const TRANSFER_PROPOSAL_VERSION: &str = "station_owner_transfer_proposal_v1";

/// Arguments every owner-signed step shares.
#[derive(Args, Debug)]
pub struct ChainArgs {
    /// Chain id the signature binds. Required, never inferred: use the value the
    /// node reports. A wrong one produces a tx that fails on every peer.
    #[clap(long)]
    pub chain_id: String,
    /// Network id.
    #[clap(long, default_value = "testnet")]
    pub network_id: String,
}

#[derive(Args, Debug)]
pub struct ActionArgs {
    #[clap(long)]
    pub station_id: String,
    /// The registered owner's ed25519 private key file.
    #[clap(long)]
    pub owner_key: PathBuf,
    /// The station's current `owner_nonce` (from the node's owner-ledger route).
    #[clap(long)]
    pub nonce: u64,
    /// Last block height at which the action may be applied (tip + ~5000).
    #[clap(long)]
    pub expires_at_height: u64,
    #[clap(flatten)]
    pub chain: ChainArgs,
    /// Transaction timestamp (unix seconds), pinned for reproducibility. The
    /// node refuses a tx more than 150 s from its clock.
    #[clap(long)]
    pub submitted_at: i64,
    /// Output artifact path.
    #[clap(long)]
    pub out: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum StationOwnerCommands {
    /// CURRENT owner: sign a station transfer and write a PROPOSAL for the new
    /// owner to countersign with `accept`.
    Transfer {
        #[clap(long)]
        station_id: String,
        /// Current owner's private key file.
        #[clap(long)]
        owner_key: PathBuf,
        /// New owner's public ed25519 key (hex); their address is derived.
        #[clap(long)]
        to_pubkey: String,
        /// The station's current `owner_nonce`.
        #[clap(long)]
        nonce: u64,
        #[clap(long)]
        expires_at_height: u64,
        #[clap(flatten)]
        chain: ChainArgs,
        /// Output proposal path.
        #[clap(long)]
        out: PathBuf,
    },
    /// NEW owner: countersign a transfer proposal and write the submit artifact.
    Accept {
        /// Proposal written by `transfer`.
        #[clap(long)]
        proposal: PathBuf,
        /// New owner's private key file.
        #[clap(long)]
        new_owner_key: PathBuf,
        #[clap(long)]
        submitted_at: i64,
        #[clap(long)]
        out: PathBuf,
    },
    /// Owner: report the station stolen or lost (freezes it; rewards stop).
    ReportStolen(ActionArgs),
    /// Owner: lift a report-stolen freeze.
    Unfreeze(ActionArgs),
    /// Owner: self-deactivate the station.
    Deactivate(ActionArgs),
    /// Owner: reactivate a self-deactivated (not frozen) station.
    Reactivate(ActionArgs),
    /// GOVERNANCE / OPERATOR ONLY: file a time-locked owner-key recovery. Signed
    /// by an allowlisted oracle-governance key (its public hex must be in
    /// `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`) plus the new owner's acceptance.
    RecoverFile {
        #[clap(long)]
        station_id: String,
        /// The owner registered NOW (pinned into the signed preimage, as written).
        #[clap(long)]
        from_owner_address: String,
        #[clap(long)]
        governance_key: PathBuf,
        /// New owner's private key file: signs the acceptance here. Conflicts
        /// with --to-pubkey / --to-owner-signature.
        #[clap(long, conflicts_with_all = ["to_pubkey", "to_owner_signature", "accept_text"],
            required_unless_present_any = ["to_pubkey", "accept_text"])]
        new_owner_key: Option<PathBuf>,
        /// The claimant's reply from their phone: `WRFOWN1.recovery-accept.…`
        /// text, or `@path` to read it from a file. Its fields must match this
        /// command's station, owner, nonce and chain.
        #[clap(long, conflicts_with_all = ["to_pubkey", "to_owner_signature"])]
        accept_text: Option<String>,
        /// New owner's public key (hex); needs --to-owner-signature.
        #[clap(long, requires = "to_owner_signature", conflicts_with = "accept_text")]
        to_pubkey: Option<String>,
        /// Acceptance signature (hex) from `recover-accept`.
        #[clap(long, requires = "to_pubkey")]
        to_owner_signature: Option<String>,
        /// The station's current `owner_nonce`.
        #[clap(long)]
        nonce: u64,
        #[clap(flatten)]
        chain: ChainArgs,
        #[clap(long)]
        submitted_at: i64,
        #[clap(long)]
        out: PathBuf,
    },
    /// GOVERNANCE / OPERATOR: print the recovery REQUEST text
    /// (`WRFOWN1.recovery.…`) for the claimant to open in their phone, which
    /// countersigns it and returns `WRFOWN1.recovery-accept.…`. Public values
    /// only; writes nothing.
    RecoverRequest {
        #[clap(long)]
        station_id: String,
        /// The owner registered NOW (as written, pinned into the preimage).
        #[clap(long)]
        from_owner_address: String,
        /// The claimant's public key (hex); their address is derived.
        #[clap(long)]
        to_pubkey: String,
        /// The station's current `owner_nonce`.
        #[clap(long)]
        nonce: u64,
        #[clap(flatten)]
        chain: ChainArgs,
    },
    /// NEW owner: produce the acceptance signature (hex) for a recovery filing,
    /// to hand to the operator running `recover-file`. Prints it; writes nothing.
    RecoverAccept {
        #[clap(long)]
        station_id: String,
        #[clap(long)]
        from_owner_address: String,
        #[clap(long)]
        new_owner_key: PathBuf,
        #[clap(long)]
        nonce: u64,
        #[clap(flatten)]
        chain: ChainArgs,
    },
    /// CURRENT owner: cancel a pending recovery before its notice window ends.
    RecoverCancel(ActionArgs),
    /// Anyone: execute a pending recovery once its notice window has elapsed.
    /// Unsigned.
    RecoverExecute {
        #[clap(long)]
        station_id: String,
        /// The pending recovery's `recovery_nonce`.
        #[clap(long)]
        nonce: u64,
        /// Unsigned and chain-agnostic, but the submit route still requires the
        /// node's identity in the request body.
        #[clap(flatten)]
        chain: ChainArgs,
        #[clap(long)]
        submitted_at: i64,
        #[clap(long)]
        out: PathBuf,
    },
}

fn address_of(key: &PrivateKey) -> String {
    PublicKey::from_private_key(key).to_address()
}

fn write_artifact(out: &PathBuf, artifact: &Value) -> Result<()> {
    let rendered = serde_json::to_string_pretty(artifact)? + "\n";
    atomic_artifact_write(out, |file| file.write_all(rendered.as_bytes()))
        .with_context(|| format!("cannot write artifact {}", out.display()))
}

fn envelope(data_json: String, submitted_at: i64) -> Result<Transaction> {
    let tx = Transaction::canonical_station_owner_tx(&data_json, submitted_at)
        .ok_or_else(|| anyhow!("submitted_at is outside the supported timestamp range"))?;
    if tx.id != Some(tx.calculate_hash()) {
        return Err(anyhow!("internal: owner tx id does not recompute"));
    }
    Ok(tx)
}

fn artifact_of(chain: &ChainArgs, submitted_at: i64, tx: &Transaction) -> Value {
    json!({
        "version": OWNER_ARTIFACT_VERSION,
        "chain_id": chain.chain_id,
        "network_id": chain.network_id,
        "submitted_at": submitted_at,
        "transactions": [tx],
    })
}

fn summary(kind: &str, station_id: &str, nonce: u64, tx: &Transaction, out: &PathBuf) -> Value {
    json!({
        "kind": kind,
        "station_id": station_id,
        "nonce": nonce,
        "transaction_id": tx.id.map(|id| id.to_hex()),
        "artifact": out.display().to_string(),
        "self_verified": true,
    })
}

fn build_action(kind: StationOwnerActionKind, a: &ActionArgs) -> Result<Value> {
    let key = read_key(&a.owner_key)?;
    let mut payload = StationOwnerActionPayload::new(
        &a.station_id,
        address_of(&key),
        PublicKey::from_private_key(&key).to_hex(),
        kind,
        a.nonce,
        a.expires_at_height,
    );
    payload.sign_with_domain(&a.chain.chain_id, &a.chain.network_id, &key);
    payload
        .verify_stateless_with_domain(&a.chain.chain_id, &a.chain.network_id, a.expires_at_height)
        .map_err(|e| anyhow!("self-verification failed: {e:?}"))?;
    let tx = envelope(serde_json::to_string(&payload)?, a.submitted_at)?;
    write_artifact(&a.out, &artifact_of(&a.chain, a.submitted_at, &tx))?;
    Ok(summary(
        StationOwnerActionPayload::TX_KIND,
        &a.station_id,
        a.nonce,
        &tx,
        &a.out,
    ))
}

pub async fn handle_station_owner_command(action: &StationOwnerCommands) -> Result<Value> {
    use StationOwnerCommands as C;
    match action {
        C::Transfer {
            station_id,
            owner_key,
            to_pubkey,
            nonce,
            expires_at_height,
            chain,
            out,
        } => {
            let key = read_key(owner_key)?;
            let to_pk = PublicKey::from_hex(to_pubkey)
                .map_err(|e| anyhow!("--to-pubkey is not valid ed25519 hex: {e}"))?;
            let mut payload = StationOwnerTransferPayload::new(
                station_id,
                address_of(&key),
                PublicKey::from_private_key(&key).to_hex(),
                to_pk.to_address(),
                to_pk.to_hex(),
                *nonce,
                *expires_at_height,
            );
            payload.sign_as_owner_with_domain(&chain.chain_id, &chain.network_id, &key);
            let proposal = json!({
                "version": TRANSFER_PROPOSAL_VERSION,
                "chain_id": chain.chain_id,
                "network_id": chain.network_id,
                "payload": payload,
            });
            write_artifact(out, &proposal)?;
            Ok(json!({
                "kind": StationOwnerTransferPayload::TX_KIND,
                "station_id": station_id,
                "nonce": nonce,
                "from_owner_address": payload.from_owner_address,
                "to_owner_address": payload.to_owner_address,
                "expires_at_height": expires_at_height,
                "proposal": out.display().to_string(),
                "next": "the new owner runs `station accept` on this proposal",
            }))
        },
        C::Accept {
            proposal,
            new_owner_key,
            submitted_at,
            out,
        } => {
            let raw = fs::read_to_string(proposal)
                .with_context(|| format!("cannot read proposal {}", proposal.display()))?;
            let doc: Value = serde_json::from_str(&raw).context("proposal is not JSON")?;
            if doc["version"] != TRANSFER_PROPOSAL_VERSION {
                return Err(anyhow!(
                    "not a {TRANSFER_PROPOSAL_VERSION} document (version {})",
                    doc["version"]
                ));
            }
            let chain = ChainArgs {
                chain_id: doc["chain_id"].as_str().context("proposal chain_id")?.to_string(),
                network_id: doc["network_id"].as_str().context("proposal network_id")?.to_string(),
            };
            let mut payload: StationOwnerTransferPayload =
                serde_json::from_value(doc["payload"].clone()).context("proposal payload")?;
            let key = read_key(new_owner_key)?;
            if PublicKey::from_private_key(&key).to_hex() != payload.to_owner_pubkey {
                return Err(anyhow!(
                    "--new-owner-key is not the key this proposal transfers to \
                     (to_owner_pubkey mismatch); refusing to sign"
                ));
            }
            payload.countersign_with_domain(&chain.chain_id, &chain.network_id, &key);
            payload
                .verify_stateless_with_domain(
                    &chain.chain_id,
                    &chain.network_id,
                    payload.expires_at_height,
                )
                .map_err(|e| anyhow!("self-verification failed: {e:?}"))?;
            let tx = envelope(serde_json::to_string(&payload)?, *submitted_at)?;
            write_artifact(out, &artifact_of(&chain, *submitted_at, &tx))?;
            Ok(summary(
                StationOwnerTransferPayload::TX_KIND,
                &payload.station_id,
                payload.transfer_nonce,
                &tx,
                out,
            ))
        },
        C::ReportStolen(a) => build_action(StationOwnerActionKind::ReportStolen, a),
        C::Unfreeze(a) => build_action(StationOwnerActionKind::Unfreeze, a),
        C::Deactivate(a) => build_action(StationOwnerActionKind::Deactivate, a),
        C::Reactivate(a) => build_action(StationOwnerActionKind::Reactivate, a),
        C::RecoverFile {
            station_id,
            from_owner_address,
            governance_key,
            new_owner_key,
            accept_text,
            to_pubkey,
            to_owner_signature,
            nonce,
            chain,
            submitted_at,
            out,
        } => {
            let gov = read_key(governance_key)?;
            // The claimant's phone reply: check it is about THIS recovery, then
            // take the claimant's identity and signature from it.
            let from_phone = match accept_text {
                Some(raw) => {
                    let text = match raw.strip_prefix('@') {
                        Some(path) => fs::read_to_string(path)
                            .with_context(|| format!("cannot read accept text {path}"))?,
                        None => raw.clone(),
                    };
                    let (body, signature) = decode_recovery_accept(&text)
                        .map_err(|e| anyhow!("--accept-text refused: {e}"))?;
                    let want = (
                        &chain.chain_id,
                        &chain.network_id,
                        station_id,
                        from_owner_address,
                        *nonce,
                    );
                    let got = (
                        &body.chain_id,
                        &body.network_id,
                        &body.station_id,
                        &body.from_owner_address,
                        body.recovery_nonce,
                    );
                    if want != got {
                        return Err(anyhow!(
                            "--accept-text is for a different recovery (chain, network, station, \
                             from_owner_address or nonce differ from this command); refusing"
                        ));
                    }
                    Some((body, signature))
                },
                None => None,
            };
            let (to_pk_hex, to_addr) = match (new_owner_key, from_phone.as_ref().map(|(b, _)| b.to_owner_pubkey.clone()).or_else(|| to_pubkey.clone())) {
                (Some(path), _) => {
                    let k = read_key(path)?;
                    (PublicKey::from_private_key(&k).to_hex(), address_of(&k))
                },
                (None, Some(hex)) => {
                    let pk = PublicKey::from_hex(&hex)
                        .map_err(|e| anyhow!("--to-pubkey is not valid ed25519 hex: {e}"))?;
                    (pk.to_hex(), pk.to_address())
                },
                (None, None) => unreachable!("clap requires one of the two"),
            };
            let mut payload = StationOwnerRecoveryFilingPayload::new(
                station_id,
                from_owner_address,
                to_addr,
                to_pk_hex,
                *nonce,
            );
            payload.sign_as_governance_with_domain(&chain.chain_id, &chain.network_id, &gov);
            match (new_owner_key, from_phone.as_ref(), to_owner_signature) {
                (Some(path), _, _) => {
                    let k = read_key(path)?;
                    payload.accept_with_domain(&chain.chain_id, &chain.network_id, &k);
                },
                (None, Some((_, sig)), _) => payload.to_owner_signature = Some(sig.clone()),
                (None, None, Some(sig)) => payload.to_owner_signature = Some(sig.clone()),
                (None, None, None) => unreachable!("clap requires one of the three"),
            }
            payload
                .verify_stateless_with_domain(&chain.chain_id, &chain.network_id)
                .map_err(|e| {
                    anyhow!(
                        "self-verification failed: {e:?}. A NotGovernance / \
                         MissingOrBadGovernanceSignature here usually means the governance \
                         key's PUBLIC hex is not in WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS; \
                         MissingOrBadAcceptance means the acceptance signature does not match \
                         these exact fields"
                    )
                })?;
            let tx = envelope(serde_json::to_string(&payload)?, *submitted_at)?;
            write_artifact(out, &artifact_of(chain, *submitted_at, &tx))?;
            Ok(summary(
                StationOwnerRecoveryFilingPayload::TX_KIND,
                station_id,
                *nonce,
                &tx,
                out,
            ))
        },
        C::RecoverRequest {
            station_id,
            from_owner_address,
            to_pubkey,
            nonce,
            chain,
        } => {
            let pk = PublicKey::from_hex(to_pubkey)
                .map_err(|e| anyhow!("--to-pubkey is not valid ed25519 hex: {e}"))?;
            let body = RecoveryBody {
                chain_id: chain.chain_id.clone(),
                network_id: chain.network_id.clone(),
                station_id: station_id.clone(),
                from_owner_address: from_owner_address.clone(),
                to_owner_address: pk.to_address(),
                to_owner_pubkey: pk.to_hex(),
                recovery_nonce: *nonce,
            };
            Ok(json!({
                "station_id": station_id,
                "to_owner_address": body.to_owner_address,
                "nonce": nonce,
                "request_text": encode_recovery_request(&body),
                "next": "the claimant opens request_text in their phone and returns the recovery-accept text; pass it to `station recover-file --accept-text`",
            }))
        },
        C::RecoverAccept {
            station_id,
            from_owner_address,
            new_owner_key,
            nonce,
            chain,
        } => {
            let k = read_key(new_owner_key)?;
            let pk = PublicKey::from_private_key(&k);
            let mut payload = StationOwnerRecoveryFilingPayload::new(
                station_id,
                from_owner_address,
                pk.to_address(),
                pk.to_hex(),
                *nonce,
            );
            payload.accept_with_domain(&chain.chain_id, &chain.network_id, &k);
            let body = RecoveryBody {
                chain_id: chain.chain_id.clone(),
                network_id: chain.network_id.clone(),
                station_id: station_id.clone(),
                from_owner_address: from_owner_address.clone(),
                to_owner_address: pk.to_address(),
                to_owner_pubkey: pk.to_hex(),
                recovery_nonce: *nonce,
            };
            let accept_text = payload
                .to_owner_signature
                .as_deref()
                .map(|sig| encode_recovery_accept(&body, sig));
            Ok(json!({
                "station_id": station_id,
                "from_owner_address": from_owner_address,
                "to_pubkey": pk.to_hex(),
                "to_owner_address": pk.to_address(),
                "nonce": nonce,
                "to_owner_signature": payload.to_owner_signature,
                "accept_text": accept_text,
                "next": "pass --to-pubkey and --to-owner-signature to `station recover-file`",
            }))
        },
        C::RecoverCancel(a) => {
            let key = read_key(&a.owner_key)?;
            let mut payload = StationOwnerRecoveryCancelPayload::new(
                &a.station_id,
                address_of(&key),
                PublicKey::from_private_key(&key).to_hex(),
                a.nonce,
            );
            payload.sign_with_domain(&a.chain.chain_id, &a.chain.network_id, &key);
            payload
                .verify_stateless_with_domain(&a.chain.chain_id, &a.chain.network_id)
                .map_err(|e| anyhow!("self-verification failed: {e:?}"))?;
            let tx = envelope(serde_json::to_string(&payload)?, a.submitted_at)?;
            write_artifact(&a.out, &artifact_of(&a.chain, a.submitted_at, &tx))?;
            Ok(summary(
                StationOwnerRecoveryCancelPayload::TX_KIND,
                &a.station_id,
                a.nonce,
                &tx,
                &a.out,
            ))
        },
        C::RecoverExecute {
            station_id,
            nonce,
            chain,
            submitted_at,
            out,
        } => {
            let payload = StationOwnerRecoveryExecutePayload::new(station_id, *nonce);
            // Unsigned: the output address is a fixed placeholder.
            let tx = envelope(serde_json::to_string(&payload)?, *submitted_at)?;
            write_artifact(out, &artifact_of(chain, *submitted_at, &tx))?;
            Ok(summary(
                StationOwnerRecoveryExecutePayload::TX_KIND,
                station_id,
                *nonce,
                &tx,
                out,
            ))
        },
    }
}
