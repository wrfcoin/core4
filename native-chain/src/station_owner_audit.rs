//! `station_owner_key_audit`: before `device_submission_limit` is scheduled,
//! how many station owners can sign a `set_submitter_key` today (#3565
//! design 1, owner ruling 2026-11-14 item 4: passkey-only owners are audited
//! first, counts only).
//!
//! Station owner actions are ed25519 only. An owner can sign one when their
//! `owner_address` has ever appeared on chain derived from an ed25519 public
//! key (a transaction input key, a registration ownership proof, an owner
//! action or transfer key, or a commit `auth_pubkey`). An owner whose address
//! never did is a passkey-only (or unknown) owner.
//!
//! Also counts the stations with a recent contribution commit signed by a key
//! that is NOT the owner key: those need an approved submitter key before the
//! limit, and the ones among them whose owner cannot sign are the stations a
//! passkey-signed owner action (or a 12-word wallet) would have to cover.
//!
//! The copy is replayed under the chain's own protocol and feature schedule
//! (block 1 + applied extensions, as the node boots) and the reconstructed
//! state is checked against the tip's committed state root before counting.
//!
//! READ-ONLY by intent, but opening the store may run checkpoint migrations:
//! run it ONLY on a COPY of a node's block DB, never on a live node volume
//! (the `db-copy-and-replay` kits make the copy). Prints counts and nothing
//! else: no station id, address, key or coordinate.
//!
//!   station_owner_key_audit <path/to/copy/blocks.redb> [recent_blocks=2000]   (bin: src/bin/station_owner_key_audit.rs)

use std::collections::{BTreeMap, BTreeSet};

use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::crypto::signature::PublicKey;
use crate::storage::{GateThenLoadError, Storage, gate_then_load_state, verify_tip_state_root_commitment};

/// Every ed25519 public key a transaction carries, as the addresses they derive.
fn ed25519_addresses(tx: &Transaction, out: &mut BTreeSet<String>) {
    let mut add = |hex: &str| {
        if hex.len() == 64 {
            if let Ok(key) = PublicKey::from_hex(hex) {
                out.insert(normalize(&key.to_address()));
            }
        }
    };
    for input in &tx.inputs {
        if let Some(key) = input.public_key.as_deref() {
            add(key);
        }
    }
    if let Some(data) = tx.data.as_deref() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
            walk(&value, &mut add);
        }
    }
}

/// Any string field whose name ends in `pubkey` (owner_pubkey, from_/to_owner_pubkey,
/// auth_pubkey, submitter_pubkey, ...), at any depth.
fn walk(value: &serde_json::Value, add: &mut impl FnMut(&str)) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if k.ends_with("pubkey") {
                    if let Some(s) = v.as_str() {
                        add(s);
                    }
                }
                walk(v, add);
            }
        },
        serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, add)),
        _ => {},
    }
}

/// Lowercase 40-hex owner identity (same folding as `same_station_owner`).
pub fn normalize(address: &str) -> String {
    let a = address.trim().to_ascii_lowercase();
    a.strip_prefix("0x").or_else(|| a.strip_prefix("wrf")).unwrap_or(&a).to_string()
}

/// One station as the audit sees it.
pub struct AuditStation {
    pub owner: String,
    pub active: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AuditCounts {
    pub stations: usize,
    pub active_stations: usize,
    pub owners: usize,
    pub owners_with_ed25519_key: usize,
    pub owners_passkey_or_unknown: usize,
    pub active_stations_of_passkey_or_unknown_owners: usize,
    pub stations_with_recent_commits: usize,
    pub stations_with_recent_non_owner_commits: usize,
    pub of_which_owner_cannot_sign: usize,
}

/// Pure counting over the registry and the blocks (height, tx).
pub fn audit<'a>(
    stations: &BTreeMap<String, AuditStation>,
    blocks: impl Iterator<Item = (u64, &'a Transaction)>,
    tip: u64,
    recent_blocks: u64,
) -> AuditCounts {
    let mut bound = BTreeSet::new();
    let mut recent: BTreeMap<&str, bool> = BTreeMap::new(); // station -> any non-owner-signed commit
    let since = tip.saturating_sub(recent_blocks);
    for (height, tx) in blocks {
        ed25519_addresses(tx, &mut bound);
        if height < since || tx.tx_type != TransactionType::ContributionCommit {
            continue;
        }
        let Some(commit) = tx.parse_commit_payload() else { continue };
        let Some((id, station)) = stations.get_key_value(commit.station_id.as_str()) else { continue };
        let owner_signed = commit
            .auth_pubkey
            .as_deref()
            .and_then(|k| PublicKey::from_hex(k).ok())
            .is_some_and(|k| normalize(&k.to_address()) == station.owner);
        *recent.entry(id.as_str()).or_insert(false) |= !owner_signed;
    }
    let owners: BTreeSet<&str> = stations.values().map(|s| s.owner.as_str()).collect();
    let can_sign = |owner: &str| bound.contains(owner);
    AuditCounts {
        stations: stations.len(),
        active_stations: stations.values().filter(|s| s.active).count(),
        owners: owners.len(),
        owners_with_ed25519_key: owners.iter().filter(|o| can_sign(o)).count(),
        owners_passkey_or_unknown: owners.iter().filter(|o| !can_sign(o)).count(),
        active_stations_of_passkey_or_unknown_owners: stations
            .values()
            .filter(|s| s.active && !can_sign(&s.owner))
            .count(),
        stations_with_recent_commits: recent.len(),
        stations_with_recent_non_owner_commits: recent.values().filter(|n| **n).count(),
        of_which_owner_cannot_sign: recent
            .iter()
            .filter(|(id, non_owner)| **non_owner && !can_sign(&stations[**id].owner))
            .count(),
    }
}


/// Open the DB COPY at `path`, replay it under the copied chain's own protocol
/// and feature schedule, verify the reconstruction against the tip commitment,
/// then count. `Err` carries a reason, never a station id, address or key.
pub fn run_audit(path: &std::path::Path, recent_blocks: u64) -> Result<(u64, AuditCounts), String> {
    let storage = Storage::open(path, 100).map_err(|e| format!("cannot open the block DB copy: {e}"))?;
    // Like the node's boot gate and replay_verify: signing version, activation
    // table and base feature schedule (with the block-1-keyed bootstrap) from
    // durable block 1, the schedule-extension authority, and the
    // applied-extension overlay at every replayed block boundary
    // (gate_then_load_state). Without them a WAL tail past a feature's
    // activation replays under every-feature-off rules and fails, or
    // reconstructs the wrong state.
    let loaded = gate_then_load_state(&storage, |storage| {
        let block1 = storage
            .load_lowest_block()
            .map_err(|e| format!("cannot read block 1: {e}"))?
            .ok_or_else(|| "the DB holds no blocks".to_string())?;
        if block1.height != 1 {
            return Err(format!("the lowest stored block is {}, not block 1", block1.height));
        }
        wrfcoin_shared_protocol::crypto::chain_binding::install_protocol_from_block1(&block1)
            .map_err(|e| format!("block 1's protocol could not be resolved: {e}"))?;
        crate::upgrade::install_schedule_extension_authority_for_block1(&block1);
        Ok::<(), String>(())
    });
    let chain = match loaded {
        Ok((_, Some(chain))) => chain,
        Ok((_, None)) => return Err("the DB holds no state".into()),
        Err(GateThenLoadError::Gate(e)) => return Err(format!("protocol gate: {e}")),
        Err(GateThenLoadError::Load(e)) => return Err(format!("cannot load state: {e}")),
    };
    // Count only from a verified reconstruction (#2219).
    verify_tip_state_root_commitment(&chain)
        .map_err(|e| format!("the reconstructed state does not match the tip commitment: {e}"))?;
    let stations: BTreeMap<String, AuditStation> = chain
        .state
        .station_registry
        .entries()
        .map(|(id, s)| (id.clone(), AuditStation { owner: normalize(&s.owner_address), active: s.is_active }))
        .collect();
    let mut txs: Vec<(u64, Transaction)> = Vec::new();
    let mut tip = 0u64;
    storage
        .scan_persisted_blocks(|chunk| {
            for (height, block) in chunk {
                tip = tip.max(*height);
                txs.extend(block.transactions.iter().cloned().map(|tx| (*height, tx)));
            }
            std::ops::ControlFlow::Continue(())
        })
        .map_err(|e| format!("block scan: {e}"))?;
    Ok((tip, audit(&stations, txs.iter().map(|(h, tx)| (*h, tx)), tip, recent_blocks)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{CommitPayload, TransactionInput, TransactionOutput};
    use crate::crypto::hash::hash_string;
    use crate::crypto::signature::PrivateKey;

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).unwrap()
    }
    fn addr(k: &PrivateKey) -> String {
        normalize(&PublicKey::from_private_key(k).to_address())
    }
    fn tx(tx_type: TransactionType, tag: &str, input_key: Option<&PrivateKey>, data: Option<String>) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("in-{tag}")),
                vout: 0,
                signature: None,
                public_key: input_key.map(|k| PublicKey::from_private_key(k).to_hex()),
                coinbase_data: None,
            }],
            vec![TransactionOutput { amount: 1, address: format!("wrf-{tag}"), script: "OP_TRUE".into(), nft_data: None }],
        );
        tx.tx_type = tx_type;
        tx.data = data;
        tx.set_id();
        tx
    }
    fn commit_tx(station: &str, signer: &PrivateKey, tag: &str) -> Transaction {
        let mut c = CommitPayload {
            commit_id: hash_string(tag).to_hex(),
            station_id: station.into(),
            timestamp: 1,
            contribution_type: "weather".into(),
            auth_pubkey: None,
            auth_signature: None,
        };
        c.sign_authorization(signer);
        tx(TransactionType::ContributionCommit, tag, None, Some(serde_json::to_string(&c).unwrap()))
    }

    #[test]
    fn counts_owners_who_can_sign_and_stations_that_need_approval() {
        // Owner A has signed something (a tx input); owner P never showed an
        // ed25519 key (a passkey owner). Collector key C signs commits.
        let (a, p, c) = (key(1), key(2), key(3));
        let mut stations = BTreeMap::new();
        stations.insert("s-a".to_string(), AuditStation { owner: addr(&a), active: true });
        stations.insert("s-a2".to_string(), AuditStation { owner: addr(&a), active: false });
        stations.insert("s-p".to_string(), AuditStation { owner: addr(&p), active: true });
        stations.insert("s-q".to_string(), AuditStation { owner: "0xAbCd".repeat(1) + &"0".repeat(36), active: true });
        let txs = vec![
            (5, tx(TransactionType::Transfer, "t1", Some(&a), None)),
            (10, commit_tx("s-a", &a, "c1")),     // owner-signed
            (3_000, commit_tx("s-a", &a, "c2")),  // owner-signed, recent
            (3_001, commit_tx("s-p", &c, "c3")),  // collector, recent, owner cannot sign
            (3_002, commit_tx("s-q", &c, "c4")),  // collector, recent, owner unknown
            (100, commit_tx("s-a2", &c, "c5")),   // collector, old: not recent
        ];
        let counts = audit(&stations, txs.iter().map(|(h, t)| (*h, t)), 3_002, 2_000);
        assert_eq!(
            counts,
            AuditCounts {
                stations: 4,
                active_stations: 3,
                owners: 3,
                owners_with_ed25519_key: 1,
                owners_passkey_or_unknown: 2,
                active_stations_of_passkey_or_unknown_owners: 2,
                stations_with_recent_commits: 3,
                stations_with_recent_non_owner_commits: 2,
                of_which_owner_cannot_sign: 2,
            }
        );
    }

    #[test]
    fn a_registration_or_owner_action_key_counts_and_addresses_fold() {
        let p = key(9);
        let mut stations = BTreeMap::new();
        stations.insert("s".to_string(), AuditStation { owner: addr(&p), active: true });
        let data = serde_json::json!({ "tx_kind": "station_owner_action_v1",
            "owner_pubkey": PublicKey::from_private_key(&p).to_hex() }).to_string();
        let txs = vec![(1, tx(TransactionType::StationRegistration, "r", None, Some(data)))];
        let counts = audit(&stations, txs.iter().map(|(h, t)| (*h, t)), 1, 10);
        assert_eq!((counts.owners_with_ed25519_key, counts.owners_passkey_or_unknown), (1, 0));
        assert_eq!(normalize("0xABCD"), "abcd");
        assert_eq!(normalize("wrfabcd"), "abcd");
    }
}
