//! Station-registration submission surface (#2537).
//!
//! #2569 landed the consensus half of the production registration path — the
//! `StationRegistration` transaction type, its governance + owner
//! authorization, and the `ApplyStationRegistration` transition — but shipped
//! **no way to submit one**. That gap was called out in the #2569 PR body and
//! is why it said `Refs #2537` rather than `Closes`. An operator holding a
//! correctly signed registration had no route to hand it to a node, so the
//! path that makes contributions reward-eligible could not actually be used.
//!
//! This is the deliberate analogue of
//! [`super::oracle::submit_oracle_registrations`], not a new mechanism:
//!
//! - Transactions are admitted **verbatim**. They are not rebuilt through
//!   `enqueue_protocol_transaction`, which stamps a fresh wall-clock timestamp
//!   and a nanosecond-seeded input txid — that would give each node a different
//!   transaction id for the same registration, and therefore a different block.
//! - The route puts *transactions in the mempool and nothing else*, so a
//!   station enters consensus the same way on every node. It is categorically
//!   not the #1775 out-of-band admin shape, which mutates one node's state and
//!   forks it.
//! - Authorization is re-verified here even though consensus verifies it again:
//!   an unauthorized registration should be refused while it is still fixable,
//!   not silently dropped at assembly.
//!
//! What it deliberately does NOT do: the oracle route enforces the #2496
//! block-1 agreement rule, because every genesis `OracleRegistration` must
//! carry the identical absolute set. Station registrations carry no set — each
//! one is independent — so there is no cross-transaction agreement to enforce
//! and no such check here.

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use wrfcoin_native_chain::blockchain::chain::MAX_REORG_DEPTH;
use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_native_chain::crypto::domain_signing::{signing_chain_id, signing_network_id};
use wrfcoin_shared_protocol::station_registry::validate_station_id;

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::security::api_auth::ApiRole;

/// Submission window, mirroring the oracle ceremony route. Block assembly
/// drops transactions outside `filter_by_submission_timing` and reports it only
/// as a `warn!`, so a stale artifact would otherwise be accepted here, silently
/// dropped at assembly, and the operator would be left holding a 201 for a
/// station that never registered.
const DEFAULT_SUBMISSION_TIME_WINDOW_SECS: u64 = 300;

#[derive(Debug, Serialize, Deserialize)]
pub struct StationRegistrationSubmitRequest {
    /// Declared explicitly for the same reason the oracle route declares it:
    /// the governance signature binds the chain identity, and the node resolves
    /// its own from the environment with a silent default. Without the declared
    /// pair, an artifact built under a different chain identity fails
    /// authorization with no way to tell that apart from a bad key.
    pub chain_id: String,
    pub network_id: String,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdmittedStation {
    pub station_id: String,
    pub owner_address: String,
    pub provenance_class: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StationRegistrationSubmitResponse {
    pub accepted: usize,
    pub transaction_ids: Vec<String>,
    pub stations: Vec<AdmittedStation>,
}

/// Canonical payout binding read directly from consensus state (#2780).
///
/// `station_registry` currently retains the latest station record only. It
/// does not retain the registration transaction id or inclusion height, so
/// this response deliberately omits those unavailable historical fields
/// rather than returning placeholders that look like provenance.
#[derive(Debug, Serialize, Deserialize)]
pub struct CanonicalStationRegistrationResponse {
    pub station_id: String,
    pub owner_address: String,
    pub provenance_class: String,
    pub is_active: bool,
    /// The registered `location.elevation_meters` (#3285), `None` when the
    /// station was registered without one. Collectors read it to compute
    /// `pressure_sea_level_hpa` from the same elevation the oracle attester
    /// recomputes against; a station with none submits no sea-level key.
    #[serde(default)]
    // determinism-ok: read-only API echo of the registry's Option<f64> elevation; nothing here is consensus arithmetic
    pub elevation_meters: Option<f64>,
    /// Tip height whose canonical state and root were read under one lock.
    pub observed_at_height: u64,
    /// Monotonic BFT finality when available, otherwise the probabilistic
    /// `tip - MAX_REORG_DEPTH` settlement floor used by node monitoring.
    pub settled_height: u64,
    pub state_root_hex: String,
}

/// Deterministic record-to-context boundary used only by the real handler's
/// concurrency regression. The production build contains neither this state
/// nor either barrier.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct CanonicalStationReadTestSeam {
    station_id: String,
    pub(crate) record_read: std::sync::Arc<tokio::sync::Barrier>,
    pub(crate) resume_context: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
static CANONICAL_STATION_READ_TEST_SEAM: std::sync::LazyLock<
    std::sync::Mutex<Option<CanonicalStationReadTestSeam>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

#[cfg(test)]
fn canonical_station_read_test_seam_lock(
) -> std::sync::MutexGuard<'static, Option<CanonicalStationReadTestSeam>> {
    match CANONICAL_STATION_READ_TEST_SEAM.lock() {
        Ok(installed) => installed,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
pub(crate) fn install_canonical_station_read_test_seam(
    station_id: &str,
) -> CanonicalStationReadTestSeam {
    let seam = CanonicalStationReadTestSeam {
        station_id: station_id.to_string(),
        record_read: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
        resume_context: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
    };
    *canonical_station_read_test_seam_lock() = Some(seam.clone());
    seam
}

#[cfg(test)]
pub(crate) fn clear_canonical_station_read_test_seam(station_id: &str) {
    let mut installed = canonical_station_read_test_seam_lock();
    if installed
        .as_ref()
        .is_some_and(|seam| seam.station_id == station_id)
    {
        *installed = None;
    }
}

#[cfg(test)]
async fn pause_after_canonical_station_record_read(station_id: &str) {
    let seam = canonical_station_read_test_seam_lock()
        .as_ref()
        .filter(|seam| seam.station_id == station_id)
        .cloned();
    if let Some(seam) = seam {
        seam.record_read.wait().await;
        seam.resume_context.wait().await;
    }
}

/// Admit signed station registrations to the mempool (#2537).
pub async fn submit_station_registrations(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<StationRegistrationSubmitRequest>,
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

    let node_chain_id = signing_chain_id();
    let node_network_id = signing_network_id();
    if request.chain_id != node_chain_id || request.network_id != node_network_id {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!(
                "artifact chain identity ({}/{}) does not match this node ({}/{}); \
                 the governance signature binds the chain identity, so these \
                 registrations cannot authorize here",
                request.chain_id, request.network_id, node_chain_id, node_network_id
            ),
        );
    }

    // One clock read for the whole batch: sampling per transaction could accept
    // the first and reject the last of an otherwise identical artifact.
    let now_secs = chrono::Utc::now().timestamp(); // determinism-ok: admission-time freshness gate at the API boundary; decides accept/reject only, never alters the bytes that reach consensus
    let submission_window = DEFAULT_SUBMISSION_TIME_WINDOW_SECS;
    let admission_window = submission_window / 2;

    let mut transaction_ids: Vec<String> = Vec::new();
    let mut stations: Vec<AdmittedStation> = Vec::new();

    // #2742: admission gates on the height the registration would MINE at
    // (tip + 1), so a registration that consensus would refuse post-activation
    // is rejected here with a diagnosis instead of being gossiped and
    // silently filtered by every producer/applier.
    let admission_height = { state.blockchain.read().await.height + 1 };

    // Validate the whole batch before admitting any of it.
    for (index, tx) in request.transactions.iter().enumerate() {
        if tx.tx_type != TransactionType::StationRegistration {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] is {:?}, expected StationRegistration",
                    tx.tx_type
                ),
            );
        }
        let Some(payload) = tx.parse_station_registration() else {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] does not carry a parseable station registration"
                ),
            );
        };
        // Fail-closed, and the reason this matters more than for most routes:
        // `provenance_class` is the field that decides whether this station's
        // contributions can ever earn. An unauthorized `Real` admitted here
        // would be rejected by consensus, but only after it had been gossiped.
        if !tx.verify_station_registration_authorization_at_height(admission_height) {
            // Name the binding failure specifically when it is the one check
            // that failed — "not authorized" alone would send an operator
            // hunting through signatures that are actually fine (#2744's
            // attribution lesson).
            let detail = if tx.verify_station_registration_authorization()
                && !payload.owner_address_is_bound_to_owner_pubkey()
            {
                format!(
                    "transactions[{index}] (station {}) has valid signatures but its \
                     owner_address is not derived from owner_pubkey (#2742): from this \
                     chain height the ownership proof must bind the reward address. \
                     Derive the address from the owner key (PublicKey::to_address) and \
                     re-sign, or use `wrfcoin-cli station build-registration`, which now \
                     refuses this mismatch client-side",
                    payload.station_id
                )
            } else {
                format!(
                    "transactions[{index}] (station {}) is not authorized: it needs both \
                     an owner possession proof and a signature from an allowlisted \
                     oracle-governance key over the whole record",
                    payload.station_id
                )
            };
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "UNPROCESSABLE_ENTITY", detail);
        }

        // Registration format vs `StationRegistrationNonce`: v1 only before
        // activation, v2 (signed expected nonce, checked against the ledger)
        // only from it.
        {
            let nonce_active = wrfcoin_shared_protocol::crypto::chain_binding::feature_active(
                wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::StationRegistrationNonce,
                admission_height,
            );
            if let Some(refusal) = registration_nonce_refusal(
                &state.blockchain.read().await.state,
                &payload,
                admission_height,
                nonce_active,
                index,
            ) {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    refusal.code,
                    refusal.message,
                );
            }
        }

        // #3345 (admission policy, not consensus): once `StationOwnerConsent` is
        // active at the height this would mine at, a registration that names a
        // DIFFERENT owner for an already-registered station is an inert no-op
        // in a block. Refuse it here, while it is still fixable.
        let consent_active = wrfcoin_shared_protocol::crypto::chain_binding::feature_active(
            wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::StationOwnerConsent,
            admission_height,
        );
        if let Some(refusal) = registration_owner_change_refusal(
            &state.blockchain.read().await.state,
            &payload,
            consent_active,
            index,
        ) {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                refusal.code,
                refusal.message,
            );
        }

        let tx_time = tx.timestamp.timestamp();
        let delta = now_secs.abs_diff(tx_time);
        if delta > admission_window {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!(
                    "transactions[{index}] (station {}) has a submitted_at {delta}s from now, \
                     beyond the {admission_window}s admission margin (half the \
                     {submission_window}s window block assembly enforces). Assembly measures \
                     against a block timestamp that only ever moves forward, so this batch \
                     could still be dropped after a 201. Rebuild with a current --submitted-at",
                    payload.station_id
                ),
            );
        }

        // The transaction id folds the pinned timestamp and the full payload.
        // An id that is absent or does not recompute means the artifact was
        // edited after signing, so this node would put different bytes in the
        // block than its peers.
        match tx.id.as_ref() {
            Some(id) if *id == tx.calculate_hash() => transaction_ids.push(id.to_hex()),
            Some(_) => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    format!(
                        "transactions[{index}] (station {}) has an id that does not match \
                         its contents; the artifact was modified after signing",
                        // secret-leak-ok: station_id is a public identifier in an error response
                        payload.station_id
                    ),
                );
            },
            None => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    format!(
                        "transactions[{index}] (station {}) carries no transaction id",
                        payload.station_id
                    ),
                );
            },
        }

        stations.push(AdmittedStation {
            station_id: payload.station_id.clone(),
            owner_address: payload.owner_address.clone(),
            provenance_class: payload.provenance_class.as_str().to_string(),
        });
    }

    {
        let mut mempool = state.mempool.write().await;
        // Roll back on failure, for the same reason the oracle route does:
        // `add_transaction` can reject item N (duplicate, capacity, fee floor)
        // after 1..N-1 are already in, leaving a partially admitted artifact.
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
                        "transactions[{index}] rejected by mempool: {err}; the {} already \
                         admitted were rolled back",
                        admitted.len()
                    ),
                );
            }
            if let Some(id) = tx.id.as_ref() {
                admitted.push(*id);
            }
        }
    }

    HttpResponse::Created().json(StationRegistrationSubmitResponse {
        accepted: transaction_ids.len(),
        transaction_ids,
        stations,
    })
}

/// Read the canonical station owner/provenance binding from chain state.
pub async fn get_station_registration(
    req: HttpRequest,
    state: web::Data<AppState>,
    station_id: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }

    let station_id = station_id.into_inner();
    if let Err(error) = validate_station_id(&station_id) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            error.to_string(),
        );
    }

    let blockchain = state.blockchain.read().await;
    let Some(station) = blockchain.state.station_registry.get(&station_id) else {
        return envelope_error(
            &req,
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("station is not registered: {station_id}"),
        );
    };

    #[cfg(test)]
    pause_after_canonical_station_record_read(&station_id).await;

    let probabilistic_floor = blockchain.height.saturating_sub(MAX_REORG_DEPTH);
    let settled_height = probabilistic_floor.max(blockchain.finalized_height());

    HttpResponse::Ok().json(CanonicalStationRegistrationResponse {
        station_id: station.station_id.clone(),
        owner_address: station.owner_address.clone(),
        provenance_class: station.provenance_class.as_str().to_string(),
        is_active: station.is_active,
        elevation_meters: station.location.elevation_meters,
        observed_at_height: blockchain.height,
        settled_height,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

// ---------------------------------------------------------------------------
// Owner-ledger transactions (#3345 / #3347)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AdmittedOwnerStepResponse {
    pub station_id: String,
    /// The `tx_kind` (`station_owner_transfer_v1`, ...).
    pub kind: String,
    /// The owner-ledger nonce the tx carries (`recovery_nonce` for recovery).
    pub nonce: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StationOwnerTxSubmitResponse {
    pub accepted: usize,
    pub transaction_ids: Vec<String>,
    pub steps: Vec<AdmittedOwnerStepResponse>,
    /// The height the txs were checked at (`tip + 1`).
    pub admission_height: u64,
}

/// A named admission refusal for the owner-tx submit route.
#[derive(Debug)]
pub(crate) struct OwnerBatchRefusal {
    pub code: &'static str,
    pub message: String,
}

/// Validate a whole owner-tx batch at `admission_height` against `state`:
/// the precheck, one tx per station, timestamp freshness and id integrity.
/// Pure (no clock, no globals): the caller supplies `now_secs` and the two
/// governing feature states, so every refusal code can be exercised directly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_owner_batch(
    state: &wrfcoin_native_chain::blockchain::state::State,
    transactions: &[Transaction],
    admission_height: u64,
    chain_id: &str,
    network_id: &str,
    now_secs: i64,
    consent: bool,
    actions: bool,
) -> Result<(Vec<String>, Vec<AdmittedOwnerStepResponse>), OwnerBatchRefusal> {
    validate_owner_batch_with_pending(
        state,
        transactions,
        admission_height,
        chain_id,
        network_id,
        now_secs,
        consent,
        actions,
        &std::collections::BTreeSet::new(),
    )
}

/// The stations that already have a `recovery_execute` queued in `mempool`.
pub(crate) fn pending_execute_stations(
    mempool: &wrfcoin_native_chain::mempool::pool::Mempool,
) -> std::collections::BTreeSet<String> {
    mempool
        .get_transactions_by_type(TransactionType::StationRegistration, usize::MAX)
        .iter()
        .filter_map(|tx| tx.recovery_execute_station())
        .collect()
}

/// [`validate_owner_batch`] plus the stations that already have a
/// `recovery_execute` pending in the mempool. A `recovery_execute` is unsigned
/// and permissionless, so anyone could queue many valid-looking copies in the
/// ripe window; at most ONE per station is admitted, and a later one is refused
/// (`OWNER_TX_EXECUTE_ALREADY_PENDING`) until the first is mined or evicted.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_owner_batch_with_pending(
    state: &wrfcoin_native_chain::blockchain::state::State,
    transactions: &[Transaction],
    admission_height: u64,
    chain_id: &str,
    network_id: &str,
    now_secs: i64,
    consent: bool,
    actions: bool,
    pending_executes: &std::collections::BTreeSet<String>,
) -> Result<(Vec<String>, Vec<AdmittedOwnerStepResponse>), OwnerBatchRefusal> {
    let admission_window = DEFAULT_SUBMISSION_TIME_WINDOW_SECS / 2;
    let mut transaction_ids: Vec<String> = Vec::new();
    let mut steps: Vec<AdmittedOwnerStepResponse> = Vec::new();
    let mut seen_stations: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (index, tx) in transactions.iter().enumerate() {
        let step = state
            .precheck_station_owner_tx_with_features(
                tx,
                admission_height,
                chain_id,
                network_id,
                consent,
                actions,
            )
            .map_err(|rejection| OwnerBatchRefusal {
                code: rejection.code(),
                message: format!(
                    "transactions[{index}] refused at height {admission_height}: {rejection:?}"
                ),
            })?;
        if tx.recovery_execute_station().is_some() && pending_executes.contains(&step.station_id) {
            return Err(OwnerBatchRefusal {
                code: "OWNER_TX_EXECUTE_ALREADY_PENDING",
                message: format!(
                    "transactions[{index}]: a recovery_execute for station {} is already pending \
                     in the mempool; it executes the recovery for everyone, so a second is \
                     refused until the first is mined or evicted",
                    step.station_id
                ),
            });
        }
        if !seen_stations.insert(step.station_id.clone()) {
            return Err(OwnerBatchRefusal {
                code: "OWNER_TX_DUPLICATE_STATION_IN_BATCH",
                message: format!(
                    "transactions[{index}] is a second owner tx for station {}; each consumes \
                     the same per-station nonce, so submit one per batch",
                    step.station_id
                ),
            });
        }
        let delta = now_secs.abs_diff(tx.timestamp.timestamp());
        if delta > admission_window {
            return Err(OwnerBatchRefusal {
                code: "OWNER_TX_STALE_TIMESTAMP",
                message: format!(
                    "transactions[{index}] has a timestamp {delta}s from now, beyond the \
                     {admission_window}s admission margin; rebuild it with a current timestamp"
                ),
            });
        }
        match tx.id.as_ref() {
            Some(id) if *id == tx.calculate_hash() => transaction_ids.push(id.to_hex()),
            Some(_) => {
                return Err(OwnerBatchRefusal {
                    code: "OWNER_TX_ID_MISMATCH",
                    message: format!(
                        "transactions[{index}] has an id that does not match its contents"
                    ),
                });
            },
            None => {
                return Err(OwnerBatchRefusal {
                    code: "OWNER_TX_ID_MISSING",
                    message: format!("transactions[{index}] carries no transaction id"),
                });
            },
        }
        // The envelope must be EXACTLY what the node would build for this
        // payload (fee 0, the canonical output, the input txid derived from the
        // payload). A crafted input txid defeats the mempool's double-spend
        // dedupe and a non-zero fee skews its ordering. Only the timestamp (within
        // the window) is the sender's to choose.
        if !tx.is_canonical_station_owner_envelope() {
            return Err(OwnerBatchRefusal {
                code: "OWNER_TX_ENVELOPE_NONCANONICAL",
                message: format!(
                    "transactions[{index}] is not the canonical owner-tx envelope for its \
                     payload (fee 0, one zero-value output to the owner, the input txid derived \
                     from the payload); send the `payloads` form or rebuild it with the CLI"
                ),
            });
        }
        steps.push(AdmittedOwnerStepResponse {
            station_id: step.station_id,
            kind: step.kind.to_string(),
            nonce: step.nonce,
        });
    }
    Ok((transaction_ids, steps))
}

/// Admission policy (B): does this registration, mined at `admission_height`,
/// change the registered owner of an existing station while
/// `station_owner_consent` is active? Then refuse it with a named code while
/// it is still fixable.
pub(crate) fn registration_owner_change_refusal(
    state: &wrfcoin_native_chain::blockchain::state::State,
    payload: &wrfcoin_shared_protocol::blockchain::transaction::StationRegistrationPayload,
    consent_active: bool,
    index: usize,
) -> Option<OwnerBatchRefusal> {
    if !consent_active {
        return None;
    }
    let station = Transaction::station_from_registration(payload);
    state
        .station_registration_changes_owner(&station)
        .then(|| OwnerBatchRefusal {
            code: "STATION_OWNER_CHANGE_REQUIRES_CONSENT",
            message: format!(
                "transactions[{index}] (station {}) names a different owner than the registered \
                 one. Since station_owner_consent is active, only the current owner's signed \
                 transfer (or a time-locked recovery) can change the owner; a registration cannot",
                payload.station_id
            ),
        })
}

/// One payload for the node to wrap (payload form).
#[derive(Debug, Serialize, Deserialize)]
pub struct OwnerPayloadItem {
    /// The payload's `tx_kind` (`station_owner_transfer_v1`, ...). Must match
    /// the `tx_kind` inside `payload_json`.
    pub kind: String,
    /// The payload's exact JSON TEXT, as a string. The node forwards these
    /// bytes unchanged as the transaction's `data`; the signatures are over the
    /// length-prefixed preimage rebuilt from the parsed fields, so whitespace
    /// and key order do not affect verification.
    pub payload_json: String,
}

/// Request body of the owner-tx submit route: EXACTLY ONE of the full
/// `transactions` envelope form (the CLI's artifact) or the `payloads` form
/// (phone and backend).
#[derive(Debug, Serialize, Deserialize)]
pub struct StationOwnerSubmitRequest {
    pub chain_id: String,
    pub network_id: String,
    #[serde(default)]
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub payloads: Vec<OwnerPayloadItem>,
}

/// Resolve the request's batch into transactions: the envelope form as given,
/// or the payload form wrapped by the node with a node-chosen timestamp.
pub(crate) fn build_owner_batch_transactions(
    transactions: &[Transaction],
    payloads: &[OwnerPayloadItem],
    now_secs: i64,
) -> Result<Vec<Transaction>, OwnerBatchRefusal> {
    match (transactions.is_empty(), payloads.is_empty()) {
        (true, true) => Err(OwnerBatchRefusal {
            code: "OWNER_TX_BODY_EMPTY",
            message: "send either `transactions` (envelopes) or `payloads`; both are empty"
                .to_string(),
        }),
        (false, false) => Err(OwnerBatchRefusal {
            code: "OWNER_TX_BODY_FORM",
            message: "send EITHER `transactions` OR `payloads`, not both".to_string(),
        }),
        (false, true) => Ok(transactions.to_vec()),
        (true, false) => payloads
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let value: serde_json::Value =
                    serde_json::from_str(&item.payload_json).map_err(|e| OwnerBatchRefusal {
                        code: "OWNER_TX_MALFORMED",
                        message: format!("payloads[{index}].payload_json is not valid JSON: {e}"),
                    })?;
                let tx_kind = value.get("tx_kind").and_then(serde_json::Value::as_str);
                if tx_kind != Some(item.kind.as_str()) {
                    return Err(OwnerBatchRefusal {
                        code: "OWNER_TX_KIND_MISMATCH",
                        message: format!(
                            "payloads[{index}].kind {:?} does not match the payload's tx_kind {:?}",
                            item.kind, tx_kind
                        ),
                    });
                }
                // The ONE canonical envelope (shared with the CLI and with the
                // admission check that every envelope must equal it).
                Transaction::canonical_station_owner_tx(&item.payload_json, now_secs)
                .ok_or_else(|| OwnerBatchRefusal {
                    code: "OWNER_TX_MALFORMED",
                    message: format!("payloads[{index}]: node clock out of range"),
                })
            })
            .collect(),
    }
}

/// Insert a validated batch into the mempool, all or nothing, WHILE HOLDING its
/// write lock (the caller passes the locked mempool). The
/// one-pending-`recovery_execute`-per-station rule is re-checked here, against
/// the mempool as it is NOW: the earlier validation read the mempool before this
/// lock was taken, so two concurrent submissions could both have passed it.
pub(crate) fn admit_owner_batch_into_mempool(
    mempool: &mut wrfcoin_native_chain::mempool::pool::Mempool,
    transactions: &[Transaction],
) -> Result<(), OwnerBatchRefusal> {
    let mut admitted: Vec<wrfcoin_native_chain::crypto::hash::Hash> = Vec::new();
    for (index, tx) in transactions.iter().enumerate() {
        if let Some(station) = tx.recovery_execute_station() {
            if pending_execute_stations(mempool).contains(&station) {
                for id in &admitted {
                    mempool.remove_transaction(id);
                }
                return Err(OwnerBatchRefusal {
                    code: "OWNER_TX_EXECUTE_ALREADY_PENDING",
                    message: format!(
                        "transactions[{index}]: a recovery_execute for station {station} is \
                         already pending in the mempool"
                    ),
                });
            }
        }
        if let Err(err) = mempool.add_transaction(tx.clone()) {
            for id in &admitted {
                mempool.remove_transaction(id);
            }
            return Err(OwnerBatchRefusal {
                code: "MEMPOOL_REJECTED",
                message: format!(
                    "transactions[{index}] rejected by mempool: {err}; the {} already admitted \
                     were rolled back",
                    admitted.len()
                ),
            });
        }
        if let Some(id) = tx.id.as_ref() {
            admitted.push(*id);
        }
    }
    Ok(())
}

/// Admission policy for the registration FORMAT (`StationRegistrationNonce`):
/// before activation only v1 is admissible; from activation only v2, whose
/// signed `expected_nonce` must equal the station's current owner-ledger nonce.
/// Refused here with a named code while still fixable, instead of being mined
/// as an inert no-op. Pure: the caller supplies the activation state.
pub(crate) fn registration_nonce_refusal(
    state: &wrfcoin_native_chain::blockchain::state::State,
    payload: &wrfcoin_shared_protocol::blockchain::transaction::StationRegistrationPayload,
    admission_height: u64,
    nonce_active: bool,
    index: usize,
) -> Option<OwnerBatchRefusal> {
    if payload.is_v2() != nonce_active {
        let (code, why) = if payload.is_v2() {
            (
                "STATION_REGISTRATION_NONCE_FEATURE_INACTIVE",
                "a v2 (nonce-bound) registration is not valid until station_registration_nonce is active",
            )
        } else {
            (
                "STATION_REGISTRATION_V1_REFUSED",
                "station_registration_nonce is active, so only v2 registrations (with a signed expected_nonce) apply",
            )
        };
        return Some(OwnerBatchRefusal {
            code,
            message: format!("transactions[{index}] (station {}): {why}", payload.station_id),
        });
    }
    if !payload.is_v2() {
        return None;
    }
    let station = Transaction::station_from_registration(payload);
    state
        .precheck_station_registration_v2(
            &station,
            payload.expected_nonce.unwrap_or(0),
            admission_height,
        )
        .err()
        .map(|refusal| OwnerBatchRefusal {
            code: refusal.code(),
            message: format!(
                "transactions[{index}] (station {}) refused at height {admission_height}: \
                 {refusal:?}. Read the station's current owner_nonce (GET .../owner-ledger) and \
                 re-sign the registration against it",
                payload.station_id
            ),
        })
}

/// Admit signed station owner transactions to the mempool (#3345 / #3347).
///
/// Transfer, owner action (report-stolen / unfreeze / deactivate / reactivate),
/// recovery filing, recovery cancel and recovery execute. The payload
/// signatures are the authorization, so this needs only the Contributor role
/// (the registration route is Operator because registrations are
/// governance-signed). Admission POLICY, not consensus: each tx is checked at
/// `tip + 1` — feature active, stateless verification, then a dry run of the
/// real stateful step — so a tx that would be mined as an inert no-op is refused
/// here, with a named code, while it is still fixable.
pub async fn submit_station_owner_transactions(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<StationOwnerSubmitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let (node_chain_id, node_network_id) = (identity.chain_id.clone(), identity.network_id.clone());
    if request.chain_id != node_chain_id || request.network_id != node_network_id {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "CHAIN_IDENTITY_MISMATCH",
            format!(
                "artifact chain identity ({}/{}) does not match this node ({}/{}); \
                 owner signatures bind the chain identity",
                request.chain_id, request.network_id, node_chain_id, node_network_id
            ),
        );
    }

    let now_secs = chrono::Utc::now().timestamp(); // determinism-ok: admission-time freshness gate at the API boundary; decides accept/reject only, never alters the bytes that reach consensus

    // Payload form: the node builds the (unsigned) envelope itself, with the
    // node's own clock, so a client never has to reimplement the transaction
    // serialisation or hash.
    let transactions = match build_owner_batch_transactions(
        &request.transactions,
        &request.payloads,
        now_secs,
    ) {
        Ok(transactions) => transactions,
        Err(refusal) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                refusal.code,
                refusal.message,
            );
        },
    };

    let (admission_height, outcome) = {
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ConsensusFeature as F, feature_active,
        };
        // Stations that already have a recovery_execute queued (read before
        // the chain lock so the two locks are never held together).
        let pending_executes = pending_execute_stations(&*state.mempool.read().await);
        let blockchain = state.blockchain.read().await;
        let admission_height = blockchain.height + 1;
        (
            admission_height,
            validate_owner_batch_with_pending(
                &blockchain.state,
                &transactions,
                admission_height,
                &node_chain_id,
                &node_network_id,
                now_secs,
                feature_active(F::StationOwnerConsent, admission_height),
                feature_active(F::StationOwnerActions, admission_height),
                &pending_executes,
            ),
        )
    };
    let (transaction_ids, steps) = match outcome {
        Ok(ok) => ok,
        Err(refusal) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                refusal.code,
                refusal.message,
            );
        },
    };

    if let Err(refusal) = admit_owner_batch_into_mempool(&mut *state.mempool.write().await, &transactions) {
        let status = if refusal.code == "MEMPOOL_REJECTED" {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        return envelope_error(&req, status, refusal.code, refusal.message);
    }

    HttpResponse::Created().json(StationOwnerTxSubmitResponse {
        accepted: transaction_ids.len(),
        transaction_ids,
        steps,
        admission_height,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StationOwnerFeaturesResponse {
    pub station_owner_consent: bool,
    pub station_owner_actions: bool,
    pub station_active_reward_gate: bool,
}

#[derive(Debug, Serialize)]
pub struct StationOwnerLedgerResponse {
    pub station_id: String,
    /// The registered owner NOW.
    pub owner_address: String,
    /// The registry's governance-controlled flag (distinct from
    /// `owner_ledger.owner_deactivated`).
    pub is_active: bool,
    pub owner_ledger: wrfcoin_shared_protocol::blockchain::station_owner::StationOwnerLedgerView,
    /// Frozen or owner-deactivated, AND `station_owner_actions` active at
    /// `observed_at_height + 1`: the station earns nothing right now.
    pub rewards_suspended: bool,
    /// Whether each governing feature is active at the height the next block
    /// mines at (`observed_at_height + 1`); an owner tx is inert unless its
    /// feature is active.
    pub features: StationOwnerFeaturesResponse,
    pub recovery_notice_blocks: u64,
    pub observed_at_height: u64,
    pub settled_height: u64,
    pub state_root_hex: String,
}

/// Read a station's owner-ledger entry (#3345 / #3347): the nonce the next
/// owner-signed tx must carry, the freeze and owner-deactivation flags, and any
/// pending recovery with its notice window. The only way a client learns the
/// nonce, and the way it confirms an owner tx took effect (a stateful refusal
/// is an inert no-op in the block, with no error to the submitter).
pub async fn get_station_owner_ledger(
    req: HttpRequest,
    state: web::Data<AppState>,
    station_id: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let station_id = station_id.into_inner();
    if let Err(error) = validate_station_id(&station_id) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            error.to_string(),
        );
    }
    let blockchain = state.blockchain.read().await;
    let Some(station) = blockchain.state.station_registry.get(&station_id) else {
        return envelope_error(
            &req,
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("station is not registered: {station_id}"),
        );
    };
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature as F, feature_active};
    let next = blockchain.height + 1;
    let probabilistic_floor = blockchain.height.saturating_sub(MAX_REORG_DEPTH);
    let settled_height = probabilistic_floor.max(blockchain.finalized_height());
    HttpResponse::Ok().json(StationOwnerLedgerResponse {
        station_id: station.station_id.clone(),
        owner_address: station.owner_address.clone(),
        is_active: station.is_active,
        owner_ledger: blockchain.state.station_owner_ledger_view(&station_id),
        rewards_suspended: blockchain
            .state
            .station_owner_ledger_refuses_rewards(&station_id, next),
        features: StationOwnerFeaturesResponse {
            station_owner_consent: feature_active(F::StationOwnerConsent, next),
            station_owner_actions: feature_active(F::StationOwnerActions, next),
            station_active_reward_gate: feature_active(F::StationActiveRewardGate, next),
        },
        recovery_notice_blocks:
            wrfcoin_shared_protocol::blockchain::station_owner::STATION_OWNER_RECOVERY_NOTICE_BLOCKS,
        observed_at_height: blockchain.height,
        settled_height,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

/// Default and ceiling page sizes for [`list_station_registrations`] (#3342).
pub const STATION_LIST_DEFAULT_LIMIT: usize = 200;
pub const STATION_LIST_MAX_LIMIT: usize = 500;

/// Hard ceiling on the registry size this route will enumerate (#3343 review).
/// Checked in O(1) BEFORE any enumeration; above it the route answers 503 and
/// does no per-entry work. Far above any testnet/mainnet station count; a
/// registry that outgrows it needs an ordered index, not a bigger number.
pub const STATION_LIST_MAX_REGISTRY_ENTRIES: usize = 100_000;

/// A station ordered by `station_id` for the bounded page selection.
struct ByStationId<'a>(&'a wrfcoin_shared_protocol::station_registry::WeatherStation);

#[cfg(test)]
thread_local! {
    /// Per-thread comparison count for the #3343 bounded-work regression.
    pub(crate) static STATION_PAGE_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Ord for ByStationId<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        STATION_PAGE_COMPARISONS.with(|c| c.set(c.get() + 1));
        self.0.station_id.cmp(&other.0.station_id)
    }
}
impl PartialOrd for ByStationId<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ByStationId<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.station_id == other.0.station_id
    }
}
impl Eq for ByStationId<'_> {}

/// Select the `limit` smallest stations with id strictly after `after`, in id
/// order, plus whether more exist — WITHOUT collecting or sorting the registry
/// (#3343 review). A max-heap holds at most `limit + 1` candidates, so memory
/// is O(limit) and comparisons O(n log limit): `?limit=1` never sorts the
/// registry. Returns the page, `has_more`, and the heap's peak size.
pub(crate) fn select_station_page<'a>(
    entries: impl Iterator<Item = &'a wrfcoin_shared_protocol::station_registry::WeatherStation>,
    after: Option<&str>,
    limit: usize,
) -> (
    Vec<&'a wrfcoin_shared_protocol::station_registry::WeatherStation>,
    bool,
    usize,
) {
    use std::collections::BinaryHeap;
    let keep = limit.saturating_add(1);
    // No input-sized pre-allocation: the heap grows to at most `limit + 1` (<= 501).
    let mut heap: BinaryHeap<ByStationId<'a>> = BinaryHeap::new();
    let mut peak = 0usize;
    for station in entries {
        if after.is_some_and(|cursor| station.station_id.as_str() <= cursor) {
            continue;
        }
        let candidate = ByStationId(station);
        if heap.len() < keep {
            heap.push(candidate);
        } else if heap.peek().is_some_and(|largest| candidate < *largest) {
            heap.pop();
            heap.push(candidate);
        }
        peak = peak.max(heap.len());
    }
    let mut page: Vec<_> = heap.into_sorted_vec().into_iter().map(|s| s.0).collect();
    let has_more = page.len() > limit;
    page.truncate(limit);
    (page, has_more, peak)
}

#[derive(Debug, Deserialize)]
pub struct StationListQuery {
    /// Page size; absent → [`STATION_LIST_DEFAULT_LIMIT`], above
    /// [`STATION_LIST_MAX_LIMIT`] → clamped, `0` → 422.
    pub limit: Option<usize>,
    /// Keyset cursor: return stations whose id sorts strictly after this one.
    pub after: Option<String>,
}

/// One station as the registry holds it, WITH its exact registered location.
#[derive(Debug, Serialize, Deserialize)]
pub struct CanonicalStationListEntry {
    pub station_id: String,
    pub owner_address: String,
    pub provenance_class: String,
    pub is_active: bool,
    // determinism-ok: read-only API echo of the registry's f64 latitude; nothing here is consensus arithmetic
    pub latitude: f64,
    // determinism-ok: read-only API echo of the registry's f64 longitude; nothing here is consensus arithmetic
    pub longitude: f64,
    #[serde(default)]
    // determinism-ok: read-only API echo of the registry's Option<f64> elevation; nothing here is consensus arithmetic
    pub elevation_meters: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CanonicalStationListResponse {
    pub stations: Vec<CanonicalStationListEntry>,
    /// The last `station_id` of this page when another page exists.
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// Registry size at `observed_at_height` (all stations, every page).
    pub total: usize,
    pub observed_at_height: u64,
    pub settled_height: u64,
    pub state_root_hex: String,
}

/// List the station registry with each station's EXACT registered location
/// (#3342), for the backend's chain → `weather_stations` sync.
///
/// Why Operator and not Reader, like the per-station read above: this is the
/// first surface that echoes registry coordinates unrounded. The Reader-level
/// precedent (`GET /transactions/oracle/pending`, #3240) rounds them to 0.01°
/// precisely because every Contributor and Oracle bearer satisfies Reader.
/// Exact coordinates go only to the node operator's own services; the backend's
/// anonymous map projection still coarsens them (backend#1237), and the node
/// API itself is never proxied publicly (#658).
///
/// Read-only over existing state, under one read lock, in deterministic
/// `station_id` order with keyset pagination. No state or consensus change.
pub async fn list_station_registrations(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<StationListQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let limit = match query.limit {
        None => STATION_LIST_DEFAULT_LIMIT,
        Some(0) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "limit must be at least 1",
            );
        },
        Some(n) => n.min(STATION_LIST_MAX_LIMIT),
    };
    let after = match query.after.as_deref() {
        None | Some("") => None,
        Some(cursor) => {
            if let Err(error) = validate_station_id(cursor) {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    format!("after: {error}"),
                );
            }
            Some(cursor)
        },
    };

    let blockchain = state.blockchain.read().await;
    let registry = &blockchain.state.station_registry;
    // O(1) ceiling BEFORE any per-entry work (#3343 review).
    let total = registry.len();
    if total > STATION_LIST_MAX_REGISTRY_ENTRIES {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "STATION_LIST_REGISTRY_TOO_LARGE",
            format!(
                "station registry has {total} entries, above this route's ceiling of {STATION_LIST_MAX_REGISTRY_ENTRIES}"
            ),
        );
    }
    let (page, has_more, _peak) =
        select_station_page(registry.entries().map(|(_, station)| station), after, limit);

    let stations: Vec<CanonicalStationListEntry> = page
        .into_iter()
        .map(|station| CanonicalStationListEntry {
            station_id: station.station_id.clone(),
            owner_address: station.owner_address.clone(),
            provenance_class: station.provenance_class.as_str().to_string(),
            is_active: station.is_active,
            latitude: station.location.latitude,
            longitude: station.location.longitude,
            elevation_meters: station.location.elevation_meters,
        })
        .collect();
    let next_cursor = if has_more {
        stations.last().map(|s| s.station_id.clone())
    } else {
        None
    };

    let probabilistic_floor = blockchain.height.saturating_sub(MAX_REORG_DEPTH);
    let settled_height = probabilistic_floor.max(blockchain.finalized_height());

    HttpResponse::Ok().json(CanonicalStationListResponse {
        stations,
        next_cursor,
        has_more,
        total,
        observed_at_height: blockchain.height,
        settled_height,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

#[cfg(test)]
mod owner_admission_tests {
    //! #3345 / #3347: every `OWNER_TX_*` refusal code of the owner-tx submit
    //! route, and the (B) registration owner-change refusal, exercised through
    //! the pure validators the handlers call. The feature states are supplied
    //! directly, so no process-wide schedule is installed in this binary.
    use super::*;
    use wrfcoin_shared_protocol::blockchain::station_owner::{
        PendingStationOwnerRecovery, StationOwnerActionPayload, StationOwnerLedgerEntry,
        StationOwnerRecoveryCancelPayload, StationOwnerRecoveryExecutePayload,
        StationOwnerRecoveryFilingPayload, StationOwnerTransferPayload,
    };
    use wrfcoin_shared_protocol::blockchain::transaction::{
        StationOwnerActionKind, StationRegistrationPayload, TransactionInput, TransactionOutput,
    };
    use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
    use wrfcoin_shared_protocol::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
    use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

    type State = wrfcoin_native_chain::blockchain::state::State;
    const S: &str = "ws-owner-admission";
    const HEIGHT: u64 = 1_000;

    /// Sets the governance allowlist for one test under the process-env lock and
    /// restores the previous value, so it cannot leak into other tests.
    struct GovEnv(Option<std::ffi::OsString>);
    impl GovEnv {
        fn set(value: String) -> Self {
            let previous = std::env::var_os(ORACLE_GOVERNANCE_PUBKEYS_ENV);
            unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, value) };
            Self(previous)
        }
    }
    impl Drop for GovEnv {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(v) => std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, v),
                    None => std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV),
                }
            }
        }
    }

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
    }
    fn addr(k: &PrivateKey) -> String {
        PublicKey::from_private_key(k).to_address()
    }
    fn pk(k: &PrivateKey) -> String {
        PublicKey::from_private_key(k).to_hex()
    }
    fn ident() -> (String, String) {
        let i = ChainIdentity::active();
        (i.chain_id.clone(), i.network_id.clone())
    }

    fn tx_with(data: String) -> Transaction {
        Transaction::canonical_station_owner_tx(&data, chrono::Utc::now().timestamp())
            .expect("a representable timestamp")
    }

    fn state_with(owner: &PrivateKey, governance: &PrivateKey) -> State {
        let mut payload = StationRegistrationPayload::new(
            S,
            addr(owner),
            46.7867,
            -92.1005,
            "test-real-station",
            DataQualityTier::Gold,
            ContributionProvenanceClass::Real,
        );
        payload.sign_ownership(owner);
        payload.sign(governance);
        let mut state = State::new();
        state
            .apply_station_registration(Transaction::station_from_registration(&payload))
            .expect("register");
        state
    }

    fn transfer(from: &PrivateKey, to: &PrivateKey, nonce: u64, exp: u64) -> StationOwnerTransferPayload {
        let (c, n) = ident();
        let mut p = StationOwnerTransferPayload::new(
            S, addr(from), pk(from), addr(to), pk(to), nonce, exp,
        );
        p.sign_as_owner_with_domain(&c, &n, from);
        p.countersign_with_domain(&c, &n, to);
        p
    }
    fn action(o: &PrivateKey, kind: StationOwnerActionKind, nonce: u64) -> StationOwnerActionPayload {
        let (c, n) = ident();
        let mut p = StationOwnerActionPayload::new(S, addr(o), pk(o), kind, nonce, HEIGHT + 100);
        p.sign_with_domain(&c, &n, o);
        p
    }
    fn json<T: Serialize>(p: &T) -> String {
        serde_json::to_string(p).unwrap()
    }

    /// Run one tx through the batch validator with both features on.
    fn run(state: &State, tx: &Transaction, now: i64) -> Result<(), &'static str> {
        let (c, n) = ident();
        validate_owner_batch(state, std::slice::from_ref(tx), HEIGHT, &c, &n, now, true, true)
            .map(|_| ())
            .map_err(|r| r.code)
    }
    fn code_of(state: &State, data: String) -> &'static str {
        let tx = tx_with(data);
        run(state, &tx, tx.timestamp.timestamp()).expect_err("must be refused")
    }

    #[test]
    fn every_owner_tx_refusal_code_is_reachable_and_named() {
        use StationOwnerActionKind as K;
        // Governance env for the filing checks (the same pattern the
        // transition-extractor tests use).
        let (a, b, mallory, gov, other_gov) = (key(0xA1), key(0xB2), key(0xE5), key(0x60), key(0x61));
        let _env_lock = crate::helpers::test_env::lock();
        let _env = GovEnv::set(pk(&gov));
        let state = state_with(&a, &gov);
        let (c, n) = ident();
        let exp = HEIGHT + 100;

        // Not an owner tx / feature gate / malformed.
        let plain = tx_with("{\"x\":1}".to_string());
        assert_eq!(run(&state, &plain, plain.timestamp.timestamp()), Err("OWNER_TX_NOT_OWNER_TX"));
        let transfer_tx = tx_with(json(&transfer(&a, &b, 0, exp)));
        let now = transfer_tx.timestamp.timestamp();
        assert_eq!(
            validate_owner_batch(&state, &[transfer_tx.clone()], HEIGHT, &c, &n, now, false, true)
                .map_err(|r| r.code)
                .map(|_| ()),
            Err("OWNER_TX_FEATURE_NOT_ACTIVE")
        );
        assert_eq!(
            validate_owner_batch(&state, &[tx_with(json(&action(&a, K::ReportStolen, 0)))], HEIGHT, &c, &n, now, true, false)
                .map_err(|r| r.code)
                .map(|_| ()),
            Err("OWNER_TX_FEATURE_NOT_ACTIVE")
        );
        let mut bad = transfer(&a, &b, 0, exp);
        bad.station_id = String::new();
        assert_eq!(code_of(&state, json(&bad)), "OWNER_TX_MALFORMED");
        assert_eq!(
            code_of(&state, "{\"tx_kind\":\"station_owner_transfer_v1\",\"station_id\":1}".to_string()),
            "OWNER_TX_MALFORMED"
        );

        // Transfer, stateless codes.
        assert_eq!(code_of(&state, json(&transfer(&a, &a, 0, exp))), "OWNER_TX_SAME_OWNER");
        let mut p = transfer(&a, &b, 0, exp);
        p.from_owner_pubkey = pk(&mallory);
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_FROM_KEY_NOT_BOUND");
        let mut p = transfer(&a, &b, 0, exp);
        p.to_owner_pubkey = pk(&mallory);
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_TO_KEY_NOT_BOUND");
        let mut p = transfer(&a, &b, 0, exp);
        p.from_owner_signature = None;
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_BAD_OWNER_SIGNATURE");
        let mut p = transfer(&a, &b, 0, exp);
        p.to_owner_signature = None;
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_BAD_COUNTERSIGNATURE");
        assert_eq!(code_of(&state, json(&transfer(&a, &b, 0, HEIGHT - 1))), "OWNER_TX_EXPIRED");
        assert_eq!(
            code_of(&state, json(&transfer(&a, &b, 0, HEIGHT + 10_001))),
            "OWNER_TX_VALIDITY_TOO_LONG"
        );

        // Action, stateless codes.
        let mut p = action(&a, K::ReportStolen, 0);
        p.owner_pubkey = pk(&mallory);
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_OWNER_KEY_NOT_BOUND");
        let mut p = action(&a, K::ReportStolen, 0);
        p.owner_signature = None;
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_BAD_OWNER_SIGNATURE");

        // Recovery filing, stateless codes.
        let filing = |from: &PrivateKey, to: &PrivateKey, nonce: u64, signer: &PrivateKey, accept: bool| {
            let mut p = StationOwnerRecoveryFilingPayload::new(S, addr(from), addr(to), pk(to), nonce);
            p.sign_as_governance_with_domain(&c, &n, signer);
            if accept {
                p.accept_with_domain(&c, &n, to);
            }
            p
        };
        assert_eq!(code_of(&state, json(&filing(&a, &a, 0, &gov, true))), "OWNER_TX_SAME_OWNER");
        assert_eq!(code_of(&state, json(&filing(&a, &b, 0, &other_gov, true))), "OWNER_TX_NOT_GOVERNANCE");
        let mut p = filing(&a, &b, 0, &gov, true);
        p.auth_signature = filing(&a, &b, 9, &gov, true).auth_signature;
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_BAD_GOVERNANCE_SIGNATURE");
        // Signed over a `to_owner_pubkey` that does not derive to `to_owner_address`.
        let mut p = StationOwnerRecoveryFilingPayload::new(S, addr(&a), addr(&b), pk(&mallory), 0);
        p.sign_as_governance_with_domain(&c, &n, &gov);
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_TO_KEY_NOT_BOUND");
        assert_eq!(code_of(&state, json(&filing(&a, &b, 0, &gov, false))), "OWNER_TX_BAD_COUNTERSIGNATURE");

        // Recovery cancel, stateless codes.
        let cancel = |o: &PrivateKey, nonce: u64| {
            let mut p = StationOwnerRecoveryCancelPayload::new(S, addr(o), pk(o), nonce);
            p.sign_with_domain(&c, &n, o);
            p
        };
        let mut p = cancel(&a, 0);
        p.owner_pubkey = pk(&mallory);
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_OWNER_KEY_NOT_BOUND");
        let mut p = cancel(&a, 0);
        p.owner_signature = None;
        assert_eq!(code_of(&state, json(&p)), "OWNER_TX_BAD_OWNER_SIGNATURE");
        let mut bad_exec = StationOwnerRecoveryExecutePayload::new(S, 0);
        bad_exec.station_id = "has space".to_string();
        assert_eq!(code_of(&state, json(&bad_exec)), "OWNER_TX_MALFORMED");

        // Stateful codes.
        assert_eq!(code_of(&state, json(&transfer(&mallory, &b, 0, exp))), "OWNER_TX_NOT_REGISTERED_OWNER");
        {
            let (c2, n2) = ident();
            let mut p = StationOwnerTransferPayload::new("nope", addr(&a), pk(&a), addr(&b), pk(&b), 0, exp);
            p.sign_as_owner_with_domain(&c2, &n2, &a);
            p.countersign_with_domain(&c2, &n2, &b);
            assert_eq!(code_of(&state, json(&p)), "OWNER_TX_UNKNOWN_STATION");
        }
        assert_eq!(code_of(&state, json(&transfer(&a, &b, 1, exp))), "OWNER_TX_NONCE_MISMATCH");
        assert_eq!(code_of(&state, json(&action(&a, K::Unfreeze, 0))), "OWNER_TX_NOT_FROZEN");
        assert_eq!(code_of(&state, json(&action(&a, K::Reactivate, 0))), "OWNER_TX_NOT_DEACTIVATED");
        assert_eq!(code_of(&state, json(&cancel(&a, 0))), "OWNER_TX_NO_PENDING_RECOVERY");

        let with_entry = |entry: StationOwnerLedgerEntry| {
            let mut s = state.clone();
            s.station_owner_ledger.insert(S.to_string(), entry);
            s
        };
        let frozen = with_entry(StationOwnerLedgerEntry {
            frozen: Some(wrfcoin_shared_protocol::blockchain::station_owner::StationFreeze { since_height: 5 }),
            ..Default::default()
        });
        assert_eq!(code_of(&frozen, json(&transfer(&a, &b, 0, exp))), "OWNER_TX_STATION_FROZEN");
        let deactivated = with_entry(StationOwnerLedgerEntry { owner_deactivated: true, ..Default::default() });
        assert_eq!(code_of(&deactivated, json(&action(&a, K::Deactivate, 0))), "OWNER_TX_ALREADY_DEACTIVATED");
        let exhausted = with_entry(StationOwnerLedgerEntry { owner_nonce: u64::MAX, ..Default::default() });
        assert_eq!(code_of(&exhausted, json(&action(&a, K::ReportStolen, u64::MAX))), "OWNER_TX_NONCE_EXHAUSTED");
        let pending = with_entry(StationOwnerLedgerEntry {
            pending_recovery: Some(PendingStationOwnerRecovery {
                from_owner_address: addr(&a),
                to_owner_address: addr(&b),
                recovery_nonce: 0,
                filed_at_height: HEIGHT - 10,
            }),
            ..Default::default()
        });
        assert_eq!(code_of(&pending, json(&filing(&a, &b, 0, &gov, true))), "OWNER_TX_RECOVERY_ALREADY_PENDING");
        assert_eq!(
            code_of(&pending, json(&StationOwnerRecoveryExecutePayload::new(S, 0))),
            "OWNER_TX_NOTICE_WINDOW_NOT_ELAPSED"
        );
        let elapsed = with_entry(StationOwnerLedgerEntry {
            pending_recovery: Some(PendingStationOwnerRecovery {
                from_owner_address: addr(&a),
                to_owner_address: addr(&b),
                recovery_nonce: 0,
                filed_at_height: 1,
            }),
            owner_nonce: 0,
            ..Default::default()
        });
        // filed at 1, notice 34_000: not yet elapsed at HEIGHT 1_000, so shift
        // the validator's height past it.
        let tx = tx_with(json(&cancel(&a, 0)));
        assert_eq!(
            validate_owner_batch(&elapsed, &[tx.clone()], 1 + 34_000, &c, &n, tx.timestamp.timestamp(), true, true)
                .map_err(|r| r.code)
                .map(|_| ()),
            Err("OWNER_TX_NOTICE_WINDOW_ELAPSED")
        );

        // Batch-level codes.
        let good = tx_with(json(&transfer(&a, &b, 0, exp)));
        assert!(run(&state, &good, good.timestamp.timestamp()).is_ok(), "control: admitted");
        assert_eq!(
            validate_owner_batch(&state, &[good.clone(), good.clone()], HEIGHT, &c, &n, good.timestamp.timestamp(), true, true)
                .map_err(|r| r.code)
                .map(|_| ()),
            Err("OWNER_TX_DUPLICATE_STATION_IN_BATCH")
        );
        assert_eq!(run(&state, &good, good.timestamp.timestamp() + 151), Err("OWNER_TX_STALE_TIMESTAMP"));
        assert!(run(&state, &good, good.timestamp.timestamp() + 150).is_ok(), "the window is inclusive of 150s");
        let mut tampered = good.clone();
        tampered.fee = 7;
        assert_eq!(run(&state, &tampered, tampered.timestamp.timestamp()), Err("OWNER_TX_ID_MISMATCH"));
        let mut no_id = good.clone();
        no_id.id = None;
        assert_eq!(run(&state, &no_id, no_id.timestamp.timestamp()), Err("OWNER_TX_ID_MISSING"));
    }

    /// (B): an owner-changing registration is refused once consent is active,
    /// a same-owner correction and a first registration are not, and nothing
    /// is refused below activation.
    #[test]
    fn registration_owner_change_is_refused_only_under_consent() {
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let state = state_with(&a, &gov);
        let reg = |owner: &PrivateKey, station: &str| {
            let mut p = StationRegistrationPayload::new(
                station,
                addr(owner),
                46.7867,
                -92.1005,
                "test-real-station",
                DataQualityTier::Gold,
                ContributionProvenanceClass::Real,
            );
            p.sign_ownership(owner);
            p.sign(&gov);
            p
        };
        let refused = registration_owner_change_refusal(&state, &reg(&b, S), true, 0)
            .expect("a different owner for a registered station is refused");
        assert_eq!(refused.code, "STATION_OWNER_CHANGE_REQUIRES_CONSENT");
        assert!(registration_owner_change_refusal(&state, &reg(&b, S), false, 0).is_none(), "below activation: unchanged");
        assert!(registration_owner_change_refusal(&state, &reg(&a, S), true, 0).is_none(), "same-owner correction passes");
        assert!(registration_owner_change_refusal(&state, &reg(&b, "ws-brand-new"), true, 0).is_none(), "a first registration passes");
    }

    // ── Payload form (the node builds the envelope) ────────────────────────

    fn payload_item(kind: &str, data: &str) -> OwnerPayloadItem {
        OwnerPayloadItem { kind: kind.to_string(), payload_json: data.to_string() }
    }

    /// Payload form == envelope form, for every kind: the node-built tx is the
    /// same transaction a client would hand-build, and admission agrees.
    #[test]
    fn payload_form_equals_envelope_form_for_every_kind() {
        use StationOwnerActionKind as K;
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let _env_lock = crate::helpers::test_env::lock();
        let _env = GovEnv::set(pk(&gov));
        let state = state_with(&a, &gov);
        let (c, n) = ident();
        let exp = HEIGHT + 100;
        let now = 1_700_000_000i64;
        let mut filing = StationOwnerRecoveryFilingPayload::new(S, addr(&a), addr(&b), pk(&b), 0);
        filing.sign_as_governance_with_domain(&c, &n, &gov);
        filing.accept_with_domain(&c, &n, &b);
        let mut cancel = StationOwnerRecoveryCancelPayload::new(S, addr(&a), pk(&a), 0);
        cancel.sign_with_domain(&c, &n, &a);
        let cases: Vec<(&str, String)> = vec![
            ("station_owner_transfer_v1", json(&transfer(&a, &b, 0, exp))),
            ("station_owner_action_v1", json(&action(&a, K::ReportStolen, 0))),
            ("station_owner_action_v1", json(&action(&a, K::Unfreeze, 0))),
            ("station_owner_action_v1", json(&action(&a, K::Deactivate, 0))),
            ("station_owner_action_v1", json(&action(&a, K::Reactivate, 0))),
            ("station_owner_recovery_file_v1", json(&filing)),
            ("station_owner_recovery_cancel_v1", json(&cancel)),
            ("station_owner_recovery_execute_v1", json(&StationOwnerRecoveryExecutePayload::new(S, 0))),
        ];
        for (kind, data) in cases {
            let built = build_owner_batch_transactions(&[], &[payload_item(kind, &data)], now)
                .unwrap_or_else(|r| panic!("{kind}: {r:?}"));
            assert_eq!(built.len(), 1);
            let tx = &built[0];
            // The payload text is forwarded byte for byte.
            assert_eq!(tx.data.as_deref(), Some(data.as_str()), "{kind}: data is verbatim");
            // The node chose the timestamp.
            assert_eq!(tx.timestamp.timestamp(), now, "{kind}: node-chosen timestamp");
            assert_eq!(tx.id, Some(tx.calculate_hash()), "{kind}: id recomputes");
            // Identical to the envelope a client would build with the same constructor.
            let owner = tx.outputs[0].address.clone();
            let by_hand = Transaction::new_station_owner_tx(data.clone(), &owner, now).unwrap();
            assert_eq!(tx.id, by_hand.id, "{kind}: payload form == envelope form");
            // Admission agrees across the two forms.
            let via_payload = validate_owner_batch(&state, &built, HEIGHT, &c, &n, now, true, true)
                .map(|(ids, steps)| (ids, steps.iter().map(|x| (x.station_id.clone(), x.kind.clone(), x.nonce)).collect::<Vec<_>>()))
                .map_err(|r| r.code);
            let via_envelope = validate_owner_batch(&state, &[by_hand], HEIGHT, &c, &n, now, true, true)
                .map(|(ids, steps)| (ids, steps.iter().map(|x| (x.station_id.clone(), x.kind.clone(), x.nonce)).collect::<Vec<_>>()))
                .map_err(|r| r.code);
            assert_eq!(via_payload, via_envelope, "{kind}");
        }
    }

    /// The node-built timestamp is the node's `now`, so the 150 s freshness
    /// check passes by construction; an envelope with a stale timestamp does not.
    #[test]
    fn payload_form_timestamp_is_node_chosen_and_inside_the_window() {
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let state = state_with(&a, &gov);
        let (c, n) = ident();
        let data = json(&transfer(&a, &b, 0, HEIGHT + 100));
        let item = payload_item("station_owner_transfer_v1", &data);
        let now = chrono::Utc::now().timestamp();
        let built = build_owner_batch_transactions(&[], &[item], now).unwrap();
        assert!(validate_owner_batch(&state, &built, HEIGHT, &c, &n, now, true, true).is_ok());
        assert!(
            validate_owner_batch(&state, &built, HEIGHT, &c, &n, now + 150, true, true).is_ok(),
            "still inside the 150 s window a moment later"
        );
        assert_eq!(
            validate_owner_batch(&state, &built, HEIGHT, &c, &n, now + 151, true, true)
                .map_err(|r| r.code)
                .map(|_| ()),
            Err("OWNER_TX_STALE_TIMESTAMP")
        );
    }

    /// Whitespace and key order in `payload_json` do not change the signed
    /// preimage (it is rebuilt from the parsed fields), so every spelling
    /// verifies and is admitted.
    #[test]
    fn payload_json_formatting_and_key_order_do_not_change_the_preimage() {
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let state = state_with(&a, &gov);
        let (c, n) = ident();
        let canonical = transfer(&a, &b, 0, HEIGHT + 100);
        let compact = json(&canonical);
        let pretty = serde_json::to_string_pretty(&canonical).unwrap();
        let value: serde_json::Value = serde_json::from_str(&compact).unwrap();
        let reversed = {
            let obj = value.as_object().unwrap();
            let fields: Vec<String> = obj
                .iter()
                .rev()
                .map(|(k, v)| format!("{}  :\n {}", serde_json::to_string(k).unwrap(), v))
                .collect();
            format!("{{ \n {} \n}}", fields.join(" ,\n "))
        };
        assert_ne!(compact, pretty);
        assert_ne!(compact, reversed);
        let want = canonical.message_with_domain(
            wrfcoin_shared_protocol::blockchain::transaction::STATION_OWNER_TRANSFER_DOMAIN,
            &c,
            &n,
        );
        let now = 1_700_000_000i64;
        for (label, text) in [("compact", &compact), ("pretty", &pretty), ("key-reversed", &reversed)] {
            let parsed: StationOwnerTransferPayload = serde_json::from_str(text).unwrap();
            assert_eq!(
                parsed.message_with_domain(
                    wrfcoin_shared_protocol::blockchain::transaction::STATION_OWNER_TRANSFER_DOMAIN,
                    &c,
                    &n
                ),
                want,
                "{label}: same preimage"
            );
            let built = build_owner_batch_transactions(
                &[],
                &[payload_item("station_owner_transfer_v1", text)],
                now,
            )
            .unwrap();
            assert_eq!(built[0].data.as_deref(), Some(text.as_str()), "{label}: forwarded verbatim");
            assert!(
                validate_owner_batch(&state, &built, HEIGHT, &c, &n, now, true, true).is_ok(),
                "{label}: admitted"
            );
        }
    }

    #[test]
    fn body_form_refusals_are_named() {
        let (a, b) = (key(0xA1), key(0xB2));
        let data = json(&transfer(&a, &b, 0, HEIGHT + 100));
        let item = || payload_item("station_owner_transfer_v1", &data);
        let env = tx_with(data.clone());
        let code = |r: Result<Vec<Transaction>, OwnerBatchRefusal>| r.unwrap_err().code;
        assert_eq!(code(build_owner_batch_transactions(&[], &[], 0)), "OWNER_TX_BODY_EMPTY");
        assert_eq!(
            code(build_owner_batch_transactions(std::slice::from_ref(&env), &[item()], 0)),
            "OWNER_TX_BODY_FORM"
        );
        assert_eq!(
            code(build_owner_batch_transactions(&[], &[payload_item("station_owner_action_v1", &data)], 0)),
            "OWNER_TX_KIND_MISMATCH"
        );
        assert_eq!(
            code(build_owner_batch_transactions(&[], &[payload_item("station_owner_transfer_v1", "not json")], 0)),
            "OWNER_TX_MALFORMED"
        );
        assert!(build_owner_batch_transactions(std::slice::from_ref(&env), &[], 0).is_ok());
    }

    // ── recovery_execute: admission refusals and one pending per station ──────

    fn execute_tx(station: &str, nonce: u64) -> Transaction {
        tx_with(json(&StationOwnerRecoveryExecutePayload::new(station, nonce)))
    }

    /// A state whose station has a recovery filed at `filed_at`.
    fn state_with_pending_recovery(filed_at: u64) -> State {
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let mut state = state_with(&a, &gov);
        state.station_owner_ledger.insert(
            S.to_string(),
            StationOwnerLedgerEntry {
                pending_recovery: Some(PendingStationOwnerRecovery {
                    from_owner_address: addr(&a),
                    to_owner_address: addr(&b),
                    recovery_nonce: 0,
                    filed_at_height: filed_at,
                }),
                ..Default::default()
            },
        );
        state
    }

    /// An execute that cannot do anything is refused AT ADMISSION, before the
    /// mempool sees it: no station, nothing pending, window not yet elapsed.
    #[test]
    fn recovery_execute_that_cannot_work_is_refused_before_the_mempool() {
        let (c, n) = ident();
        let none = std::collections::BTreeSet::new();
        let run = |state: &State, tx: &Transaction, height: u64| {
            validate_owner_batch_with_pending(
                state, std::slice::from_ref(tx), height, &c, &n, tx.timestamp.timestamp(), true,
                true, &none,
            )
            .map(|_| ())
            .map_err(|r| r.code)
        };
        // Unknown station (also: no ledger entry at all).
        let (a, gov) = (key(0xA1), key(0x60));
        let bare = state_with(&a, &gov);
        assert_eq!(run(&bare, &execute_tx("no-such-station", 0), HEIGHT), Err("OWNER_TX_NO_PENDING_RECOVERY"));
        // A registered station with nothing pending.
        assert_eq!(run(&bare, &execute_tx(S, 0), HEIGHT), Err("OWNER_TX_NO_PENDING_RECOVERY"));
        // A pending recovery whose notice window has not elapsed.
        let pending = state_with_pending_recovery(HEIGHT - 10);
        assert_eq!(run(&pending, &execute_tx(S, 0), HEIGHT), Err("OWNER_TX_NOTICE_WINDOW_NOT_ELAPSED"));
        // The wrong recovery nonce.
        assert_eq!(
            run(&pending, &execute_tx(S, 9), HEIGHT - 10 + 34_000),
            Err("OWNER_TX_NONCE_MISMATCH")
        );
        // A recovery whose station was never registered but whose ledger names it
        // (a state the chain cannot reach, but admission must still refuse).
        let mut orphan = State::new();
        orphan.station_owner_ledger.insert(
            "ghost".to_string(),
            StationOwnerLedgerEntry {
                pending_recovery: Some(PendingStationOwnerRecovery {
                    from_owner_address: "wrf-x".to_string(),
                    to_owner_address: "wrf-y".to_string(),
                    recovery_nonce: 0,
                    filed_at_height: 1,
                }),
                ..Default::default()
            },
        );
        assert_eq!(run(&orphan, &execute_tx("ghost", 0), 1 + 34_000), Err("OWNER_TX_UNKNOWN_STATION"));
        // Control: ripe, right nonce, nothing queued: admitted.
        assert!(run(&pending, &execute_tx(S, 0), HEIGHT - 10 + 34_000).is_ok());
    }

    /// At most one pending recovery_execute per station: a second is refused with
    /// a named code until the first leaves the mempool.
    #[test]
    fn at_most_one_recovery_execute_per_station_is_pending() {
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::mempool::pool::Mempool;
        let (c, n) = ident();
        let ripe = HEIGHT - 10 + 34_000;
        let state = state_with_pending_recovery(HEIGHT - 10);
        let tx = execute_tx(S, 0);
        let now = tx.timestamp.timestamp();
        let check = |pending: &std::collections::BTreeSet<String>| {
            validate_owner_batch_with_pending(
                &state, std::slice::from_ref(&tx), ripe, &c, &n, now, true, true, pending,
            )
            .map(|_| ())
            .map_err(|r| r.code)
        };
        let mut mempool = Mempool::new(&Config::default());
        assert!(pending_execute_stations(&mempool).is_empty());
        assert!(check(&pending_execute_stations(&mempool)).is_ok(), "nothing queued: admitted");

        mempool.add_transaction(tx.clone()).expect("the first execute is queued");
        let queued = pending_execute_stations(&mempool);
        assert!(queued.contains(S));
        assert_eq!(check(&queued), Err("OWNER_TX_EXECUTE_ALREADY_PENDING"));

        // A different station is unaffected, and a non-execute owner tx never counts.
        assert!(!queued.contains("another-station"));
        let other = Transaction::new_station_owner_tx(
            json(&action(&key(0xA1), StationOwnerActionKind::ReportStolen, 0)),
            "wrf-owner",
            tx.timestamp.timestamp(),
        )
        .unwrap();
        mempool.add_transaction(other).expect("a non-execute owner tx");
        assert_eq!(pending_execute_stations(&mempool).len(), 1);

        // Once the first is mined or evicted the slot frees.
        mempool.remove_transaction(&tx.id.unwrap());
        assert!(pending_execute_stations(&mempool).is_empty());
        assert!(check(&pending_execute_stations(&mempool)).is_ok());
    }

    // ── canonical envelope and the in-lock execute re-check ───────────────────

    /// Only the canonical envelope for a payload is admitted. A crafted input
    /// txid would defeat the mempool's double-spend dedupe and a fee would skew
    /// its ordering, so any deviation (other than the timestamp, within the
    /// window) is refused with a named code.
    #[test]
    fn only_the_canonical_envelope_is_admitted() {
        let (a, b, gov) = (key(0xA1), key(0xB2), key(0x60));
        let state = state_with(&a, &gov);
        let (c, n) = ident();
        let data = json(&transfer(&a, &b, 0, HEIGHT + 100));
        let now = chrono::Utc::now().timestamp();
        let canonical = Transaction::canonical_station_owner_tx(&data, now).unwrap();
        let code = |tx: &Transaction| {
            validate_owner_batch(&state, std::slice::from_ref(tx), HEIGHT, &c, &n, now, true, true)
                .map(|_| ())
                .map_err(|r| r.code)
        };
        assert!(canonical.is_canonical_station_owner_envelope());
        assert_eq!(code(&canonical), Ok(()), "control: the canonical envelope is admitted");

        let mut crafted_input = canonical.clone();
        crafted_input.inputs[0].txid = wrfcoin_shared_protocol::crypto::hash::hash_string("crafted");
        crafted_input.set_id();
        assert_eq!(code(&crafted_input), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        let mut with_fee = canonical.clone();
        with_fee.fee = 5;
        with_fee.set_id();
        assert_eq!(code(&with_fee), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        let mut other_output = canonical.clone();
        other_output.outputs[0].address = "wrf-someone-else".to_string();
        other_output.set_id();
        assert_eq!(code(&other_output), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        let mut nonzero_output = canonical.clone();
        nonzero_output.outputs[0].amount = 1;
        nonzero_output.set_id();
        assert_eq!(code(&nonzero_output), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        let mut extra_output = canonical.clone();
        let spare = extra_output.outputs[0].clone();
        extra_output.outputs.push(spare);
        extra_output.set_id();
        assert_eq!(code(&extra_output), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        let mut subsecond = canonical.clone();
        subsecond.timestamp = canonical.timestamp + chrono::Duration::milliseconds(500);
        subsecond.set_id();
        assert_eq!(code(&subsecond), Err("OWNER_TX_ENVELOPE_NONCANONICAL"));

        // The timestamp IS the sender's to choose, within the window.
        let shifted = Transaction::canonical_station_owner_tx(&data, now - 100).unwrap();
        assert_eq!(code(&shifted), Ok(()), "a different whole-second timestamp inside the window");
        // The node-built (payload form) transaction is canonical by construction.
        let built = build_owner_batch_transactions(
            &[],
            &[payload_item("station_owner_transfer_v1", &data)],
            now,
        )
        .unwrap();
        assert!(built[0].is_canonical_station_owner_envelope());
    }

    /// The one-pending-execute rule is re-checked UNDER the mempool lock. The
    /// second execute here is a different payload (so a different input txid and
    /// the mempool's own double-spend check cannot catch it) for the same
    /// station: only the re-check refuses it. With the re-check removed this
    /// test goes RED.
    #[test]
    fn the_in_lock_recheck_refuses_a_second_execute_the_mempool_would_accept() {
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::mempool::pool::Mempool;
        let canon = |nonce: u64, station: &str| {
            Transaction::canonical_station_owner_tx(
                &json(&StationOwnerRecoveryExecutePayload::new(station, nonce)),
                1_700_000_000,
            )
            .unwrap()
        };
        let mut mempool = Mempool::new(&Config::default());
        let first = canon(0, S);
        let racing = canon(1, S); // validated earlier, against an empty mempool

        // Control: with nothing queued the batch is admitted, all of it.
        let mut empty = Mempool::new(&Config::default());
        assert!(admit_owner_batch_into_mempool(&mut empty, &[first.clone()]).is_ok());
        assert_eq!(empty.size(), 1);

        // The race: `first` landed after `racing` was validated.
        admit_owner_batch_into_mempool(&mut mempool, &[first.clone()]).expect("first admitted");
        let refusal = admit_owner_batch_into_mempool(&mut mempool, &[racing.clone()])
            .expect_err("the second execute for the station must be refused");
        assert_eq!(refusal.code, "OWNER_TX_EXECUTE_ALREADY_PENDING");
        assert_eq!(mempool.size(), 1, "nothing was added");
        // The mempool on its own WOULD accept it: that is what the re-check is for.
        assert!(Mempool::new(&Config::default()).add_transaction(racing.clone()).is_ok());

        // All or nothing: an earlier item of the same batch is rolled back.
        let other_station = canon(0, "another-station");
        let refusal = admit_owner_batch_into_mempool(&mut mempool, &[other_station.clone(), racing])
            .expect_err("batch refused");
        assert_eq!(refusal.code, "OWNER_TX_EXECUTE_ALREADY_PENDING");
        assert_eq!(mempool.size(), 1, "the earlier item was rolled back");
        assert!(!mempool.contains(&other_station.id.unwrap()));
    }

    /// Registration FORMAT admission (`StationRegistrationNonce`): v1 only
    /// before activation, v2 only after, and v2 must carry the station's
    /// current ledger nonce. Each refusal has a named code.
    #[test]
    fn registration_format_and_nonce_refusals_are_named() {
        let (a, gov) = (key(0xA1), key(0x60));
        let state = state_with(&a, &gov);
        let payload = |nonce: Option<u64>| {
            let mut p = StationRegistrationPayload::new(
                S,
                addr(&a),
                46.7867,
                -92.1005,
                "test-real-station",
                DataQualityTier::Gold,
                ContributionProvenanceClass::Real,
            );
            if let Some(n) = nonce {
                p = p.with_expected_nonce(n);
            }
            p.sign_ownership(&a);
            p.sign(&gov);
            p
        };
        let code = |p: &StationRegistrationPayload, active: bool| {
            registration_nonce_refusal(&state, p, HEIGHT, active, 0).map(|r| r.code)
        };
        // Before activation: v1 passes, v2 is refused.
        assert_eq!(code(&payload(None), false), None);
        assert_eq!(
            code(&payload(Some(0)), false),
            Some("STATION_REGISTRATION_NONCE_FEATURE_INACTIVE")
        );
        // From activation: v1 is refused, v2 with the matching nonce passes.
        assert_eq!(code(&payload(None), true), Some("STATION_REGISTRATION_V1_REFUSED"));
        assert_eq!(code(&payload(Some(0)), true), None, "the station has no ledger entry: nonce 0");
        assert_eq!(
            code(&payload(Some(1)), true),
            Some("STATION_REGISTRATION_NONCE_MISMATCH"),
            "a nonce ahead of the ledger is refused too"
        );
        // After the ledger moves on, the old nonce is refused.
        let mut advanced = state.clone();
        advanced.station_owner_ledger.insert(
            S.to_string(),
            StationOwnerLedgerEntry { owner_nonce: 4, ..Default::default() },
        );
        let at = |p: &StationRegistrationPayload| {
            registration_nonce_refusal(&advanced, p, HEIGHT, true, 0).map(|r| r.code)
        };
        assert_eq!(at(&payload(Some(0))), Some("STATION_REGISTRATION_NONCE_MISMATCH"));
        assert_eq!(at(&payload(Some(4))), None);
        let mut exhausted = state.clone();
        exhausted.station_owner_ledger.insert(
            S.to_string(),
            StationOwnerLedgerEntry { owner_nonce: u64::MAX, ..Default::default() },
        );
        assert_eq!(
            registration_nonce_refusal(&exhausted, &payload(Some(u64::MAX)), HEIGHT, true, 0)
                .map(|r| r.code),
            Some("STATION_REGISTRATION_NONCE_EXHAUSTED")
        );
    }
}
