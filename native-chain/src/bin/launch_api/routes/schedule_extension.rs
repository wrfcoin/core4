//! `POST /transactions/governance/schedule-extensions` (#3277 S4): admit a
//! governance-signed feature-schedule extension to this node's mempool.
//!
//! The body is the signed artifact exactly as `wrfcoin governance
//! schedule-extension sign` writes it. The node wraps it in a
//! `FeatureScheduleExtension` transaction; nothing it adds is signed or
//! consensus-relevant beyond the artifact itself.
//!
//! Every check the producer would make is run HERE first, so an operator gets
//! the precise reason at submission instead of a 201 followed by a silent
//! prune at block assembly (the failure mode the registration routes guard
//! against too):
//!
//! 1. the non-consensus admission policy (known, schedulable features only);
//! 2. the full V1–V9 validity against this node's canonical state at the next
//!    height (`validate_extension_against_state`, the function the apply arm
//!    runs), which needs the installed schedule-extension authority;
//! 3. mempool admission.
//!
//! Operator-only. Submit on a producing node: like the registration routes
//! this does not gossip, the extension reaches the chain when this node mines.

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use wrfcoin_native_chain::blockchain::state::State;
use wrfcoin_native_chain::blockchain::transaction::Transaction;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    MIN_RUNUP_BLOCKS, ScheduleEntry, SignedFeatureScheduleExtension, check_admission_policy,
    extension_gate_active, schedule_extension_authority, validate_extension_against_state,
};

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduleExtensionSubmitResponse {
    pub transaction_id: String,
    pub sequence: u64,
    /// The height the extension was validated for (this node's tip + 1).
    pub validated_for_height: u64,
    /// The last carrying height at which every entry still satisfies the
    /// minimum run-up (V7). A block carrying the extension later than this is
    /// invalid, so the producer would drop it: re-sign with later heights.
    pub latest_valid_carrying_height: u64,
    pub entries: Vec<ScheduleEntry>,
}

pub async fn submit_schedule_extension(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SignedFeatureScheduleExtension>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let signed = request.into_inner();

    // #3361 F3: the faucet binding is admissible only on the chain instance
    // its row names — this node's block 1, via its installed authority.
    let node_genesis = schedule_extension_authority().map(|a| a.context().genesis_hash);
    if let Err(e) = check_admission_policy(&signed.extension, node_genesis) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("refused by this binary's admission policy (#3277): {e}"),
        );
    }

    let Some(authority) = schedule_extension_authority() else {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "this node has no schedule-extension authority installed, so it can apply no \
             extension (#3277)",
        );
    };

    let (next_height, validity) = {
        let chain = state.blockchain.read().await;
        let next_height = chain.height.saturating_add(1);
        let base = State::base_feature_schedule();
        let validity = if !extension_gate_active(&base, next_height) {
            Err(format!(
                "the extension transaction is not enabled at height {next_height} (V1): \
                 schedule_extension is not active yet"
            ))
        } else {
            validate_extension_against_state(
                &signed,
                next_height,
                &authority.context(),
                &base,
                &chain.state.feature_schedule_extensions,
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        };
        (next_height, validity)
    };
    if let Err(reason) = validity {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("invalid at height {next_height} (#3277): {reason}"),
        );
    }

    let tx = Transaction::new_feature_schedule_extension(&signed);
    let Some(transaction_id) = tx.id.map(|id| id.to_hex()) else {
        return envelope_error(
            &req,
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "the extension transaction carries no id",
        );
    };
    if let Err(err) = state.mempool.write().await.add_transaction(tx) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("rejected by mempool: {err}"),
        );
    }

    let latest_valid_carrying_height = signed
        .extension
        .entries
        .iter()
        .map(|e| e.activation_height.saturating_sub(MIN_RUNUP_BLOCKS))
        .min()
        .unwrap_or(next_height);
    HttpResponse::Created().json(ScheduleExtensionSubmitResponse {
        transaction_id,
        sequence: signed.extension.sequence,
        validated_for_height: next_height,
        latest_valid_carrying_height,
        entries: signed.extension.entries,
    })
}
