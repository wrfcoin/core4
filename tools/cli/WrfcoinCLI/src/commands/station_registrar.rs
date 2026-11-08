//! `wrfcoin station registrar-sign`: the signer the testnet station registrar
//! service (infra `testnet/services/station-registrar`) calls.
//!
//! It builds exactly one v2 registration for a NEW phone station, authorized
//! by a pinned `station_registrar_key` key instead of the governance key, and
//! refuses anything the chain's registrar rule would not apply:
//!
//! - the owner proof is checked and must be bound to the address (the same
//!   `prepare_registration` path as `build-registration`);
//! - the row must pass `consensus::station_registrar::registrar_shape_refusal`,
//!   the chain's own rule function, called here as is;
//! - after signing, the tx must pass
//!   `Transaction::verify_station_registrar_authorization_with_domain`, the
//!   consensus predicate over the keys this build pins for `--genesis-hash`.
//!
//! `--check` reads no key: it runs the first two and answers. With
//! `--key-file` the key is read only after both passed. Nothing goes to a node.
//! Testnet only: the online registrar key is a software key.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::DateTime;
use serde::Deserialize;
use serde_json::{Value, json};

use wrfcoin_cli::constants::MAX_STATION_ARTIFACT_BYTES;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::consensus::station_registrar::registrar_shape_refusal;
use wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature;
use wrfcoin_shared_protocol::crypto::domain_signing::is_authorized_oracle_governance;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::governance::schedule_extension::is_canonical_genesis_hash;

use super::station::{
    RegistrationRow, STATION_ARTIFACT_VERSION, atomic_artifact_write, prepare_registration,
    read_key,
};

/// The one network a software registrar key may sign for.
pub const REGISTRAR_NETWORK: &str = "testnet";

/// The row the registrar service sends: the phone's own export, every field
/// explicit (no defaults), nothing else. `submitted_at` is stamped by this
/// command, elevation is never allowed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistrarRow {
    station_id: String,
    owner_address: String,
    owner_pubkey: String,
    owner_signature: String,
    latitude: f64,
    longitude: f64,
    hardware_model: String,
    provenance_class: String,
    data_quality_tier: String,
}

pub(crate) struct RegistrarSignArgs<'a> {
    pub row_file: &'a Path,
    pub chain_id: &'a str,
    pub network_id: &'a str,
    pub genesis_hash: &'a str,
    pub expected_nonce: u64,
    pub check: bool,
    pub key_file: Option<&'a PathBuf>,
    pub submitted_at: Option<i64>,
    pub out: Option<&'a PathBuf>,
}

pub(crate) fn registrar_sign(args: RegistrarSignArgs<'_>) -> Result<Value> {
    if args.network_id != REGISTRAR_NETWORK {
        return Err(anyhow!(
            "station registrar-sign signs for network {REGISTRAR_NETWORK} only, not {}",
            args.network_id
        ));
    }
    if args.expected_nonce != 0 {
        return Err(anyhow!(
            "the registrar registers new stations only: --expected-nonce must be 0, not {}",
            args.expected_nonce
        ));
    }
    let genesis = args.genesis_hash.trim();
    if !is_canonical_genesis_hash(genesis) {
        return Err(anyhow!(
            "--genesis-hash must be the chain's block-1 hash, 64 lowercase hex"
        ));
    }
    // A chain instance with no pinned registrar key can never apply one.
    if !ConsensusFeature::StationRegistrarKey.schedulable(Some(genesis)) {
        return Err(anyhow!(
            "this build pins no station registrar key for chain instance {genesis}; \
             nothing it signs could apply there"
        ));
    }
    let (check, key_file, submitted_at, out) =
        (args.check, args.key_file, args.submitted_at, args.out);
    if check && (key_file.is_some() || submitted_at.is_some() || out.is_some()) {
        return Err(anyhow!(
            "--check reads no key and writes nothing: drop --key-file, --submitted-at and --out"
        ));
    }
    let raw = fs::read_to_string(args.row_file)
        .with_context(|| format!("cannot read row file {}", args.row_file.display()))?;
    let row: RegistrarRow =
        serde_json::from_str(&raw).map_err(|e| anyhow!("the row is not a registrar row: {e}"))?;

    let submitted_at = if check {
        // Only for the timestamp validity check in `prepare_registration`;
        // nothing is signed or written in this mode.
        1
    } else {
        submitted_at.ok_or_else(|| anyhow!("--submitted-at is required to sign"))?
    };
    let registration = RegistrationRow {
        station_id: row.station_id,
        owner_address: row.owner_address,
        owner_pubkey: Some(row.owner_pubkey),
        owner_signature: Some(row.owner_signature),
        latitude: row.latitude,
        longitude: row.longitude,
        elevation_meters: None,
        hardware_model: row.hardware_model,
        provenance_class: row.provenance_class,
        data_quality_tier: row.data_quality_tier,
        submitted_at,
        expected_nonce: Some(0),
    };
    // Owner proof verified and bound to the address, or refused.
    let prepared = prepare_registration(&registration, None, args.chain_id, args.network_id)?;
    // The chain's own rule, on the exact payload that would be signed. The
    // f64 form: the fixed-point form is not built here.
    if let Some(reason) = registrar_shape_refusal(&prepared.payload, false) {
        return Err(anyhow!("outside the station registrar limits: {reason}"));
    }
    let station_id = prepared.payload.station_id.clone();
    if check {
        return Ok(json!({
            "check": true,
            "station_id": station_id,
            "owner_address": prepared.payload.owner_address,
            "owner_proof": "valid",
            "registrar_limits": "inside",
            "expected_nonce": 0,
        }));
    }

    let key_file = key_file.ok_or_else(|| anyhow!("--key-file is required to sign"))?;
    let out = out.ok_or_else(|| anyhow!("--out is required to sign"))?;
    // Read only now, after every rule passed.
    let key = read_key(key_file)?;
    let registrar_pubkey = PublicKey::from_private_key(&key).to_hex();
    if is_authorized_oracle_governance(&registrar_pubkey) {
        return Err(anyhow!(
            "this key is a governance key; the registrar signer never signs with one"
        ));
    }
    let mut tx = Transaction::new_station_registration_with_domain(
        args.chain_id,
        args.network_id,
        prepared.payload,
        &key,
    );
    drop(key);
    // Reproducible: the declared time, not the wall clock, then the id again.
    tx.timestamp = DateTime::from_timestamp(prepared.submitted_at, 0)
        .ok_or_else(|| anyhow!("submitted_at is outside the supported timestamp range"))?;
    tx.set_id();
    // The consensus predicate over this build's pins, on the signed bytes.
    if !tx.verify_station_registrar_authorization_with_domain(
        args.chain_id,
        args.network_id,
        genesis,
    ) {
        return Err(anyhow!(
            "this key is not a station registrar key pinned for chain instance {genesis}; \
             nothing written"
        ));
    }
    let transaction_id = tx.id.map(|id| id.to_hex());
    let artifact = json!({
        "version": STATION_ARTIFACT_VERSION,
        "chain_id": args.chain_id,
        "network_id": args.network_id,
        "submitted_at": prepared.submitted_at,
        "transactions": [tx],
    });
    let rendered = serde_json::to_string_pretty(&artifact)? + "\n";
    if rendered.len() > MAX_STATION_ARTIFACT_BYTES {
        return Err(anyhow!("registrar artifact is over the launch API ceiling"));
    }
    atomic_artifact_write(out, |file| file.write_all(rendered.as_bytes()))
        .with_context(|| format!("cannot write artifact {}", out.display()))?;
    Ok(json!({
        "signed": true,
        "self_verified": true,
        "station_id": station_id,
        "transaction_id": transaction_id,
        "registrar_pubkey": registrar_pubkey,
        "artifact": out.display().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::transaction::{
        StationRegistrationPayload, station_ownership_message_with_domain,
    };
    use wrfcoin_shared_protocol::consensus::station_registrar::{
        MOBILE_STATION_SLOT, mobile_station_id,
    };
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
    use wrfcoin_shared_protocol::governance::schedule_extension::install_station_registrar_pin_for_tests;

    const CHAIN: &str = "888";
    const NETWORK: &str = "testnet";
    const NOW: i64 = 1_800_000_000;
    // Test keys only: fixed throwaway seeds.
    const REGISTRAR: u8 = 0x51;
    const SPARE: u8 = 0x52;
    const WRONG: u8 = 0x53;

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
    }
    fn pk_hex(seed: u8) -> String {
        PublicKey::from_private_key(&key(seed)).to_hex()
    }
    fn genesis() -> String {
        hex::encode([7u8; 32])
    }
    fn unpinned_genesis() -> String {
        hex::encode([8u8; 32])
    }
    /// Pin the test keys, and hold the process signing-domain lock for the
    /// test: the tx's input fingerprint folds the chain identity from the
    /// environment, which other tests in this binary change.
    fn pin() -> std::sync::MutexGuard<'static, ()> {
        install_station_registrar_pin_for_tests(&genesis(), vec![pk_hex(REGISTRAR), pk_hex(SPARE)]);
        crate::crypto::SIGNING_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn dir(tag: &str) -> Dir {
        let path = std::env::temp_dir().join(format!(
            "wrfcoin-registrar-sign-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    /// A phone row signed by `owner`, inside every registrar limit.
    fn row(owner: &PrivateKey) -> Value {
        let pk = PublicKey::from_private_key(owner);
        let owner_address = format!("0x{}", pk.to_address().trim_start_matches("wrf")); // secret-leak-ok: a public address from a test key's public key
        let station_id = mobile_station_id(&owner_address, MOBILE_STATION_SLOT).unwrap();
        let message =
            station_ownership_message_with_domain(CHAIN, NETWORK, &station_id, &owner_address);
        json!({
            "station_id": station_id,
            "owner_address": owner_address,
            "owner_pubkey": pk.to_hex(),
            "owner_signature": owner.sign(&message).to_hex(),
            "latitude": 46.8,
            "longitude": -92.1,
            "hardware_model": "phone-barometer",
            "provenance_class": "real",
            "data_quality_tier": "unverified",
        })
    }

    fn write_row(dir: &Dir, value: &Value) -> PathBuf {
        let path = dir.0.join("row.json");
        fs::write(&path, value.to_string()).unwrap();
        path
    }
    fn write_key(dir: &Dir, seed: u8) -> PathBuf {
        let path = dir.0.join(format!("k{seed}.key"));
        fs::write(&path, key(seed).to_hex()).unwrap();
        path
    }

    fn check(row_file: &Path, genesis: &str) -> Result<Value> {
        registrar_sign(RegistrarSignArgs {
            row_file,
            chain_id: CHAIN,
            network_id: NETWORK,
            genesis_hash: genesis,
            expected_nonce: 0,
            check: true,
            key_file: None,
            submitted_at: None,
            out: None,
        })
    }
    fn sign(row_file: &Path, genesis: &str, key_file: &PathBuf, out: &PathBuf) -> Result<Value> {
        registrar_sign(RegistrarSignArgs {
            row_file,
            chain_id: CHAIN,
            network_id: NETWORK,
            genesis_hash: genesis,
            expected_nonce: 0,
            check: false,
            key_file: Some(key_file),
            submitted_at: Some(NOW),
            out: Some(out),
        })
    }
    fn tx_of(out: &Path) -> Transaction {
        let artifact: Value = serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(artifact["chain_id"], CHAIN);
        assert_eq!(artifact["network_id"], NETWORK);
        assert_eq!(artifact["transactions"].as_array().unwrap().len(), 1);
        serde_json::from_value(artifact["transactions"][0].clone()).unwrap()
    }

    #[test]
    fn check_then_sign_builds_the_registration_the_rule_accepts() {
        let _domain = pin();
        let d = dir("ok");
        let owner = key(0xA1);
        let value = row(&owner);
        let row_file = write_row(&d, &value);

        let checked = check(&row_file, &genesis()).unwrap();
        assert_eq!(checked["owner_proof"], "valid");
        assert_eq!(checked["station_id"], value["station_id"]);

        for (seed, name) in [(REGISTRAR, "online"), (SPARE, "spare")] {
            let out = d.0.join(format!("{name}.json"));
            let summary = sign(&row_file, &genesis(), &write_key(&d, seed), &out).unwrap();
            assert_eq!(summary["signed"], true);
            assert_eq!(summary["registrar_pubkey"], pk_hex(seed));
            let tx = tx_of(&out);
            assert!(tx.verify_station_registrar_authorization_with_domain(
                CHAIN,
                NETWORK,
                &genesis()
            ));
            assert_eq!(tx.id, Some(tx.calculate_hash()), "txid recomputes");
            assert_eq!(tx.timestamp.timestamp(), NOW);
            assert_eq!(
                summary["transaction_id"],
                json!(tx.id.map(|id| id.to_hex()))
            );
            let p: StationRegistrationPayload = tx.parse_station_registration().unwrap();
            assert!(p.is_v2());
            assert_eq!(p.expected_nonce, Some(0));
            assert_eq!(p.auth_pubkey.as_deref(), Some(pk_hex(seed).as_str()));
            assert_eq!(p.station_id, value["station_id"].as_str().unwrap());
            assert_eq!(p.latitude.to_bits(), 46.8_f64.to_bits());
            assert!(p.elevation_meters.is_none());
            // Not a governance registration.
            assert!(!tx.verify_station_registration_authorization_with_domain(CHAIN, NETWORK));
        }
        // Reproducible: the same inputs give the same bytes.
        let again = d.0.join("again.json");
        sign(&row_file, &genesis(), &write_key(&d, REGISTRAR), &again).unwrap();
        assert_eq!(
            fs::read(d.0.join("online.json")).unwrap(),
            fs::read(again).unwrap()
        );
    }

    #[test]
    fn an_unpinned_key_or_chain_is_refused_and_nothing_is_written() {
        let _domain = pin();
        let d = dir("unpinned");
        let row_file = write_row(&d, &row(&key(0xA2)));
        let out = d.0.join("out.json");
        let wrong = sign(&row_file, &genesis(), &write_key(&d, WRONG), &out).unwrap_err();
        assert!(
            wrong
                .to_string()
                .contains("not a station registrar key pinned"),
            "{wrong}"
        );
        assert!(!out.exists());
        for result in [
            check(&row_file, &unpinned_genesis()),
            sign(
                &row_file,
                &unpinned_genesis(),
                &write_key(&d, REGISTRAR),
                &out,
            ),
        ] {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("pins no station registrar key")
            );
        }
        assert!(!out.exists());
        assert!(
            check(&row_file, "ABC")
                .unwrap_err()
                .to_string()
                .contains("block-1 hash")
        );
    }

    #[test]
    fn anything_outside_the_rule_is_refused_before_a_key_is_read() {
        let _domain = pin();
        let d = dir("shape");
        let owner = key(0xA3);
        // The key file does not exist: a refusal must come before reading it.
        let missing_key = d.0.join("no-such.key");
        let out = d.0.join("out.json");
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>, &str)> = vec![
            (
                "hardware",
                Box::new(|v| v["hardware_model"] = json!("davis-vantage")),
                "phone-barometer",
            ),
            (
                "tier",
                Box::new(|v| v["data_quality_tier"] = json!("gold")),
                "unverified",
            ),
            (
                "provenance",
                Box::new(|v| v["provenance_class"] = json!("synthetic")),
                "real",
            ),
            (
                "off grid",
                Box::new(|v| v["latitude"] = json!(46.85)),
                "0.1 degree grid",
            ),
            (
                "slot 1",
                Box::new(|v| {
                    let id = mobile_station_id(v["owner_address"].as_str().unwrap(), 1).unwrap();
                    v["station_id"] = json!(id);
                }),
                "owner signature does not verify",
            ),
            (
                "bad owner proof",
                Box::new(|v| v["owner_signature"] = json!("00".repeat(64))),
                "owner signature does not verify",
            ),
            (
                "elevation",
                Box::new(|v| v["elevation_meters"] = json!(10.0)),
                "not a registrar row",
            ),
            (
                "extra field",
                Box::new(|v| v["submitted_at"] = json!(1)),
                "not a registrar row",
            ),
            (
                "missing field",
                Box::new(|v| {
                    v.as_object_mut().unwrap().remove("longitude");
                }),
                "not a registrar row",
            ),
        ];
        for (name, mutate, expected) in cases {
            let mut value = row(&owner);
            mutate(&mut value);
            let row_file = write_row(&d, &value);
            let refused = check(&row_file, &genesis()).unwrap_err().to_string();
            assert!(refused.contains(expected), "{name}: {refused}");
            let refused = sign(&row_file, &genesis(), &missing_key, &out)
                .unwrap_err()
                .to_string();
            assert!(refused.contains(expected), "{name} (sign): {refused}");
            assert!(!out.exists(), "{name}");
        }
    }

    #[test]
    fn a_slot_1_id_with_its_own_owner_proof_is_outside_the_rule() {
        let _domain = pin();
        let d = dir("slot1");
        let owner = key(0xA4);
        let mut value = row(&owner);
        let address = value["owner_address"].as_str().unwrap().to_string();
        let id = mobile_station_id(&address, 1).unwrap();
        let message = station_ownership_message_with_domain(CHAIN, NETWORK, &id, &address);
        value["station_id"] = json!(id);
        value["owner_signature"] = json!(owner.sign(&message).to_hex());
        let refused = check(&write_row(&d, &value), &genesis())
            .unwrap_err()
            .to_string();
        assert!(refused.contains("slot-0"), "{refused}");
    }

    #[test]
    fn only_new_stations_on_testnet_and_check_writes_nothing() {
        let _domain = pin();
        let d = dir("args");
        let row_file = write_row(&d, &row(&key(0xA5)));
        let base = |nonce: u64, network: &'static str| RegistrarSignArgs {
            row_file: &row_file,
            chain_id: CHAIN,
            network_id: network,
            genesis_hash: "0707070707070707070707070707070707070707070707070707070707070707",
            expected_nonce: nonce,
            check: true,
            key_file: None,
            submitted_at: None,
            out: None,
        };
        assert!(
            registrar_sign(base(1, NETWORK))
                .unwrap_err()
                .to_string()
                .contains("new stations only")
        );
        assert!(
            registrar_sign(base(0, "mainnet"))
                .unwrap_err()
                .to_string()
                .contains("testnet only")
        );
        let key_file = write_key(&d, REGISTRAR);
        let mut with_key = base(0, NETWORK);
        with_key.key_file = Some(&key_file);
        assert!(
            registrar_sign(with_key)
                .unwrap_err()
                .to_string()
                .contains("reads no key")
        );
        assert!(registrar_sign(base(0, NETWORK)).is_ok());
    }
}
