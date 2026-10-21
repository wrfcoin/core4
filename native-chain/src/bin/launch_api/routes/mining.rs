use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "p2p")]
use crate::helpers::notify_p2p_chain_advanced;
use crate::helpers::{envelope_error, require_role, validate_participant_id};
use crate::state::AppState;
use crate::transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct MineBlockRequest {
    pub miner_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MineBlockResponse {
    pub height: u64,
    pub hash: String,
    pub transaction_count: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn mine_block(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<MineBlockRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.miner_address, "miner_address") {
        return resp;
    }
    let _miner_address = match request.miner_address.parse::<Address>() {
        Ok(addr) => addr,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "Invalid miner_address format. Expected 20-byte hex address.",
            );
        },
    };

    // Partition-mode gate (#1217). A node in a minority partition must
    // not produce new blocks because they cannot reach BFT finality
    // (< 1/3 stake) and will be reverted on reconnection. Returns 503
    // so the operator's mining loop backs off rather than retry-storming
    // an unreachable block production path.
    //
    // The check happens before the block_production_lock to avoid
    // holding that lock for a doomed request.
    let current_mode = *state.partition_mode.read().await;
    if !current_mode.allows_block_production() {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "PARTITION_READ_ONLY",
            format!(
                "Block production halted: node is in {} partition mode (local stake < 1/3 of total). \
                 Mining cannot proceed until the partition heals.",
                current_mode.label()
            ),
        );
    }

    // #3093 upgrade deadline: the next block is at or beyond the height at
    // which the genesis schedule activates a feature this binary does not
    // implement. Refuse to produce it and request the clean halt — a block
    // built on the old rules here is a silent fork on every upgraded peer.
    {
        let next_height = state.blockchain.read().await.height.saturating_add(1);
        if let Some(deadline) = wrfcoin_native_chain::upgrade::production_refused_at(next_height) {
            let message = wrfcoin_native_chain::upgrade::request_upgrade_halt(
                &deadline,
                next_height,
                "block production (/blocks/mine)",
            );
            return envelope_error(&req, StatusCode::SERVICE_UNAVAILABLE, "UPGRADE_REQUIRED", message);
        }
    }

    // Serialize concurrent mine requests so only one block is produced at a
    // time, preventing wasted work from stale-tip races (#974).
    let _mine_guard = state.block_production_lock.lock().await;

    // ---------------------------------------------------------------------
    // #2290 STAGED BUILD-THEN-COMMIT SEAM.
    //
    // `create_pop_block_with_state` mutates whatever `Arc<RwLock<..>>` handles
    // it is handed: it appends the block to the chain, swaps in the post-block
    // `State`, and drains the mempool. Running it against the LIVE handles and
    // gating afterwards accepted the block into the running node even when the
    // gate refused it — the 409 stopped persistence but not acceptance, which
    // is a self-fork on the node's own view.
    //
    // So build against detached STAGING copies. Nothing the consensus engine
    // does is observable through `state.blockchain` / `state.mempool` until the
    // gate passes and we deliberately install the result below.
    // ---------------------------------------------------------------------
    let (staged_blockchain, base_height, base_tip) = {
        let blockchain = state.blockchain.read().await;
        (blockchain.clone(), blockchain.height, blockchain.tip)
    };
    let staged_mempool = state.mempool.read().await.clone();
    // Derived from the clone itself, so the id set and the pool it describes are
    // from the same instant — a transaction admitted concurrently cannot land
    // between the two and desync the removal diff computed at commit time.
    let mempool_ids_before = staged_mempool.transaction_ids();
    // Must precede candidate construction: this is the local view from which
    // omitted sufficient-fee transactions can be observed (#2199).
    let ordering_snapshot = staged_mempool.ordering_snapshot();

    // #1855: capture pre-block state before the consensus engine mutates it so
    // extract_transitions works against the same baseline the P2P applier uses
    // (pre-block, not post-block state). Taken from the staging snapshot, which
    // is byte-identical to live at this point.
    let pre_block_state = staged_blockchain.state.clone();

    let staged_blockchain = Arc::new(RwLock::new(staged_blockchain));
    let staged_mempool = Arc::new(RwLock::new(staged_mempool));

    // Stateful PoP mining path: this applies governance/reward transitions
    // and quality/trust side effects as part of block creation.
    // Governance transitions are serialized into block metadata by the
    // consensus engine so extract_transitions can read them directly
    // instead of cloning the full State for simulation (#940).
    let block = match state
        .consensus
        .create_pop_block_with_state(
            staged_blockchain.clone(),
            staged_mempool.clone(),
            &request.miner_address,
        )
        .await
    {
        Ok(b) => b,
        Err(e) => {
            let msg = e.to_string();
            // Defense-in-depth: the outer HTTP guard catches the
            // read-only case before reaching here, but if the mempool
            // flipped to ReadOnly between the outer check and this
            // call (or a future caller skips the outer guard), the
            // deeper adapter/shared-protocol guards will bubble up
            // `PartitionReadOnly` / `SP_CONS_PARTITION_READ_ONLY`.
            // Map both to the same 503 envelope so operator tooling
            // sees a single error code (#1217).
            let is_partition_read_only = msg.contains("PartitionReadOnly")
                || msg.contains("SP_CONS_PARTITION_READ_ONLY")
                || msg.contains("partition read-only");
            let is_sync_only = msg.contains("SYNC_ONLY");
            let is_stale_tip = msg.contains("stale tip");
            // #2290: the builder now applies the SAME producer-authority gate at
            // its append boundary, and being deeper it fires before the
            // route-level check below. Map it to the identical envelope so one
            // condition keeps one status and one code — otherwise the same
            // refusal reaches operators as 409 or 500 depending on which guard
            // happened to see it first.
            let is_producer_unauthorized = msg.contains("SP_CONS_PRODUCER_UNAUTHORIZED");
            // #2523: a block-1 cut that would install no oracle set is an
            // operator/ceremony error, not a node fault. It must not reach the
            // operator as 500 — that reads as "our bug, retry", and a retry
            // produces the same refusal. 422 matches the submit-side rejection
            // in #2535 so the ceremony sees one consistent class for "your
            // artifact is wrong", whichever surface catches it.
            let is_block_one_no_oracle_set = msg.contains("SP_CONS_BLOCK_ONE_NO_ORACLE_SET");
            // #3365: the #3361 option-B refusal — a faucet chain's block 1 cut
            // with no faucet address — is the same class: the operator's cut
            // input is wrong ("set WRFCOIN_GENESIS_FAUCET_ADDRESS"), a retry
            // refuses identically, and the reason must reach the operator
            // rather than hide behind a redacted 500 correlation id.
            let is_block_one_no_faucet_address =
                msg.contains("SP_CONS_BLOCK_ONE_NO_FAUCET_ADDRESS");
            let (status, code) = if is_sync_only {
                (StatusCode::SERVICE_UNAVAILABLE, "SYNC_ONLY")
            } else if is_partition_read_only {
                (StatusCode::SERVICE_UNAVAILABLE, "PARTITION_READ_ONLY")
            } else if is_producer_unauthorized {
                (StatusCode::CONFLICT, "PRODUCER_NOT_AUTHORIZED")
            } else if is_block_one_no_oracle_set {
                (StatusCode::UNPROCESSABLE_ENTITY, "BLOCK_ONE_NO_ORACLE_SET")
            } else if is_block_one_no_faucet_address {
                (StatusCode::UNPROCESSABLE_ENTITY, "BLOCK_ONE_NO_FAUCET_ADDRESS")
            } else if is_stale_tip {
                (StatusCode::CONFLICT, "CONFLICT")
            } else {
                // #2138: an unclassified failure is a node fault. Its text is
                // logged against a correlation id; the caller gets the class.
                return crate::helpers::envelope_redacted(
                    &req,
                    wrfcoin_native_chain::security::error_redaction::InternalError::new(
                        wrfcoin_native_chain::security::error_redaction::ErrorKind::Consensus,
                        format!("stateful block creation failed: {e}"),
                    ),
                );
            };
            return envelope_error(&req, status, code, format!("Consensus stateful block creation failed: {e}"));
        },
    };
    // #2290 PRODUCER-AUTH GATE ON THE LOCAL PRODUCTION PATH.
    //
    // Authoritative application of both gossip and IBD blocks
    // (`pop_adapter::validate_block`) runs this check, but `/blocks/mine` did
    // not — and this route is the testnet's ACTUAL production path (the N5
    // collector drives it). Without
    // this, a node whose key is not in the authorized set, or whose signing is
    // misconfigured, would mine and PERSIST blocks the whole network rejects
    // while reporting nothing wrong locally: a silent self-fork on the one node
    // that matters. AUTH_LIVE_FAILURE_PLAYBOOK §1.7, in code.
    //
    // Deliberately the SAME shared function the acceptance boundary calls,
    // so the producer cannot drift from the appliers. It is gated against the
    // PRE-block `producer_set_history` — exactly the snapshot the appliers see
    // when they validate the block before applying it — so producer and applier
    // cannot disagree about which set was in force.
    //
    // Because the block was built against detached staging copies, refusing
    // here leaves the live chain, the live mempool, and storage byte-identical
    // to their state before the request. The refusal is genuinely atomic: the
    // block is neither accepted nor persisted, and there is nothing to unwind.
    if let Err(reason) =
        wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized(
            &block,
            &pre_block_state.producer_set_history,
        )
    {
        log::error!( // secret-leak-ok: reason is a public producer-authority error string with no key or signature bytes
            target: "consensus",
            "REFUSING locally mined block height={}: producer not authorized ({reason}). \
             This node's producer key is not in the on-chain authorized set, or block signing is \
             misconfigured. Peers would reject this block; it was built against a staging copy \
             and has NOT been accepted into the live chain or written to storage.",
            block.height,
        );
        return envelope_error(
            &req,
            StatusCode::CONFLICT,
            "PRODUCER_NOT_AUTHORIZED",
            format!( // secret-leak-ok: reason is a public producer-authority error string with no key or signature bytes
                "Refusing to produce a block this network would reject: {reason}. \
                 The block was NOT accepted or persisted. Check that this node's producer key is \
                 in the genesis/on-chain producer set and that block signing is wired (#2290)."
            ),
        );
    }

    // ---------------------------------------------------------------------
    // #2290 COMMIT. The gate passed, so install the staged result.
    //
    // Compare-and-swap on (height, tip): the staged chain was forked from a
    // snapshot, so if anything advanced the live chain while we were building
    // (a gossiped block applied by the P2P path), installing would silently
    // revert it. Refuse instead, with the same 409 the stale-tip race already
    // returns, and leave live state untouched.
    //
    // The mempool is NOT swapped wholesale — that would drop transactions
    // submitted concurrently with block production. Instead the removals the
    // engine performed on the staging pool are diffed out and replayed against
    // the live pool, so concurrently admitted transactions survive.
    // ---------------------------------------------------------------------
    let removed_mempool_ids: Vec<_> = {
        let staged = staged_mempool.read().await;
        mempool_ids_before
            .into_iter()
            .filter(|tx_id| !staged.contains(tx_id))
            .collect()
    };
    {
        let mut blockchain = state.blockchain.write().await;
        if blockchain.height != base_height || blockchain.tip != base_tip {
            log::warn!(
                target: "consensus",
                "Discarding locally mined block height={}: live chain advanced from height={} tip={} \
                 to height={} tip={} while it was being built. Nothing was applied.",
                block.height,
                base_height,
                base_tip,
                blockchain.height,
                blockchain.tip,
            );
            return envelope_error(
                &req,
                StatusCode::CONFLICT,
                "CONFLICT",
                "Consensus stateful block creation failed: stale tip — the chain advanced while \
                 the block was being built. Retry.",
            );
        }
        // Cheap move out of the staging handle; the emptied Default is dropped.
        *blockchain = std::mem::take(&mut *staged_blockchain.write().await);
        // #3277 F4: a self-cut node re-resolves its base from the block 1 it
        // just mined (the same schedule it stamped, now read from the block).
        wrfcoin_native_chain::upgrade::reresolve_base_schedule_if_block1(&block);
        // #3277 S3: the canonical state changed; the effective schedule and
        // the clean-halt deadline follow it (never from the staging copy).
        wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
            &blockchain.state,
            "locally mined block",
        );
    }
    {
        let mut mempool = state.mempool.write().await;
        for tx_id in &removed_mempool_ids {
            mempool.remove_transaction(tx_id);
        }
    }

    let hash = block.hash.unwrap_or_else(|| block.calculate_hash());
    let state_transitions = extract_transitions(
        &pre_block_state,
        &block,
        block.height,
        block.timestamp.timestamp(),
    );

    let block_height = block.height;
    let tx_count = block.transactions.len();
    let consensus_label = block
        .metadata
        .get("consensus")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    CORE_METRICS.record_block_ordering(
        &block,
        &ordering_snapshot,
        &hex::encode(block.producer_pubkey),
    );

    log::info!(
        "Mined block height={} hash={} txs={} consensus={}",
        block_height,
        hash,
        tx_count,
        consensus_label,
    );

    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        let sr: [u8; 32] = {
            let bc = state.blockchain.read().await;
            bc.state.state_root.0
        };
        notify_p2p_chain_advanced(p2p, &block, true, sr);
    }

    // Broadcast NewBlock event to WebSocket subscribers (#1081).
    let _ = state.event_tx.send(crate::websocket::WsEvent::NewBlock {
        height: block_height,
        hash: hash.to_string(),
    });

    // Emit RewardDistributed events for each reward transition (#971).
    // First-reward detection: if the account's earned_balance_total equals the
    // reward amount, this is the first reward (it was zero before the block).
    {
        let blockchain = state.blockchain.read().await;
        for t in &state_transitions {
            let (address, amount, reason) = match t {
                StateTransition::ApplyReward {
                    address,
                    amount,
                    reason,
                } => (address, amount, reason),
                StateTransition::ApplyContributionRewardDecision { record, .. }
                    if record.amount.0 != 0
                        && record
                            .commit_id()
                            .map(|commit_id| {
                                blockchain
                                    .state
                                    .finalized_contribution_rewards
                                    .contains(commit_id)
                            })
                            .unwrap_or(false) =>
                {
                    (&record.address, &record.amount, &record.reason)
                },
                _ => continue,
            };
            let amount_u64 = u64::try_from(amount.into_inner()).unwrap_or_else(|_| {
                log::warn!(
                    "Reward amount {} exceeds u64::MAX, saturating",
                    amount.into_inner(),
                );
                u64::MAX
            });
            let address_hex = hex::encode(address.as_bytes());
            let is_first = blockchain
                .state
                .get_account(address)
                .map(|acct| acct.earned_balance_total.0 == amount.0)
                .unwrap_or(false);

            if is_first {
                log::info!(
                    target: "rewards",
                    "First reward for contributor {}: {} base units ({})",
                    address_hex, amount_u64, reason,
                );
            }

            log::info!(
                target: "rewards",
                "Reward distributed: contributor={} amount={} reason={} first={}",
                address_hex, amount_u64, reason, is_first,
            );

            let _ = state
                .event_tx
                .send(crate::websocket::WsEvent::RewardDistributed {
                    contributor: address_hex,
                    amount: amount_u64,
                    reason: reason.clone(),
                    is_first_reward: is_first,
                    block_height,
                });
        }
    }

    // Persist block and transitions to storage (moves block — must be last).
    let (checkpoint_state, finalized_height) = {
        let blockchain = state.blockchain.read().await;
        let storage = state.storage.read().await;
        (
            storage.checkpoint_state_bytes(block_height, &blockchain.state),
            blockchain.finalized_height(),
        )
    };
    match checkpoint_state {
        Ok(checkpoint_state) => {
            let mut storage = state.storage.write().await;
            if let Err(e) = storage
                .persist_block_with_finalized(
                    block,
                    state_transitions,
                    checkpoint_state,
                    finalized_height,
                )
                .await
            {
                log::error!("Failed to persist block {}: {}", block_height, e);
            }
        },
        Err(e) => {
            log::error!("Failed to persist block {}: {}", block_height, e);
        },
    }

    HttpResponse::Created().json(MineBlockResponse {
        height: block_height,
        hash: hash.to_string(),
        transaction_count: tx_count,
    })
}

// ---------------------------------------------------------------------------
// Mining stats read-only endpoint (#935)
// ---------------------------------------------------------------------------

/// GET /mining/stats — return basic mining statistics derived from the chain.
pub async fn get_mining_stats(state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;

    let height = blockchain.height;
    let blocks = blockchain.blocks();
    let block_count = blocks.len();

    // Derive average block time from the most recent blocks (up to 20).
    let recent: Vec<_> = blocks.values().rev().take(20).collect();
    let avg_block_time_secs = if recent.len() >= 2 {
        let newest = recent.first().unwrap().timestamp;
        let oldest = recent.last().unwrap().timestamp;
        let span = newest.signed_duration_since(oldest).num_seconds().max(1);
        span as f64 / (recent.len() - 1) as f64
    } else {
        0.0
    };

    // Total transactions across all in-memory blocks, memoized per block
    // height to avoid rescanning every block on every scrape (#1385).
    let total_transactions: usize = {
        let mut cache = state
            .mining_tx_count_cache
            .lock()
            .expect("mining_tx_count_cache poisoned");
        cache.retain(|block_height, _| blocks.contains_key(block_height));

        let mut total = 0usize;
        for (block_height, block) in blocks {
            let tx_count = *cache
                .entry(*block_height)
                .or_insert_with(|| block.transactions.len());
            total = total.saturating_add(tx_count);
        }
        total
    };

    // Latest block difficulty.
    let difficulty = blocks.values().next_back().map(|b| b.difficulty).unwrap_or(0);

    HttpResponse::Ok().json(json!({
        "chain_height": height,
        "blocks_in_memory": block_count,
        "total_transactions": total_transactions,
        "difficulty": difficulty,
        "avg_block_time_secs": avg_block_time_secs,
    }))
}
