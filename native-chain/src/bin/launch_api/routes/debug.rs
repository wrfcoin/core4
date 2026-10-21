use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Deserialize;

use crate::helpers::{debug_seed_api_disabled, envelope_error, envelope_redacted, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::common::types::{Address, Balance};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::security::error_redaction::{ErrorKind, InternalError};

#[derive(Debug, Deserialize)]
pub struct SeedAccountRequest {
    pub address: String,
    pub amount: u128,
    /// Optional: also lock this amount as governance stake so the account has
    /// voting power.  Stake is taken from the seeded balance.
    pub stake: Option<u128>,
}

pub async fn seed_account(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<SeedAccountRequest>,
) -> impl Responder {
    // #1811 consensus-safety gate: `seed_account` mutates `accounts` /
    // `stake_locked` / `spendable_balance` — all hashed into the consensus
    // `state_root` — out of band (no gossiped tx, never reconstructed by
    // `extract_transitions`), so any call forks this node from its peers. Off by
    // default; return 503 unless explicitly opted in on a single-node /
    // trusted-operator deployment. Checked before auth so a disabled node
    // uniformly reports the route as unavailable.
    if !state.debug_seed_api_enabled {
        return debug_seed_api_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let address = match body.address.parse::<Address>() {
        Ok(addr) => addr,
        Err(_) => return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", "Invalid address format"),
    };

    let mut blockchain = state.blockchain.write().await;
    let transition = StateTransition::ApplyReward {
        address,
        amount: Balance::new(body.amount),
        reason: "Debug Seed".to_string(),
    };

    if let Err(e) = blockchain.apply_state_transition(transition) {
        // #2138: the state-transition error text is a node internal; logged, not sent.
        return envelope_redacted(&req, InternalError::new(ErrorKind::Consensus, format!("debug seed apply_state_transition: {e}")));
    }

    // Optionally lock stake for governance voting power.
    if let Some(stake_amount) = body.stake {
        if stake_amount > 0 {
            if let Some(account) = blockchain.state.get_account_mut(&address) {
                let stake_bal = Balance::new(stake_amount);
                if account.spendable_balance >= stake_bal {
                    account.stake_locked += stake_bal;
                    account.spendable_balance -= stake_bal;
                } else {
                    return envelope_error(
                        &req,
                        StatusCode::BAD_REQUEST,
                        "BAD_REQUEST",
                        "Stake amount exceeds spendable balance",
                    );
                }
            }
            // Direct field mutation bypasses apply_state_transition, so
            // recompute state_root to keep it consistent (#1858).
            blockchain.state.recalculate_state_root();
        }
    }

    HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "address": body.address,
        "amount": body.amount,
        "stake": body.stake.unwrap_or(0),
    }))
}
