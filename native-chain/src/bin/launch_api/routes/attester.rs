//! `GET /node/attester`: this node's attester status, for the local dashboard.
//!
//! Operator role (the local dashboard already holds that token). Every field
//! is a public chain fact or derived from one: the oracle id is a public key,
//! the seated set is on chain, attestations and pay are block data, and the
//! balance is the public account state of the oracle key's address. No key,
//! token, path or address of the host is ever returned.
//!
//! Which oracle: `?oracle=<64 hex>` if given, else the public oracle id the
//! launcher declares in `WRFCOIN_ATTESTER_ORACLE_ID` (read once at first use).
//! With neither, the route answers `configured: false` and nothing else.
//!
//! Counting reuses `wrfcoin_shared_protocol::oracle::statement`, the same fold
//! as `wrfcoin oracle statement`, over the blocks this node holds in memory
//! (at most [`RECENT_BLOCKS`]). Read-only: no state is written and nothing here
//! is reachable from block validation.

use std::sync::OnceLock;

use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Deserialize;
use serde_json::{Value, json};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::oracle::statement::{StatementBlock, compute_statement, format_units};

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;

/// Env var holding this node's public oracle id (set by the launcher).
pub const ATTESTER_ORACLE_ID_ENV: &str = "WRFCOIN_ATTESTER_ORACLE_ID";
/// Recent window the route counts over, in blocks (one 512-block epoch).
pub const RECENT_BLOCKS: u64 = 512;
/// How far back the route looks for the registration that seated the oracle.
pub const SEAT_LOOKBACK_BLOCKS: u64 = 8_192;

#[derive(Debug, Deserialize)]
pub struct AttesterQuery {
    pub oracle: Option<String>,
}

fn configured_oracle_id() -> Option<String> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        // determinism-ok: local operator config naming which public oracle id a read-only status route reports; never reaches block validation or state.
        std::env::var(ATTESTER_ORACLE_ID_ENV)
            .ok()
            .and_then(|raw| normalize_oracle_id(&raw))
    })
    .clone()
}

/// 64 lowercase hex characters, or `None`.
pub fn normalize_oracle_id(raw: &str) -> Option<String> {
    let id = raw.trim().trim_start_matches("0x").to_ascii_lowercase();
    (id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// One oracle registration seen in a block: its height and whether the
/// absolute set it names contains the oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetChange {
    pub height: u64,
    pub contains: bool,
}

/// Where the current seat began, from registrations scanned oldest-first.
///
/// `Some(h)`: the first registration of the latest unbroken run that includes
/// the oracle. `None`: no registration in the window includes it (it was
/// seated earlier than the window, or is not seated).
pub fn seated_since(changes: &[SetChange]) -> Option<u64> {
    let mut since = None;
    for change in changes {
        match (change.contains, since) {
            (true, None) => since = Some(change.height),
            (false, _) => since = None,
            _ => {},
        }
    }
    since
}

/// Registration set changes in `blocks`, oldest first.
fn set_changes<'a>(oracle_id: &str, blocks: impl Iterator<Item = &'a Block>) -> Vec<SetChange> {
    let mut out = Vec::new();
    for block in blocks {
        for tx in &block.transactions {
            if let Some(reg) = tx.parse_oracle_registration() {
                out.push(SetChange {
                    height: block.height,
                    contains: reg
                        .oracle_set
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(oracle_id)),
                });
            }
        }
    }
    out
}

/// Inputs the response is built from, gathered under one read lock.
pub struct AttesterFacts {
    pub oracle_id: String,
    pub tip: u64,
    pub seated: bool,
    pub set_size: usize,
    pub quorum: usize,
    pub changes: Vec<SetChange>,
    pub lookback_from: u64,
    pub recent: Vec<StatementBlock>,
    pub recent_from: u64,
    pub balance_units: Option<u128>,
    pub skipped_blocks: u64,
}

/// The response body. Pure, so it is tested without a running node.
pub fn attester_view(f: &AttesterFacts) -> Value {
    let statement = compute_statement(&f.oracle_id, f.recent_from, f.tip, &f.recent, None);
    let since = seated_since(&f.changes);
    let payee = PublicKey::from_hex(&f.oracle_id)
        .ok()
        .map(|pk| Address::from_public_key(&pk).to_string());
    // Seat depth matters: an attester should start only once the set change
    // that seated it is at least 3 blocks deep.
    let seat_depth = since.map(|h| f.tip.saturating_sub(h));
    json!({
        "configured": true,
        "oracle_id": f.oracle_id,
        "seated": f.seated,
        "set_size": f.set_size,
        "quorum": f.quorum,
        "seated_since_height": if f.seated { since } else { None },
        "seat_depth_blocks": if f.seated { seat_depth } else { None },
        "seated_before_window": f.seated && since.is_none(),
        "seat_lookback_from_height": f.lookback_from,
        "tip_height": f.tip,
        "recent": {
            "from_height": f.recent_from,
            "to_height": f.tip,
            "blocks_in_memory": statement.blocks_scanned,
            "blocks_unreadable": f.skipped_blocks,
            "attestations_included": statement.attestations_included,
            "paid_votes": statement.paid_votes,
            "approve_votes_not_paid": statement.unpaid_approve_votes,
            "paid_units": statement.paid_units.to_string(),
            "paid_wrf": format_units(statement.paid_units),
        },
        "last_attested_height": statement.last_attested_height,
        "payee_address": payee,
        "payee_balance_units": f.balance_units.map(|b| b.to_string()),
        "payee_balance_wrf": f.balance_units.map(format_units),
    })
}

/// `GET /node/attester`
pub async fn get_node_attester(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<AttesterQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let oracle_id = match query.oracle.as_deref() {
        Some(raw) => match normalize_oracle_id(raw) {
            Some(id) => id,
            None => {
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    "oracle must be 64 hex characters (the oracle public key)",
                );
            },
        },
        None => match configured_oracle_id() {
            Some(id) => id,
            None => {
                return HttpResponse::Ok().json(json!({
                    "configured": false,
                    "hint": "no oracle id: start the node with WRFCOIN_ATTESTER_ORACLE_ID or pass ?oracle=<id>",
                }));
            },
        },
    };

    let facts = {
        let blockchain = state.blockchain.read().await;
        let tip = blockchain.height;
        let recent_from = tip.saturating_sub(RECENT_BLOCKS - 1);
        let lookback_from = tip.saturating_sub(SEAT_LOOKBACK_BLOCKS - 1);
        let window = blockchain
            .blocks()
            .range(lookback_from..=tip)
            .map(|(_, b)| b);
        let changes = set_changes(&oracle_id, window);
        let mut skipped = 0u64;
        let recent = blockchain
            .blocks()
            .range(recent_from..=tip)
            .filter_map(|(_, block)| match StatementBlock::from_block(block) {
                Ok(b) => Some(b),
                Err(_) => {
                    skipped += 1;
                    None
                },
            })
            .collect();
        let seated = blockchain
            .state
            .oracle_set
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&oracle_id));
        let balance_units = PublicKey::from_hex(&oracle_id).ok().map(|pk| {
            blockchain
                .state
                .get_account(&Address::from_public_key(&pk))
                .map_or(0u128, |a| a.balance.into_inner() as u128)
        });
        AttesterFacts {
            oracle_id,
            tip,
            seated,
            set_size: blockchain.state.oracle_set.len(),
            quorum: blockchain.state.oracle_quorum,
            changes,
            lookback_from,
            recent,
            recent_from,
            balance_units,
            skipped_blocks: skipped,
        }
    };
    HttpResponse::Ok().json(attester_view(&facts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::oracle::attestation::{
        AttesterAllocation, AttesterSettlementRecord,
    };
    use wrfcoin_shared_protocol::oracle::statement::IncludedVote;

    const ME: &str = "c4dbd69bc84a4651db0a03ccc142a019c0856126ac3c920e361cba2269034725"; // pragma: allowlist secret -- public ed25519 oracle id (a public key), test data

    #[test]
    fn seated_since_is_the_start_of_the_latest_run() {
        let c = |height, contains| SetChange { height, contains };
        assert_eq!(seated_since(&[]), None);
        assert_eq!(seated_since(&[c(10, false)]), None);
        assert_eq!(
            seated_since(&[c(10, false), c(20, true), c(20, true)]),
            Some(20)
        );
        assert_eq!(
            seated_since(&[c(5, true), c(10, false), c(20, true)]),
            Some(20)
        );
        assert_eq!(seated_since(&[c(5, true), c(9, true)]), Some(5));
        assert_eq!(seated_since(&[c(5, true), c(10, false)]), None);
    }

    #[test]
    fn oracle_id_normalizes() {
        assert_eq!(
            normalize_oracle_id(&format!(" 0x{} ", ME.to_uppercase())).as_deref(),
            Some(ME)
        );
        assert_eq!(normalize_oracle_id("c4db"), None);
        assert_eq!(normalize_oracle_id(&"z".repeat(64)), None);
    }

    fn facts(seated: bool, changes: Vec<SetChange>) -> AttesterFacts {
        AttesterFacts {
            oracle_id: ME.into(),
            tip: 28_040,
            seated,
            set_size: 4,
            quorum: 2,
            changes,
            lookback_from: 19_849,
            recent: vec![StatementBlock {
                height: 28_025,
                votes: vec![IncludedVote {
                    oracle_id: ME.into(),
                    commit_id: "aa".into(),
                    approved: true,
                }],
                settlements: vec![AttesterSettlementRecord {
                    decision_id: "aa".into(),
                    finalization_height: 28_025,
                    allocations: vec![AttesterAllocation {
                        attester_id: ME.into(),
                        amount: 41_666_667,
                    }],
                }],
            }],
            recent_from: 27_529,
            balance_units: Some(999_999_996),
            skipped_blocks: 0,
        }
    }

    #[test]
    fn view_reports_seat_activity_and_balance() {
        let v = attester_view(&facts(
            true,
            vec![SetChange {
                height: 28_015,
                contains: true,
            }],
        ));
        assert_eq!(v["configured"], true);
        assert_eq!(v["seated"], true);
        assert_eq!(v["seated_since_height"], 28_015);
        assert_eq!(v["seat_depth_blocks"], 25);
        assert_eq!(v["seated_before_window"], false);
        assert_eq!(v["recent"]["attestations_included"], 1);
        assert_eq!(v["recent"]["paid_votes"], 1);
        assert_eq!(v["recent"]["paid_wrf"], "0.41666667");
        assert_eq!(v["last_attested_height"], 28_025);
        assert_eq!(
            v["payee_address"],
            "0x5e0030865d3c59cabf338d5fafc578af9f651eba"
        );
        assert_eq!(v["payee_balance_wrf"], "9.99999996");
    }

    #[test]
    fn view_without_a_seat_has_no_seat_height() {
        let v = attester_view(&facts(
            false,
            vec![SetChange {
                height: 28_015,
                contains: true,
            }],
        ));
        assert_eq!(v["seated"], false);
        assert!(v["seated_since_height"].is_null());
        assert!(v["seat_depth_blocks"].is_null());
        let v = attester_view(&facts(true, vec![]));
        assert_eq!(v["seated_before_window"], true);
    }

    #[test]
    fn view_carries_no_secret_shaped_fields() {
        let v = attester_view(&facts(true, vec![]));
        let text = v.to_string().to_ascii_lowercase();
        for word in ["token", "private", "seed", "key_file", "path"] {
            assert!(
                !v.as_object().unwrap().keys().any(|k| k.contains(word)),
                "{word}"
            );
        }
        assert!(!text.contains("bearer"));
    }
}
