//! Authorized on-chain gamification-reward payout (mobile#215).
//!
//! `POST /rewards/payout` lets the backend (holder of `WRFCOIN_OPERATOR_TOKEN`)
//! pay real WRF when a user claims an achievement reward.
//!
//! ## Why this is not `seed_account`
//!
//! Unlike `debug::seed_account` — which mutates consensus-hashed balance state
//! out of band and therefore forks the node (#1811) — this endpoint mints
//! NOTHING directly. It builds a `TransactionType::Reward` transaction and
//! enqueues it into the mempool. The tx rides the next mined block and is
//! deterministically reconstructed by every peer via the transition extractor
//! into `StateTransition::ApplyGamificationReward`, which performs the credit,
//! the idempotency check, and the supply guardrail identically on producer,
//! applier, and WAL replay.
//!
//! ## Idempotency
//!
//! The backend's `claim_id` is the single dedup source of truth. It is recorded
//! in `State::gamification_claims_seen` (hashed into `state_root`). Submitting
//! the same `claim_id` again is safe: if already applied on-chain the endpoint
//! reports `already_processed` and mints nothing; if still pending it reports
//! `pending` with the same deterministic `tx_hash`.

use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{envelope_error, envelope_success, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::transaction::Transaction;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::consensus::proof_of_publishing::{MAX_BLOCK_EMISSION, MAX_SUPPLY};
use wrfcoin_native_chain::crypto::domain_signing::is_authorized_gamification_operator;
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_native_chain::security::api_auth::ApiRole;

/// Max accepted `claim_id` length (defensive bound on unbounded consensus set
/// growth and on tx size).
const MAX_CLAIM_ID_LEN: usize = 128;

/// Legacy private-hex setting: presence (even empty/non-Unicode) is refused.
const OPERATOR_SIGNING_KEY_ENV: &str = "WRFCOIN_GAMIFICATION_OPERATOR_SIGNING_KEY";
/// PUBLIC configuration: path to the dedicated hex Ed25519 key file.
const OPERATOR_SIGNING_KEY_FILE_ENV: &str = "WRFCOIN_GAMIFICATION_OPERATOR_SIGNING_KEY_FILE";

type SigningKeyError = (StatusCode, &'static str, String);

fn signing_key_error(code: &'static str, message: &'static str) -> SigningKeyError {
    (StatusCode::SERVICE_UNAVAILABLE, code, message.to_owned())
}

/// Load the payout key only from a protected file, never private hex in env.
/// All errors are fixed text: no file content, paths or OS error strings.
fn load_operator_signing_key() -> Result<PrivateKey, SigningKeyError> {
    // determinism-ok: API configuration refusal only; does not enter consensus validation or the signed payout preimage.
    if std::env::var_os(OPERATOR_SIGNING_KEY_ENV).is_some() {
        return Err(signing_key_error(
            "OPERATOR_SIGNING_KEY_ENV_REFUSED",
            "Legacy payout private-key environment setting is forbidden; remove it and configure the protected key file",
        ));
    }
    // determinism-ok: API-local key-file selection; consensus verifies the resulting signature and authorized public key.
    let path = std::env::var_os(OPERATOR_SIGNING_KEY_FILE_ENV)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            signing_key_error(
                "OPERATOR_SIGNING_KEY_UNSET",
                "Payout signing key file is not configured",
            )
        })?;
    let key = read_operator_signing_key_file(std::path::Path::new(&path))?;
    let pubkey = PublicKey::from_private_key(&key).to_hex();
    if !is_authorized_gamification_operator(&pubkey) {
        return Err(signing_key_error(
            "OPERATOR_KEY_NOT_ALLOWLISTED",
            "The configured payout public key is not in WRFCOIN_GAMIFICATION_OPERATOR_PUBKEYS",
        ));
    }
    Ok(key)
}

#[cfg(unix)]
fn read_operator_signing_key_file(path: &std::path::Path) -> Result<PrivateKey, SigningKeyError> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    // Refuse all final-component symlinks. Inspect the descriptor we actually
    // read, not a path stat vulnerable to a stat/open replacement race.
    // NONBLOCK also prevents a FIFO path from hanging before type validation.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| {
            signing_key_error(
                "OPERATOR_SIGNING_KEY_FILE_UNAVAILABLE",
                "Payout signing key file cannot be opened; symlinks are forbidden",
            )
        })?;
    let metadata = file.metadata().map_err(|_| {
        signing_key_error(
            "OPERATOR_SIGNING_KEY_FILE_UNAVAILABLE",
            "Payout signing key file metadata is unavailable",
        )
    })?;
    if !metadata.is_file()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(signing_key_error(
            "OPERATOR_SIGNING_KEY_FILE_INSECURE",
            "Payout signing key must be a regular mode-0600 file owned by the effective user",
        ));
    }
    // Use the canonical key-loader guard too. Keep the stricter descriptor
    // checks above: this path check never replaces validation of the opened file.
    wrfcoin_shared_protocol::security::key_file::assert_key_file_permissions(path).map_err(|_| {
        signing_key_error(
            "OPERATOR_SIGNING_KEY_FILE_INSECURE",
            "Payout signing key must be a regular mode-0600 file owned by the effective user",
        )
    })?;
    // 64 hex characters plus optional surrounding whitespace; bound reads even
    // if the same inode grows after metadata inspection. Zeroize the text copy.
    const MAX_KEY_FILE_BYTES: u64 = 128;
    let mut secret = zeroize::Zeroizing::new(String::new());
    file.take(MAX_KEY_FILE_BYTES + 1)
        .read_to_string(&mut secret)
        .map_err(|_| {
            signing_key_error(
                "OPERATOR_SIGNING_KEY_INVALID",
                "Payout signing key file must contain one hex Ed25519 secret",
            )
        })?;
    if secret.len() as u64 > MAX_KEY_FILE_BYTES {
        return Err(signing_key_error(
            "OPERATOR_SIGNING_KEY_INVALID",
            "Payout signing key file must contain one hex Ed25519 secret",
        ));
    }
    PrivateKey::from_hex(secret.trim()).map_err(|_| {
        signing_key_error(
            "OPERATOR_SIGNING_KEY_INVALID",
            "Payout signing key file must contain one hex Ed25519 secret",
        )
    })
}

#[cfg(not(unix))]
fn read_operator_signing_key_file(_path: &std::path::Path) -> Result<PrivateKey, SigningKeyError> {
    Err(signing_key_error(
        "OPERATOR_SIGNING_KEY_FILE_UNSUPPORTED",
        "Payout key-file permission enforcement requires Unix",
    ))
}

#[derive(Debug, Deserialize)]
pub struct PayoutRequest {
    /// Recipient address, 20-byte hex.
    pub address: String,
    /// Payout amount in base units (1 WRF = 10^8 base units).
    pub amount_base_units: u128,
    /// Backend-generated idempotency key. Same `claim_id` → at most one payout.
    pub claim_id: String,
}

#[derive(Debug, Serialize)]
pub struct PayoutResponse {
    /// `accepted` (new tx enqueued), `pending` (identical claim already in the
    /// mempool), or `already_processed` (claim already applied on-chain).
    pub status: &'static str,
    pub claim_id: String,
    /// Transaction hash to poll for inclusion. Present for `accepted`/`pending`;
    /// omitted for `already_processed` (the original tx is already mined).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub address: String,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub amount_base_units: u128,
}

/// POST /rewards/payout — Operator-gated authorized gamification payout.
pub async fn payout(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<PayoutRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    // --- Validate address (format only; the string form is used to build the
    // tx output so the extractor re-parses it identically on every peer) ---
    let _address = match body.address.parse::<Address>() {
        Ok(addr) => addr,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "Invalid address format. Expected 20-byte hex address.",
            );
        },
    };

    // --- Validate claim_id ---
    let claim_id = body.claim_id.trim().to_string();
    if claim_id.is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "claim_id is required",
        );
    }
    if claim_id.len() > MAX_CLAIM_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("claim_id must be at most {MAX_CLAIM_ID_LEN} bytes"),
        );
    }

    // --- Validate amount ---
    if body.amount_base_units == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "amount_base_units must be greater than 0",
        );
    }
    // The injection vehicle is a Reward tx whose output amount is u64; reject
    // amounts that would not fit the on-chain representation.
    let amount_u64: u64 = match u64::try_from(body.amount_base_units) {
        Ok(v) => v,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "AMOUNT_TOO_LARGE",
                "amount_base_units exceeds the maximum representable payout",
            );
        },
    };
    // A single payout may not exceed the per-block emission cap (200 WRF).
    if body.amount_base_units > MAX_BLOCK_EMISSION {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "AMOUNT_EXCEEDS_BLOCK_EMISSION",
            format!(
                "amount_base_units {} exceeds MAX_BLOCK_EMISSION {}",
                body.amount_base_units, MAX_BLOCK_EMISSION
            ),
        );
    }

    // Refuse legacy env configuration and insecure/unavailable key files even
    // for a seen claim. A successful idempotency readback must not conceal an
    // unsupported signer configuration.
    let operator_key = match load_operator_signing_key() {
        Ok(k) => k,
        Err((status, code, msg)) => return envelope_error(&req, status, code, msg),
    };

    // --- Idempotency + supply pre-checks against current consensus state ---
    // These are fast-feedback guards; the authoritative checks run at apply
    // time in `ApplyGamificationReward` so a race between here and inclusion can
    // never double-mint or over-mint.
    {
        let blockchain = state.blockchain.read().await;
        if blockchain
            .state
            .gamification_claims_seen
            .contains(&claim_id)
        {
            return envelope_success(
                &req,
                StatusCode::OK,
                PayoutResponse {
                    status: "already_processed",
                    claim_id,
                    tx_hash: None,
                    address: body.address.clone(),
                    amount_base_units: body.amount_base_units,
                },
            );
        }
        let projected = blockchain
            .state
            .issuance_committed_supply()
            .saturating_add(body.amount_base_units);
        if projected > MAX_SUPPLY {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "OVER_SUPPLY",
                format!(
                    "payout would exceed MAX_SUPPLY: committed supply {} + {} > {}",
                    blockchain.state.issuance_committed_supply(),
                    body.amount_base_units,
                    MAX_SUPPLY
                ),
            );
        }
    }

    // --- Sign the payout as an allowlisted operator and enqueue it ---
    // The signature binds this exact recipient/amount/claim into the tx bytes,
    // so a peer cannot forge or front-run the payout (#2081 review).
    let tx =
        Transaction::new_gamification_reward(&body.address, amount_u64, &claim_id, &operator_key);
    let tx_hash = tx.id.as_ref().map(|h| h.to_hex()).unwrap_or_default();

    let add_result = {
        let mut mempool = state.mempool.write().await;
        mempool.add_transaction(tx)
    };

    match add_result {
        Ok(()) => {
            log::info!(
                target: "rewards",
                "Gamification payout enqueued: claim_id={} address={} amount={} tx={}",
                claim_id, body.address, amount_u64, tx_hash,
            );
            envelope_success(
                &req,
                StatusCode::ACCEPTED,
                PayoutResponse {
                    status: "accepted",
                    claim_id,
                    tx_hash: Some(tx_hash),
                    address: body.address.clone(),
                    amount_base_units: body.amount_base_units,
                },
            )
        },
        Err(e) => {
            let msg = e.to_string();
            // A re-submission of the same claim collides on its deterministic
            // outpoint while the first is still pending — report it as
            // idempotent-pending, not an error.
            //
            // #2081 review (P2): DO NOT return `tx_hash` here. `tx_hash` is the
            // hash of the duplicate we just built, and `new_gamification_reward`
            // folds the current wall-clock timestamp into that hash — so it
            // differs from the pending tx's hash and, having just been rejected
            // by double-spend detection, names a transaction that can never be
            // mined. A client polling it would wait forever. The pending payout
            // is identified by its `claim_id` (the idempotency key), which the
            // client already holds; omit the misleading hash.
            if msg.contains("already in mempool") || msg.contains("Double spend") {
                return envelope_success(
                    &req,
                    StatusCode::ACCEPTED,
                    PayoutResponse {
                        status: "pending",
                        claim_id,
                        tx_hash: None,
                        address: body.address.clone(),
                        amount_base_units: body.amount_base_units,
                    },
                );
            }
            // Minority-partition read-only mempool (#1217): back off.
            if msg.contains("read-only") {
                return envelope_error(
                    &req,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "PARTITION_READ_ONLY",
                    format!("payout rejected: {msg}"),
                );
            }
            envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_SERVER_ERROR",
                format!("failed to enqueue payout: {msg}"),
            )
        },
    }
}
