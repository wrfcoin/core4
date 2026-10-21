use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{
    envelope_error, live_network_mutation_rejected, oracle_admin_api_disabled,
    parse_oracle_public_key, require_role, submit_protocol_transaction, validate_commit_hash,
    validate_nonce, validate_participant_id, validate_timestamp,
};
use crate::state::{AppState, MAX_ORACLE_SET_SIZE};
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::blockchain::transaction::{
    OracleAttestationPayload, Transaction, TransactionType,
};
use wrfcoin_native_chain::consensus::proof_of_publishing::DEFAULT_SUBMISSION_TIME_WINDOW_SECS;
use wrfcoin_native_chain::crypto::domain_signing::{signing_chain_id, signing_network_id};
use wrfcoin_native_chain::crypto::signature::{PublicKey, Signature};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::security::key_registry::SigningRole;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleAttestationRequest {
    pub oracle_id: String,
    pub commit_hash: String,
    pub approved: bool,
    pub signature: Option<String>,
    pub confidence: Option<f64>,
    pub timestamp: Option<i64>,
    /// Unique nonce to prevent replay attacks.
    pub nonce: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleConfigRequest {
    pub oracle_set: Option<Vec<String>>,
    pub oracle_quorum: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleOperatorRegisterRequest {
    pub oracle_id: String,
    pub public_key: String,
    pub epoch: Option<u64>,
    pub activated_at: Option<i64>,
    pub oracle_quorum: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleConfigResponse {
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleOperatorRegisterResponse {
    pub oracle_id: String,
    pub public_key: String,
    pub epoch: u64,
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
    pub state_root_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleStatusResponse {
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
    pub tracked_commit_count: usize,
    pub commits_with_quorum: usize,
    pub commits_pending_quorum: usize,
}

#[derive(Debug, Serialize)]
pub struct OracleLeaderboardEntry {
    pub oracle_id: String,
    pub reputation_score: u64,
    pub reputation_pct: f64,
    pub tier: String,
}

#[derive(Debug, Serialize)]
pub struct OracleLeaderboardResponse {
    pub entries: Vec<OracleLeaderboardEntry>,
    pub total_oracles: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn submit_oracle_attestation(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<OracleAttestationRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Oracle) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.oracle_id, "oracle_id") {
        return resp;
    }
    if let Err(resp) = validate_commit_hash(&req, &request.commit_hash) {
        return resp;
    }
    if let Err(resp) = parse_oracle_public_key(&req, &request.oracle_id).map(|_| ()) {
        return resp;
    };
    if let Some(confidence) = request.confidence {
        if !(0.0..=1.0).contains(&confidence) {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "confidence must be between 0 and 1",
            );
        }
    }
    let signature_hex = match request
        .signature
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(sig) => sig,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "signature is required",
            );
        },
    };

    // Nonce is required on the oracle attestation path (#1844).
    let nonce_str = match request.nonce.as_deref() {
        Some(n) => n,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "nonce is required",
            );
        },
    };
    if let Err(resp) = validate_nonce(&req, nonce_str) {
        return resp;
    }
    let timestamp = request
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, timestamp, "timestamp") {
        return resp;
    }
    let _signature = match Signature::from_hex(signature_hex) {
        Ok(sig) => sig,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "signature must be a valid ed25519 signature hex string",
            );
        },
    };
    let payload = OracleAttestationPayload {
        commit_id: request.commit_hash.clone(),
        oracle_id: request.oracle_id.clone(),
        approved: request.approved,
        signature: Some(signature_hex.to_string()),
        confidence: request.confidence,
        timestamp,
        nonce: request.nonce.clone(),
    };
    let message = payload.signing_message();
    // Defense-in-depth: require oracle_id to be a registered operator before
    // attempting crypto verification.  Without this guard the legacy
    // identity-as-public-key fallback in KeyRegistry::active_keys_at would
    // silently accept signatures from any caller that uses their own public key
    // as oracle_id, producing confusing errors at mempool admission (#1844).
    // The mempool also enforces has_identity_role, but we fail-closed here too.
    let (oracle_registered, signature_valid) = {
        let blockchain = state.blockchain.read().await;
        let registered = blockchain
            .state
            .signing_keys
            .has_identity_role(&request.oracle_id, SigningRole::Oracle);
        let sig_valid = registered
            && blockchain
                .state
                .signing_keys
                .verify_signature_for_identity_role(
                    &request.oracle_id,
                    SigningRole::Oracle,
                    timestamp,
                    message.as_bytes(),
                    signature_hex,
                );
        (registered, sig_valid)
    };
    if !oracle_registered {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_id is not a registered oracle operator",
        );
    }
    if !signature_valid {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle attestation signature verification failed",
        );
    }
    let payload_value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    submit_protocol_transaction(
        &req,
        state,
        TransactionType::OracleAttestation,
        &request.oracle_id,
        payload_value,
    )
    .await
}

pub async fn register_oracle_operator(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<OracleOperatorRegisterRequest>,
) -> impl Responder {
    // #1775: this route mutates oracle_set/quorum + signing_keys out of band
    // (folded into the consensus state_root, never gossiped/extracted) — gated
    // off by default to prevent forking this node from its peers.
    if !state.oracle_admin_api_enabled {
        return oracle_admin_api_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.oracle_id, "oracle_id") {
        return resp;
    }
    if let Err(resp) = parse_oracle_public_key(&req, &request.oracle_id).map(|_| ()) {
        return resp;
    };
    if PublicKey::from_hex(request.public_key.trim()).is_err() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "public_key must be a valid ed25519 public key hex string",
        );
    }
    let activated_at = request
        .activated_at
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, activated_at, "activated_at") {
        return resp;
    }

    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    let mut oracle_set = blockchain
        .state
        .oracle_set
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    oracle_set.push(request.oracle_id.trim().to_string());
    oracle_set.sort();
    oracle_set.dedup();

    if oracle_set.len() > MAX_ORACLE_SET_SIZE {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("oracle_set cannot exceed {MAX_ORACLE_SET_SIZE} entries"),
        );
    }

    let oracle_quorum = request
        .oracle_quorum
        .unwrap_or_else(|| blockchain.state.oracle_quorum.max(1).min(oracle_set.len()));
    if oracle_quorum == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_quorum must be greater than zero",
        );
    }
    if oracle_quorum > oracle_set.len() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_quorum cannot exceed oracle_set size",
        );
    }

    let epoch = request.epoch.unwrap_or(1);
    if epoch == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "epoch 0 is reserved for legacy oracle identity keys",
        );
    }

    let identity = request.oracle_id.trim().to_string();
    let public_key = request.public_key.trim().to_string();
    let config_transition = StateTransition::SetOracleConfig {
        oracle_set: oracle_set.clone(),
        oracle_quorum,
    };
    let key_transition = StateTransition::RegisterSigningKeyEpoch {
        identity: identity.clone(),
        role: SigningRole::Oracle,
        epoch,
        public_key: public_key.clone(),
        activated_at,
    };

    let mut preview = blockchain.clone();
    if let Err(err) = preview.apply_state_transition(config_transition.clone()) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to update oracle config: {err}"),
        );
    }
    if let Err(err) = preview.apply_state_transition(key_transition.clone()) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to register oracle signing key: {err}"),
        );
    }

    if let Err(err) = blockchain.apply_state_transition(config_transition) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to update oracle config: {err}"),
        );
    }
    if let Err(err) = blockchain.apply_state_transition(key_transition) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to register oracle signing key: {err}"),
        );
    }

    HttpResponse::Created().json(OracleOperatorRegisterResponse {
        oracle_id: identity,
        public_key,
        epoch,
        oracle_set,
        oracle_quorum,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

pub async fn get_oracle_config(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    let mut oracle_set = blockchain
        .state
        .oracle_set
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    oracle_set.sort();
    HttpResponse::Ok().json(OracleConfigResponse {
        oracle_set,
        oracle_quorum: blockchain.state.oracle_quorum,
    })
}

pub async fn get_oracle_status(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    let mut oracle_set = blockchain
        .state
        .oracle_set
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    oracle_set.sort();

    let tracked_commit_count = blockchain.state.attestations.len();
    let commits_with_quorum = blockchain
        .state
        .attestations
        .values()
        .filter(|set| set.has_quorum(blockchain.state.oracle_quorum))
        .count();
    let commits_pending_quorum = tracked_commit_count.saturating_sub(commits_with_quorum);

    HttpResponse::Ok().json(OracleStatusResponse {
        oracle_set,
        oracle_quorum: blockchain.state.oracle_quorum,
        tracked_commit_count,
        commits_with_quorum,
        commits_pending_quorum,
    })
}

pub async fn get_oracle_leaderboard(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    const SCORE_SCALE: u64 = 1_000_000;
    const DEFAULT_LIMIT: usize = 50;

    let registry = blockchain.state.oracle_registry();
    let raw_entries = registry.leaderboard(DEFAULT_LIMIT.min(MAX_ORACLE_SET_SIZE));
    let total_oracles = registry.oracle_count();

    let entries = raw_entries
        .into_iter()
        .map(|(oracle_id, score)| {
            let pct = (score as f64 / SCORE_SCALE as f64) * 100.0;
            let tier = if pct >= 75.0 {
                "Platinum"
            } else if pct >= 50.0 {
                "Gold"
            } else if pct >= 25.0 {
                "Silver"
            } else {
                "Bronze"
            };
            OracleLeaderboardEntry {
                oracle_id,
                reputation_score: score,
                reputation_pct: (pct * 100.0).round() / 100.0,
                tier: tier.to_string(),
            }
        })
        .collect();

    HttpResponse::Ok().json(OracleLeaderboardResponse {
        entries,
        total_oracles,
    })
}

pub async fn update_oracle_config(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<OracleConfigRequest>,
) -> impl Responder {
    // #1775: out-of-band SetOracleConfig mutation — gated off by default.
    if !state.oracle_admin_api_enabled {
        return oracle_admin_api_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    let current_state = &blockchain.state;

    let requested_set = request.oracle_set.clone().unwrap_or_else(|| {
        current_state
            .oracle_set
            .iter()
            .cloned()
            .collect::<Vec<String>>()
    });

    let mut normalized_set = requested_set
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    normalized_set.sort();
    normalized_set.dedup();

    if normalized_set.len() > MAX_ORACLE_SET_SIZE {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("oracle_set cannot exceed {MAX_ORACLE_SET_SIZE} entries"),
        );
    }
    if normalized_set.is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_set must include at least one oracle id",
        );
    }
    for oracle_id in &normalized_set {
        if let Err(resp) = validate_participant_id(&req, oracle_id, "oracle_set entry") {
            return resp;
        }
        if let Err(resp) = parse_oracle_public_key(&req, oracle_id) {
            return resp;
        }
    }

    let oracle_quorum = request.oracle_quorum.unwrap_or(current_state.oracle_quorum);
    if oracle_quorum == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_quorum must be greater than zero",
        );
    }
    if oracle_quorum > normalized_set.len() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "oracle_quorum cannot exceed oracle_set size",
        );
    }

    if let Err(err) = blockchain.apply_state_transition(StateTransition::SetOracleConfig {
        oracle_set: normalized_set.clone(),
        oracle_quorum,
    }) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to update oracle config: {err}"),
        );
    }

    HttpResponse::Ok().json(OracleConfigResponse {
        oracle_set: normalized_set,
        oracle_quorum,
    })
}

// ---------------------------------------------------------------------------
// Genesis oracle registration submission (#2530)
// ---------------------------------------------------------------------------

/// A batch of governance-signed `OracleRegistration` transactions, verbatim
/// from a `genesis_oracle_ceremony_v1` artifact.
///
/// `chain_id` / `network_id` are carried explicitly rather than inferred: the
/// governance signature binds them (see
/// `oracle_registration_authorization_message_with_domain`), and the node
/// resolves its own from the environment with a silent default. Without the
/// declared pair, a ceremony run under a different chain identity fails
/// authorization on every transaction with no way to tell that apart from a
/// bad key.
#[derive(Debug, Serialize, Deserialize)]
pub struct OracleRegistrationSubmitRequest {
    pub chain_id: String,
    pub network_id: String,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleRegistrationSubmitResponse {
    pub accepted: usize,
    pub transaction_ids: Vec<String>,
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
}

/// Admit a ceremony's genesis oracle registrations to the mempool.
///
/// Categorically different from [`register_oracle_operator`], which mutates
/// `oracle_set` / `oracle_quorum` out of band and is gated off by default
/// (#1775). This route puts *transactions* in the mempool and nothing else, so
/// the set enters consensus the same way on every node.
///
/// The transactions are admitted **verbatim**. They are not rebuilt through
/// `enqueue_protocol_transaction`, which stamps a fresh wall-clock timestamp
/// and a nanosecond-seeded input txid — that would give each node a different
/// transaction id for the same registration, and therefore a different block 1.
pub async fn submit_oracle_registrations(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<OracleRegistrationSubmitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    if request.transactions.is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "transactions must not be empty",
        );
    }
    // No explicit batch cap here: `MAX_JSON_PAYLOAD_BYTES` (64 KiB) rejects an
    // oversized body before this handler runs, so a cap on
    // `transactions.len()` would be unreachable code. That limit is also the
    // real ceiling on the ceremony — each registration carries the ABSOLUTE
    // oracle set (#2055), so a batch is O(N^2) in the set size and stops
    // fitting somewhere above 20 oracles. See the batch-size test.

    // Chain-identity pre-check. Reported before signature verification so the
    // operator gets the actual cause instead of N opaque authorization
    // failures.
    let node_chain_id = signing_chain_id();
    let node_network_id = signing_network_id();
    if request.chain_id != node_chain_id || request.network_id != node_network_id {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!(
                "ceremony chain identity ({}/{}) does not match this node ({}/{}); \
                 the governance signatures bind the chain identity, so these \
                 registrations cannot authorize here",
                request.chain_id, request.network_id, node_chain_id, node_network_id
            ),
        );
    }

    // Validate the whole batch before admitting any of it. A partially admitted
    // batch is the failure this route exists to prevent: block 1 would carry an
    // incomplete oracle set, and the #2496 agreement rule would reject it on
    // every peer.
    // One clock read for the whole batch: sampling per transaction could accept
    // the first and reject the last of an otherwise identical set.
    let now_secs = chrono::Utc::now().timestamp(); // determinism-ok: admission-time freshness gate at the API boundary; decides accept/reject only, never alters the bytes that reach consensus
    let submission_window = DEFAULT_SUBMISSION_TIME_WINDOW_SECS;
    // Admit only within HALF the assembly window.
    //
    // Passing this check is necessary but not sufficient for inclusion: block
    // assembly measures against `producer_block_timestamp`, which clamps
    // FORWARD past the previous block and past median-time-past, and more time
    // passes between admission and assembly. For a past-dated transaction the
    // assembly delta is therefore always >= the delta measured here, so a batch
    // admitted near the boundary can still be dropped — back into the empty
    // block 1 this check exists to prevent, through a narrower door.
    //
    // The margin is affordable because these transactions are ceremony output:
    // rebuilding the artifact with a current `--submitted-at` takes seconds. An
    // artifact already halfway to expiry is better refused than admitted and
    // silently discarded.
    let admission_window = submission_window / 2;

    let mut declared: Option<(Vec<String>, usize)> = None;
    let mut transaction_ids: Vec<String> = Vec::new();
    for (index, tx) in request.transactions.iter().enumerate() {
        if tx.tx_type != TransactionType::OracleRegistration {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] is {:?}, expected OracleRegistration",
                    tx.tx_type
                ),
            );
        }
        let Some(payload) = tx.parse_oracle_registration() else {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!("transactions[{index}] does not carry a parseable oracle registration"),
            );
        };
        if !tx.verify_oracle_registration_authorization() {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] (identity {}) is not authorized by an \
                     allowlisted governance key",
                    payload.identity
                ),
            );
        }
        // Block assembly drops any transaction outside the submission window
        // (`filter_by_submission_timing`) and reports it only as a `warn!` and
        // a `pop_skipped_mempool_transactions` entry in block metadata. The
        // submitter already got 201. Left unchecked, a ceremony artifact with a
        // stale `submitted_at` is accepted here, silently dropped at assembly,
        // and block 1 is cut with NO oracle set — the chain comes up N=0 and
        // nothing on the submission path ever said no.
        let tx_time = tx.timestamp.timestamp();
        let delta = now_secs.abs_diff(tx_time);
        if delta > admission_window {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] (identity {}) has a submitted_at {delta}s from \
                     now, beyond the {admission_window}s admission margin (half the \
                     {submission_window}s window block assembly enforces). Assembly \
                     measures against a block timestamp that only ever moves forward, so \
                     this batch could still be dropped and cut block 1 without the oracle \
                     set. Rebuild the ceremony artifact with a current --submitted-at",
                    payload.identity
                ),
            );
        }

        // The transaction id is the ceremony's determinism guarantee: it folds
        // the pinned timestamp and the full payload. An id that is absent or
        // does not recompute means the artifact was edited after signing, so
        // this node would put different bytes in block 1 than its peers.
        match tx.id.as_ref() {
            Some(id) if *id == tx.calculate_hash() => transaction_ids.push(id.to_hex()),
            Some(_) => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    format!( // secret-leak-ok: interpolates an ed25519 PUBLIC key
                        "transactions[{index}] (identity {}) has an id that does not \
                         match its contents; the artifact was modified after signing",
                        payload.identity
                    ),
                );
            },
            None => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    format!(
                        "transactions[{index}] (identity {}) carries no transaction id",
                        payload.identity
                    ),
                );
            },
        }

        match declared.as_ref() {
            None => declared = Some((payload.oracle_set.clone(), payload.oracle_quorum)),
            Some((set, quorum)) => {
                // The block-1 agreement rule (#2496) enforces this at block
                // validation. Enforcing it at admission too means an
                // incremental-shaped batch is refused while it is still
                // fixable, rather than after the cut.
                if payload.oracle_set != *set || payload.oracle_quorum != *quorum {
                    return envelope_error(
                        &req,
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "UNPROCESSABLE_ENTITY",
                        format!(
                            "transactions[{index}] (identity {}) declares a different \
                             oracle set/quorum than transactions[0]; every genesis \
                             registration must carry the identical absolute set",
                            payload.identity
                        ),
                    );
                }
            },
        }
    }

    // `declared` is Some for any non-empty batch, and empty was rejected above.
    // Handled rather than unwrapped so the route has no panic path at all.
    let Some((oracle_set, oracle_quorum)) = declared else {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "transactions must not be empty",
        );
    };

    {
        let mut mempool = state.mempool.write().await;
        // Roll back on failure. Validating the whole batch first is not enough:
        // `add_transaction` can still reject item N (duplicate, capacity, fee
        // floor) after items 1..N-1 are already in, and returning there leaves a
        // PARTIAL oracle set in the mempool. Block 1 would then be cut carrying
        // some registrations but not all — the incremental shape #2496 rejects,
        // reached without anyone constructing an incremental artifact.
        //
        // Found by an independent verifier probing this exact path; the earlier
        // version of this loop did leave the first transaction behind.
        let mut admitted: Vec<wrfcoin_native_chain::crypto::hash::Hash> = Vec::new();
        for (index, tx) in request.transactions.iter().enumerate() {
            if let Err(err) = mempool.add_transaction(tx.clone()) {
                for id in &admitted {
                    mempool.remove_transaction(id);
                }
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    format!(
                        "transactions[{index}] rejected by mempool: {err}; \
                         the {} already admitted were rolled back so block 1 \
                         cannot be cut with a partial oracle set",
                        admitted.len()
                    ),
                );
            }
            if let Some(id) = tx.id.as_ref() {
                admitted.push(*id);
            }
        }
    }

    HttpResponse::Created().json(OracleRegistrationSubmitResponse {
        accepted: transaction_ids.len(),
        transaction_ids,
        oracle_set,
        oracle_quorum,
    })
}

// ---------------------------------------------------------------------------
// GET /transactions/oracle/pending — the attester read surface (#2464)
// ---------------------------------------------------------------------------

/// One observation awaiting attestation, carrying the full reveal preimage an
/// attester needs to recompute the commit id, plus the attestation state it
/// needs to decide whether attesting is still useful work.
///
/// This is deliberately the shape of `shared-protocol`'s
/// `oracle::attester::PendingObservation` (commit_id, station_id, nonce,
/// timestamp, data, contribution_type) with node-side context alongside —
/// no other endpoint exposes the reveal `data`/`nonce` at all
/// (`/transactions/contributions/status` reports quorum state but not the
/// reading), and without the preimage an oracle can only rubber-stamp.
#[derive(Debug, Serialize)]
pub struct PendingAttestationObservation {
    pub commit_id: String,
    pub station_id: String,
    pub timestamp: i64,
    pub nonce: String,
    pub contribution_type: String,
    pub data: serde_json::Value,
    /// `"mempool"` — the reveal is unmined; an attestation submitted now can
    /// mine before or with it (the prompt reward path). `"block"` — the
    /// reveal already mined without quorum; an attestation now settles via
    /// the deferred late-quorum path (#2508).
    pub source: String,
    /// Height the reveal was applied at, for `source == "block"` only.
    pub revealed_at_height: Option<u64>,
    pub provenance_class:
        wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
    /// The station's registered `location.elevation_meters` (#3285), or
    /// `None` when the station is unregistered or registered without one.
    /// The attester recomputes a reveal's `pressure_sea_level_hpa` from the
    /// raw `pressure_hpa` and this elevation before trusting it. Read-only,
    /// off-consensus context, like `provenance_class`; older attesters ignore
    /// the field.
    // determinism-ok: read-only API echo of the registry's Option<f64> elevation; nothing here is consensus arithmetic
    pub elevation_meters: Option<f64>,
    /// #3240: the registered station's location, COARSENED to
    /// [`PENDING_LOCATION_DECIMALS`] decimal places (~1.1 km), so a service-side
    /// attester can pick the nearest NWS reference station per commit. Read
    /// from the same `station_registry` entry that yields `provenance_class`;
    /// omitted for a station with no registry entry. Unsigned, read-only,
    /// derived from existing state: not part of any preimage, transaction or
    /// state root. Coarsened because nearest-station selection needs no more,
    /// and this surface must not widen exposure of exact coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station_location: Option<PendingStationLocation>,
    pub approvals: usize,
    pub rejections: usize,
    /// Oracle ids that have already attested this commit (approve or reject),
    /// either mined into chain state or admitted to the current mempool,
    /// sorted. An attester skips commits its own id already appears in; the
    /// mempool half prevents a daemon restart from submitting a duplicate
    /// before its first attestation mines.
    pub attesting_oracles: Vec<String>,
    pub has_quorum: bool,
    pub reward_recorded: bool,
    pub reward_finalized: bool,
}

/// Decimal places kept in [`PendingStationLocation`] (0.01° ≈ 1.1 km).
pub const PENDING_LOCATION_DECIMALS: i32 = 2;

/// Coarse station location on the pending surface (#3240).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingStationLocation {
    // determinism-ok: read-surface echo of registry coordinates for a service-side reference lookup; never enters consensus
    pub latitude: f64,
    // determinism-ok: as above
    pub longitude: f64,
}

impl PendingStationLocation {
    /// Coarsen a registry location. Non-finite values yield `None` rather
    /// than a location an attester would trust.
    pub fn coarse(
        location: &wrfcoin_shared_protocol::station_registry::StationLocation,
    ) -> Option<Self> {
        // determinism-ok: presentation rounding on a read-only response field; never consensus arithmetic
        let scale = 10f64.powi(PENDING_LOCATION_DECIMALS);
        // determinism-ok: as above
        let round = |v: f64| (v * scale).round() / scale;
        if !location.latitude.is_finite() || !location.longitude.is_finite() {
            return None;
        }
        Some(Self {
            latitude: round(location.latitude),
            longitude: round(location.longitude),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct PendingAttestationsResponse {
    /// The node's signing chain identity, echoed so an attester can refuse to
    /// sign for a node whose domain differs from its own configuration
    /// (#2713/#2744 trap class: a silent identity mismatch otherwise
    /// surfaces only as opaque signature rejections).
    pub chain_id: String,
    pub network_id: String,
    pub height: u64,
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
    /// What the reward path actually compares against
    /// (`state.oracle_quorum.max(1)`).
    pub effective_quorum: usize,
    pub observations: Vec<PendingAttestationObservation>,
    /// Unmined reveals whose canonical attestation set already satisfies the
    /// effective quorum. These are intentionally separate from `observations`:
    /// attesters have no more work to do, while a collector must be able to
    /// prove the exact reveal is safe to mine instead of treating its omission
    /// as an ambiguous timeout.
    pub ready_to_mine: Vec<PendingAttestationObservation>,
    /// Commits whose reveal payload could not be recovered from the in-memory
    /// window OR the durable block archive. With the archive fallback in
    /// place this should be empty on a healthy node; a non-empty list means
    /// the archive itself is missing the reveal's block. Reported rather than
    /// silently dropped: to an attester, an omission is indistinguishable
    /// from "nothing to do".
    pub unrecoverable: Vec<String>,
    /// Names of the consensus features in force at `height + 1`, the first
    /// block an attestation submitted now can mine in (#3131). The attester's
    /// reward-tier rule follows this list — `reward_gross_outlier_guard` switches on
    /// the gross-outlier guard — so every honest attester changes rules at
    /// the scheduled height, read from this node's genesis schedule, rather
    /// than whenever its own binary is redeployed. An older node omits the
    /// field; attesters then keep the pre-activation rule.
    pub active_features: Vec<String>,
}

/// Feature names in force at `height` on this node, in declaration order.
fn active_feature_names(height: u64) -> Vec<String> {
    active_feature_names_with(height, wrfcoin_shared_protocol::crypto::chain_binding::feature_active)
}

fn active_feature_names_with(
    height: u64,
    is_active: impl Fn(wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature, u64) -> bool,
) -> Vec<String> {
    wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::ALL
        .iter()
        .copied()
        .filter(|feature| is_active(*feature, height))
        .map(|feature| feature.name().to_string())
        .collect()
}

#[cfg(test)]
mod active_features_tests {
    use super::active_feature_names_with;
    use wrfcoin_shared_protocol::crypto::chain_binding::FeatureSchedule;

    /// #3131: the pending surface reports exactly what the genesis schedule
    /// has in force at the queried height — the attester's rule switch.
    #[test]
    fn active_features_follow_the_schedule_at_the_queried_height() {
        let schedule =
            FeatureSchedule::from_entries(vec![("reward_gross_outlier_guard".to_string(), 100)]).unwrap();
        let at = |height| active_feature_names_with(height, |f, h| schedule.active(f, h));
        assert!(at(99).is_empty());
        assert_eq!(at(100), vec!["reward_gross_outlier_guard".to_string()]);
        assert_eq!(at(u64::MAX), vec!["reward_gross_outlier_guard".to_string()]);
        // No schedule: nothing active, so attesters keep the pre-activation rule.
        assert!(active_feature_names_with(100, |_, _| false).is_empty());
    }

    /// #3277 S3: the production surface attesters read (`active_features` on
    /// the pending-attestations response) reflects features activated by an
    /// APPLIED schedule extension, not only genesis. Uses an activation at
    /// `u64::MAX` that no other test queries; the overlay is process-global
    /// and other tests in this binary refresh it from their own states, so the
    /// positive observation is retried rather than assumed race-free.
    #[test]
    fn active_features_reflect_applied_schedule_extensions_3277() {
        use wrfcoin_shared_protocol::blockchain::state::State;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            feature_schedule_extension_entries, refresh_feature_schedule_extensions,
        };
        use wrfcoin_shared_protocol::governance::schedule_extension::{
            AppliedExtension, ScheduleEntry,
        };
        let before = feature_schedule_extension_entries();
        let mut state = State::default();
        state.feature_schedule_extensions.push(AppliedExtension {
            sequence: 0,
            carried_at_height: 1,
            entries: vec![ScheduleEntry::new("paymaster", u64::MAX)],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "s3".into(),
            signers: Vec::new(),
        });
        let has_paymaster =
            |h: u64| super::active_feature_names(h).iter().any(|name| name == "paymaster");
        let mut observed = false;
        for _ in 0..200 {
            state.refresh_effective_feature_schedule();
            if has_paymaster(u64::MAX) {
                observed = true;
                break;
            }
        }
        assert!(observed, "an applied extension's feature must appear in active_features");
        state.refresh_effective_feature_schedule();
        assert!(
            !has_paymaster(u64::MAX - 1),
            "never before its activation height"
        );
        refresh_feature_schedule_extensions(before);
    }
}

#[derive(Debug, Deserialize)]
pub struct PendingAttestationsQuery {
    pub limit: Option<usize>,
}

const PENDING_ATTESTATIONS_DEFAULT_LIMIT: usize = 100;
const PENDING_ATTESTATIONS_MAX_LIMIT: usize = 500;
/// Upper bound on the mempool scan, above the response cap so quorum-complete
/// entries filtered out later cannot starve the page.
const PENDING_ATTESTATIONS_MEMPOOL_SCAN: usize = 2_000;

/// The read surface an oracle attester polls: every contribution reveal whose
/// commit still needs attestations, whether the reveal is still in the
/// mempool (attest now and the reward pays promptly, in the reveal's own
/// block) or already mined without quorum (attest now and the deferred
/// late-quorum settlement path pays, #2508).
pub async fn get_pending_attestations(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<PendingAttestationsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let limit = query
        .limit
        .unwrap_or(PENDING_ATTESTATIONS_DEFAULT_LIMIT)
        .clamp(1, PENDING_ATTESTATIONS_MAX_LIMIT);

    // Mempool first, blockchain second, never both locks at once — the same
    // ordering discipline as `enqueue_protocol_transaction`.
    let (mempool_reveals, mempool_attestations) = {
        let mempool = state.mempool.read().await;
        (
            mempool.get_transactions_by_type(
                TransactionType::ContributionReveal,
                PENDING_ATTESTATIONS_MEMPOOL_SCAN,
            ),
            mempool.get_transactions_by_type(
                TransactionType::OracleAttestation,
                PENDING_ATTESTATIONS_MEMPOOL_SCAN,
            ),
        )
    };

    // A daemon restart clears its process-local suppression cache. Include
    // already-admitted, not-yet-mined attestations in the discovery response
    // so restart cannot create a fresh duplicate for the same commit.
    let mut mempool_attesters_by_commit: std::collections::BTreeMap<
        String,
        std::collections::BTreeSet<String>,
    > = std::collections::BTreeMap::new();
    for tx in &mempool_attestations {
        let Some(payload) = tx.parse_oracle_attestation_payload() else {
            continue;
        };
        mempool_attesters_by_commit
            .entry(payload.commit_id)
            .or_default()
            .insert(payload.oracle_id);
    }

    // Lock order matches `get_late_quorum_unpaid`: blockchain, then storage.
    let blockchain = state.blockchain.read().await;
    let storage = state.storage.read().await;
    let chain_state = &blockchain.state;
    let effective_quorum = chain_state.oracle_quorum.max(1);

    let attestation_context = |commit_id: &str| {
        let set = chain_state.attestations.get(commit_id);
        let approvals = set.map(|s| s.approvals()).unwrap_or(0);
        let rejections = set
            .map(|s| s.by_oracle.values().filter(|r| !r.approved).count())
            .unwrap_or(0);
        let mut attesting: std::collections::BTreeSet<String> = set
            .map(|s| s.by_oracle.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(pending) = mempool_attesters_by_commit.get(commit_id) {
            attesting.extend(pending.iter().cloned());
        }
        let has_quorum = set
            .map(|s| s.has_quorum(effective_quorum))
            .unwrap_or(false);
        (
            approvals,
            rejections,
            attesting.into_iter().collect(),
            has_quorum,
        )
    };

    let mut observations: Vec<PendingAttestationObservation> = Vec::new();
    let mut ready_to_mine: Vec<PendingAttestationObservation> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut unrecoverable: Vec<String> = Vec::new();

    // Lane 1: unmined reveals. The reveal payload here is authoritative — it
    // is the exact bytes the chain will apply.
    for tx in &mempool_reveals {
        let Some(payload) = tx.parse_reveal_payload() else {
            continue;
        };
        if payload.commit_id.is_empty() || !seen.insert(payload.commit_id.clone()) {
            continue;
        }
        let (approvals, rejections, attesting, has_quorum) =
            attestation_context(&payload.commit_id);
        let registered_station = chain_state.station_registry.get(&payload.station_id);
        let provenance_class = registered_station
            .map(|s| s.provenance_class)
            .unwrap_or_default();
        let elevation_meters = registered_station.and_then(|s| s.location.elevation_meters);
        let station_location =
            registered_station.and_then(|s| PendingStationLocation::coarse(&s.location));
        let observation = PendingAttestationObservation {
            commit_id: payload.commit_id.clone(),
            station_id: payload.station_id.clone(),
            timestamp: payload.timestamp,
            nonce: payload.nonce.clone(),
            contribution_type: payload.contribution_type.clone(),
            data: payload.data.clone(),
            source: "mempool".to_string(),
            revealed_at_height: None,
            provenance_class,
            elevation_meters,
            station_location,
            approvals,
            rejections,
            attesting_oracles: attesting,
            has_quorum,
            reward_recorded: chain_state
                .contribution_reward_records
                .contains_key(&payload.commit_id),
            reward_finalized: chain_state
                .finalized_contribution_rewards
                .contains(&payload.commit_id),
        };
        if has_quorum {
            ready_to_mine.push(observation);
        } else {
            observations.push(observation);
        }
    }

    // Lane 2: reveals already mined without quorum. State keeps only reveal
    // metadata (`RevealRecord` has no data/nonce), so the preimage is
    // recovered from the reveal's own block — in-memory window first, durable
    // archive as the fallback (`mined_reveal_payload`). Only a block missing
    // from BOTH is reported unrecoverable.
    let mut mined_pending: Vec<(&String, u64)> = chain_state
        .pending_reveals
        .iter()
        .map(|(commit_id, record)| (commit_id, record.revealed_at))
        .collect();
    mined_pending.sort();
    for (commit_id, revealed_at) in mined_pending {
        if seen.contains(commit_id)
            || chain_state.finalized_contribution_rewards.contains(commit_id)
        {
            continue;
        }
        let (approvals, rejections, attesting, has_quorum) = attestation_context(commit_id);
        if has_quorum {
            continue;
        }
        let Some(payload) = mined_reveal_payload(&blockchain, &storage, commit_id, revealed_at)
        else {
            unrecoverable.push(commit_id.clone());
            continue;
        };
        seen.insert(commit_id.clone());
        let registered_station = chain_state.station_registry.get(&payload.station_id);
        let provenance_class = registered_station
            .map(|s| s.provenance_class)
            .unwrap_or_default();
        let elevation_meters = registered_station.and_then(|s| s.location.elevation_meters);
        let station_location =
            registered_station.and_then(|s| PendingStationLocation::coarse(&s.location));
        observations.push(PendingAttestationObservation {
            commit_id: commit_id.clone(),
            station_id: payload.station_id.clone(),
            timestamp: payload.timestamp,
            nonce: payload.nonce.clone(),
            contribution_type: payload.contribution_type.clone(),
            data: payload.data.clone(),
            source: "block".to_string(),
            revealed_at_height: Some(revealed_at),
            provenance_class,
            elevation_meters,
            station_location,
            approvals,
            rejections,
            attesting_oracles: attesting,
            has_quorum,
            reward_recorded: chain_state
                .contribution_reward_records
                .contains_key(commit_id),
            reward_finalized: false,
        });
    }

    // Deterministic order: oldest reading first (attest in arrival order),
    // commit id as tie-breaker; then the page cap.
    observations.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.commit_id.cmp(&b.commit_id))
    });
    observations.truncate(limit);
    ready_to_mine.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.commit_id.cmp(&b.commit_id))
    });
    ready_to_mine.truncate(limit);

    let mut oracle_set: Vec<String> = chain_state.oracle_set.iter().cloned().collect();
    oracle_set.sort();

    HttpResponse::Ok().json(PendingAttestationsResponse {
        chain_id: signing_chain_id(),
        network_id: signing_network_id(),
        height: blockchain.height,
        oracle_set,
        oracle_quorum: chain_state.oracle_quorum,
        effective_quorum,
        observations,
        ready_to_mine,
        unrecoverable,
        active_features: active_feature_names(blockchain.height.saturating_add(1)),
    })
}

/// Resolve the reveal payload for a mined commit: in-memory window first,
/// durable block archive second.
///
/// The archive fallback is the fix for the twice-solved eviction class this
/// route initially re-introduced (#2746 review, HIGH): `block_at_height` is a
/// bounded in-memory ring of `configured_max_in_memory_blocks()` entries
/// (default 200), while `pending_reveals` entries live until settled — so
/// past the window, the deferred lane's backstop expired on a shorter clock
/// than the failures it exists to catch. Same mistake, same repair as
/// `late_quorum.rs` (whose module doc narrates the identical prior bug) and
/// the fork-point fallback in `chain.rs::execute_reorg`; the durable read
/// reuses their `Storage::load_blocks_range`, not a third mechanism.
///
/// An archive read error is treated as unrecoverable-for-now rather than a
/// route failure: this is a polling surface, and the attester retries on the
/// next cycle.
fn mined_reveal_payload(
    blockchain: &wrfcoin_native_chain::blockchain::chain::Blockchain,
    storage: &wrfcoin_native_chain::storage::Storage,
    commit_id: &str,
    revealed_at: u64,
) -> Option<wrfcoin_native_chain::blockchain::transaction::RevealPayload> {
    let from_block = |block: &wrfcoin_native_chain::blockchain::block::Block| {
        block
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::ContributionReveal)
            .filter_map(|tx| tx.parse_reveal_payload())
            .find(|p| p.commit_id == commit_id)
    };
    if let Some(block) = blockchain.block_at_height(revealed_at) {
        if let Some(payload) = from_block(block) {
            return Some(payload);
        }
        // The height is in memory but carries no matching reveal: the archive
        // holds the same block, so falling through cannot help.
        return None;
    }
    match storage.load_blocks_range(revealed_at, revealed_at) {
        Ok(blocks) => blocks.first().and_then(from_block),
        Err(err) => {
            log::warn!(
                "[oracle/pending] durable archive read failed for reveal block {revealed_at} \
                 (commit {commit_id}): {err}"
            );
            None
        },
    }
}

#[cfg(test)]
mod pending_attestation_tests {
    use super::*;
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_native_chain::blockchain::chain::Blockchain;
    use wrfcoin_native_chain::crypto::hash::Hash;
    use wrfcoin_native_chain::storage::Storage;
    use wrfcoin_shared_protocol::blockchain::transaction::{
        RevealPayload, Transaction, TransactionInput, TransactionOutput,
    };

    fn reveal_tx(commit_id: &str) -> Transaction {
        let payload = RevealPayload {
            commit_id: commit_id.to_string(),
            station_id: "WX-EVICT-1".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            data: serde_json::json!({"temperature_c": 20.0}),
            nonce: "n-evict".to_string(),
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
            quality_tier: None,
        };
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: Hash::default(),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: "wrf-evict".to_string(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::ContributionReveal;
        tx.data = Some(serde_json::to_string(&payload).expect("serialize reveal"));
        tx.set_id();
        tx
    }

    /// The #2746 HIGH: a reveal whose block has aged out of the in-memory
    /// window must still be servable from the durable archive. The in-memory
    /// miss is asserted explicitly — that assertion IS the red control, since
    /// `block_at_height(...) == None` is precisely the condition under which
    /// the pre-fix code declared the commit unrecoverable.
    #[tokio::test]
    async fn an_evicted_reveal_block_is_recovered_from_the_durable_archive() {
        let commit_id = "evicted-commit-1";
        let tmp = std::env::temp_dir().join(format!(
            "wrfcoin-evict-proof-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp storage dir");
        let mut storage =
            Storage::open(&tmp.join("blocks.redb"), 1_000).expect("open temp storage");

        // A window of 5 with the reveal mined at height 1, then built past —
        // the same reproduction shape as the genesis-metadata eviction test
        // in main.rs.
        let mut blockchain = Blockchain::with_capacity(5);
        for height in 1..=10u64 {
            let prev = if height == 1 { Hash::default() } else { blockchain.tip };
            let txs = if height == 1 { vec![reveal_tx(commit_id)] } else { vec![] };
            let mut block = Block::new(3, prev, txs, 1, height);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).expect("add block");
            storage
                .persist_block(
                    block,
                    vec![],
                    storage
                        .checkpoint_state_bytes(height, &blockchain.state)
                        .expect("checkpoint bytes"),
                )
                .await
                .expect("persist block");
        }

        // RED-control precondition: the reveal's block is GONE from memory.
        // Under the pre-fix code this exact condition rendered the commit
        // permanently unrecoverable.
        assert!(
            blockchain.block_at_height(1).is_none(),
            "height 1 must have been evicted for this test to prove anything"
        );

        let payload = mined_reveal_payload(&blockchain, &storage, commit_id, 1)
            .expect("the durable archive must recover the evicted reveal");
        assert_eq!(payload.commit_id, commit_id);
        assert_eq!(payload.nonce, "n-evict");
        assert_eq!(payload.station_id, "WX-EVICT-1");

        // A commit whose block exists nowhere is still honestly unrecoverable.
        assert!(
            mined_reveal_payload(&blockchain, &storage, "no-such-commit", 999).is_none(),
            "a height beyond the archive must not fabricate a payload"
        );

        drop(storage);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// In-memory hit still works and takes priority (no archive read needed).
    #[tokio::test]
    async fn an_in_window_reveal_block_is_served_from_memory() {
        let commit_id = "in-window-commit";
        let tmp = std::env::temp_dir().join(format!(
            "wrfcoin-inwin-proof-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp storage dir");
        let storage =
            Storage::open(&tmp.join("blocks.redb"), 1_000).expect("open temp storage");

        let mut blockchain = Blockchain::with_capacity(5);
        let mut block = Block::new(3, Hash::default(), vec![reveal_tx(commit_id)], 1, 1);
        let hash = block.calculate_hash();
        block.set_hash(hash);
        blockchain.add_block(block).expect("add block");
        // Deliberately NOT persisted: a memory hit must not need the archive.

        let payload = mined_reveal_payload(&blockchain, &storage, commit_id, 1)
            .expect("in-window block must serve the payload");
        assert_eq!(payload.commit_id, commit_id);

        drop(storage);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn pending_entry(
        station_location: Option<PendingStationLocation>,
    ) -> PendingAttestationObservation {
        PendingAttestationObservation {
            commit_id: "c".repeat(64),
            station_id: "PWS-3240".to_string(),
            timestamp: 1_700_000_000,
            nonce: "n".to_string(),
            contribution_type: "contribution".to_string(),
            data: serde_json::json!({"temperature_c": 20.0}),
            source: "mempool".to_string(),
            revealed_at_height: None,
            provenance_class: Default::default(),
            elevation_meters: None,
            station_location,
            approvals: 0,
            rejections: 0,
            attesting_oracles: Vec::new(),
            has_quorum: false,
            reward_recorded: false,
            reward_finalized: false,
        }
    }

    /// #3240: a registered station's location reaches the pending surface
    /// only COARSENED (0.01°), and an unregistered station's entry omits the
    /// key entirely (older attesters and `deny_unknown_fields`-free readers
    /// see exactly the pre-change shape).
    #[test]
    fn pending_station_location_is_coarse_and_omitted_when_unregistered_3240() {
        let precise = wrfcoin_shared_protocol::station_registry::StationLocation {
            latitude: 46.786_712,
            longitude: -92.100_549,
            elevation_meters: Some(210.0),
        };
        let coarse = PendingStationLocation::coarse(&precise).expect("finite location");
        assert_eq!(
            coarse,
            PendingStationLocation {
                latitude: 46.79,
                longitude: -92.1,
            }
        );

        let present = serde_json::to_value(pending_entry(Some(coarse))).unwrap();
        assert_eq!(present["station_location"]["latitude"], 46.79);
        assert_eq!(present["station_location"]["longitude"], -92.1);
        let wire = present.to_string();
        assert!(
            !wire.contains("46.7867") && !wire.contains("92.1005"),
            "exact coordinates must not reach the pending surface: {wire}"
        );

        let absent = serde_json::to_value(pending_entry(None)).unwrap();
        assert!(
            absent.get("station_location").is_none(),
            "an unregistered station must omit the key, not serialize null"
        );

        let non_finite = wrfcoin_shared_protocol::station_registry::StationLocation {
            latitude: f64::NAN,
            longitude: -92.1,
            elevation_meters: None,
        };
        assert_eq!(PendingStationLocation::coarse(&non_finite), None);
    }
}
