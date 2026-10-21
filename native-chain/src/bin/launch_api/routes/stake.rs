use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{envelope_error, require_role};
use crate::state::{AppState, StakeMetricsSnapshot};
use wrfcoin_native_chain::blockchain::state::ContributionRewardAggregate;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::security::api_auth::ApiRole;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct StakeStatusResponse {
    pub address: String,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub stake_required: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub stake_locked: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub spendable_balance: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub earned_balance_total: u128,
    #[serde(serialize_with = "crate::routes::decimal::contribution_rewards")]
    pub contribution_rewards: ContributionRewardAggregate,
    pub maturity_percent: f64,
    pub note: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StakeMetricsResponse {
    pub account_count: usize,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub total_stake_required: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub total_stake_locked: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub total_spendable_balance: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub total_earned_balance: u128,
    pub network_stake_maturity_percent: f64,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn get_stake_status(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let address = path.into_inner();
    let parsed_address = match address.parse::<Address>() {
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

    let blockchain = state.blockchain.read().await;
    let account = blockchain.state.get_account(&parsed_address);
    let contribution_rewards = blockchain
        .state
        .contribution_reward_aggregate(&parsed_address);
    let (stake_required, stake_locked, spendable_balance, earned_balance_total, note) =
        if let Some(account) = account {
            (
                account.stake_required.into_inner(),
                account.stake_locked.into_inner(),
                account.spendable_balance.into_inner(),
                account.earned_balance_total.into_inner(),
                "State-backed account snapshot".to_string(),
            )
        } else {
            (
                0,
                0,
                0,
                0,
                "Account not initialized in state; returning default stake snapshot".to_string(),
            )
        };

    let maturity_percent = if stake_required == 0 {
        100.0
    } else {
        ((stake_locked as f64 / stake_required as f64) * 100.0).min(100.0)
    };

    HttpResponse::Ok().json(StakeStatusResponse {
        address,
        stake_required,
        stake_locked,
        spendable_balance,
        earned_balance_total,
        contribution_rewards,
        maturity_percent,
        note,
    })
}

pub async fn get_stake_metrics(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    let current_height = blockchain.height;

    // #1385: reuse the aggregate snapshot when chain height is unchanged.
    {
        let cache = state
            .stake_metrics_cache
            .lock()
            .expect("stake_metrics_cache poisoned");
        if let Some(snapshot) = cache.as_ref() {
            if snapshot.computed_at_height == current_height {
                return HttpResponse::Ok().json(StakeMetricsResponse {
                    account_count: snapshot.account_count,
                    total_stake_required: snapshot.total_stake_required,
                    total_stake_locked: snapshot.total_stake_locked,
                    total_spendable_balance: snapshot.total_spendable_balance,
                    total_earned_balance: snapshot.total_earned_balance,
                    network_stake_maturity_percent: snapshot.network_stake_maturity_percent,
                });
            }
        }
    }

    let accounts = blockchain.state.accounts.values();

    let mut account_count = 0usize;
    let mut total_stake_required = 0u128;
    let mut total_stake_locked = 0u128;
    let mut total_spendable_balance = 0u128;
    let mut total_earned_balance = 0u128;

    for account in accounts {
        account_count += 1;
        total_stake_required =
            total_stake_required.saturating_add(account.stake_required.into_inner());
        total_stake_locked = total_stake_locked.saturating_add(account.stake_locked.into_inner());
        total_spendable_balance =
            total_spendable_balance.saturating_add(account.spendable_balance.into_inner());
        total_earned_balance =
            total_earned_balance.saturating_add(account.earned_balance_total.into_inner());
    }

    let network_stake_maturity_percent = if total_stake_required == 0 {
        100.0
    } else {
        ((total_stake_locked as f64 / total_stake_required as f64) * 100.0).min(100.0)
    };

    let response = StakeMetricsResponse {
        account_count,
        total_stake_required,
        total_stake_locked,
        total_spendable_balance,
        total_earned_balance,
        network_stake_maturity_percent,
    };

    {
        let mut cache = state
            .stake_metrics_cache
            .lock()
            .expect("stake_metrics_cache poisoned");
        *cache = Some(StakeMetricsSnapshot {
            computed_at_height: current_height,
            account_count: response.account_count,
            total_stake_required: response.total_stake_required,
            total_stake_locked: response.total_stake_locked,
            total_spendable_balance: response.total_spendable_balance,
            total_earned_balance: response.total_earned_balance,
            network_stake_maturity_percent: response.network_stake_maturity_percent,
        });
    }

    HttpResponse::Ok().json(response)
}
