//! Off-chain tooling for the forecast late score
//! (`shared_protocol::consensus::forecast_late_score`).
//!
//! Three steps, each a pure function apart from reading a key:
//!
//! 1. **Attest** ([`attest_observation`]): the oracle job. From the readings a
//!    station has on chain (as `wrfcoin_forecast_score::late_score::ChainSample`)
//!    and one commit block's (station, variable, lead, height, time), build the
//!    [`ObservationAttestation`] with the shared rule's own helpers: truth
//!    candidates and the nearest reading, window candidates, the 24 hourly
//!    samples, the reference, `mean24`, `last` and the window hash.
//! 2. **Sign** ([`sign_observation`]): one oracle signs the exact bytes core4
//!    verifies (`forecast_score_message`: the observation and the forecast
//!    commit ids it scores, i.e. the full score payload), and the signature is
//!    checked against the signing key before it is returned.
//! 3. **Build** ([`build_score_payload`], [`build_score_transaction`]): collect
//!    the quorum's signatures and the forecast commit ids into a
//!    `TransactionType::ForecastScore` transaction anyone may relay. The
//!    transaction pays its relay nothing and debits no account.
//!
//! Nothing here decides whether a score is valid: the node's admission and
//! consensus do. The checks here only refuse, early and locally, what the node
//! would refuse anyway.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};

use wrfcoin_forecast_score::late_score::{
    ChainSample, WINDOW_HOURS, hourly_window, last_reading_before, nearest_reading,
    reference_from_window, truth_candidates, window_candidates, window_hash,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{
    ForecastScorePayload, LateScoreVariable, MAX_FORECASTS_PER_SCORE_TX, MAX_OBSERVATION_SIGNATURES,
    MAX_WINDOW_READINGS, ObservationAttestation, ObservationSignature, forecast_score_message, score_outpoint,
    target_for,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

const SECS_PER_HOUR: i64 = 3_600;

/// The forecasts one attestation covers: one station, variable and lead,
/// committed in one block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitBlock {
    pub station_id: String,
    pub variable: LateScoreVariable,
    pub lead_hours: u32,
    pub commit_height: u64,
    pub commit_time: i64,
}

/// What the oracle job worked out, besides the attestation: enough for an
/// operator to see why it chose what it chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttestationReport {
    pub attestation: ObservationAttestation,
    /// The 24 hourly samples, oldest first.
    pub window: Vec<Option<i64>>,
    pub samples: usize,
    pub truth_candidates: Vec<String>,
    pub last_commit_id: String,
}

/// The oracle job: the observation the forecasts of `block` are scored
/// against, from the station's readings. `samples` are that station's readings
/// of `block.variable` with the block each landed in and its quorum AS OF THE
/// CLOSING BLOCK (the first block at or after target + 35 min). Refuses when
/// the rule leaves the forecasts unscored (no truth within 15 minutes, fewer
/// than 12 hourly samples, no reading before the commit, more than 512
/// window readings) and when the truth would be refused on chain.
pub fn attest_observation(samples: &[ChainSample], block: &CommitBlock) -> Result<AttestationReport> {
    let target = target_for(block.commit_time, block.lead_hours)
        .ok_or_else(|| anyhow!("lead must be 1, 6, 12 or 24 hours and commit_time non-negative"))?;
    let variable = block.variable;

    let truths = truth_candidates(samples, variable, target);
    let truth = nearest_reading(&truths, target)
        .ok_or_else(|| anyhow!("no quorum reading observed within 15 minutes of the target {target}"))?
        .clone();
    let truth_sample = samples
        .iter()
        .find(|s| s.id == truth.id)
        .ok_or_else(|| anyhow!("truth reading {} is not in the input", truth.id))?;
    ensure!(
        truth_sample.height > block.commit_height,
        "truth reading {} landed at height {}, not above the commit height {}",
        truth.id,
        truth_sample.height,
        block.commit_height
    );

    let candidates = window_candidates(samples, variable, block.commit_time, block.commit_height);
    let window = hourly_window(&candidates, block.commit_time).map_err(|e| anyhow!("window: {e}"))?;
    let lag = usize::try_from(block.lead_hours).map_err(|_| anyhow!("lead"))?;
    let reference = reference_from_window(&window, lag)
        .map_err(|e| anyhow!("reference: {e}"))?
        .ok_or_else(|| {
            anyhow!(
                "only {} hourly samples in the 24 h before the commit; 12 are needed",
                window.iter().flatten().count()
            )
        })?;
    let last = last_reading_before(&candidates, block.commit_time)
        .ok_or_else(|| anyhow!("no reading in the 24 h before the commit"))?
        .clone();

    // The readings the window and `last` were built from: the candidates
    // observed in [commit - 24 h, commit), in byte order, once each.
    let start = block.commit_time - WINDOW_HOURS as i64 * SECS_PER_HOUR;
    let used: BTreeSet<&str> = candidates
        .iter()
        .filter(|r| r.observed_at >= start && r.observed_at < block.commit_time)
        .map(|r| r.id.as_str())
        .collect();
    ensure!(
        used.len() <= MAX_WINDOW_READINGS,
        "{} window readings; an attestation cites at most {MAX_WINDOW_READINGS}",
        used.len()
    );
    ensure!(
        !used.contains(truth.id.as_str()),
        "truth reading {} is also a window reading",
        truth.id
    );
    let mut window_ids: Vec<String> = used.into_iter().map(str::to_string).collect();
    window_ids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));

    let attestation = ObservationAttestation {
        station_id: block.station_id.clone(),
        variable,
        lead_hours: block.lead_hours,
        commit_height: block.commit_height,
        commit_time: block.commit_time,
        target,
        truth: truth.value,
        truth_commit_id: truth.id.clone(),
        reference: reference.reference,
        mean24: reference.mean24,
        last: last.value,
        window_hash: hex::encode(window_hash(&window).map_err(|e| anyhow!("window hash: {e}"))?),
        window_commit_ids: window_ids,
    };
    attestation
        .validate()
        .map_err(|e| anyhow!("attestation refused before signing: {e}"))?;
    Ok(AttestationReport {
        attestation,
        window: window.to_vec(),
        samples: reference.samples,
        truth_candidates: truths.into_iter().map(|r| r.id).collect(),
        last_commit_id: last.id,
    })
}

/// The oracle identity a key signs as when none is given: its public key hex.
pub fn default_oracle_identity(key: &PrivateKey) -> String {
    PublicKey::from_private_key(key).to_hex()
}

/// Forecast commit ids as a score lists (and signs) them: trimmed, sorted in
/// byte order, each once.
pub fn normalize_forecast_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = ids.into_iter().map(|id| id.trim().to_string()).collect();
    set.into_iter().collect()
}

/// One oracle's signature over the full score payload on (`chain_id`,
/// `network_id`): `attestation` and the forecast commit ids it scores
/// (normalized as the payload lists them). `oracle` is the identity exactly
/// as the chain's oracle set holds it. The attestation is shape-checked first
/// and the signature verified after.
pub fn sign_observation(
    key: &PrivateKey,
    oracle: &str,
    chain_id: &str,
    network_id: &str,
    attestation: &ObservationAttestation,
    forecast_commit_ids: &[String],
) -> Result<ObservationSignature> {
    ensure!(!chain_id.trim().is_empty() && !network_id.trim().is_empty(), "chain id and network id are required");
    attestation
        .validate()
        .map_err(|e| anyhow!("attestation refused before signing: {e}"))?;
    let ids = normalize_forecast_ids(forecast_commit_ids.iter().cloned());
    ensure!(!ids.is_empty(), "a score signs at least one forecast commit id");
    let message = forecast_score_message(chain_id, network_id, attestation, &ids);
    let signature = key.sign(&message);
    let verifies = PublicKey::from_private_key(key).verify(&message, &signature);
    ensure!(verifies, "signature does not verify against the signing key");
    Ok(ObservationSignature { oracle: oracle.to_string(), ed25519: signature.to_hex() })
}

/// Whether `signature` is `public_key`'s signature over this score payload.
pub fn verify_observation_signature(
    public_key: &PublicKey,
    chain_id: &str,
    network_id: &str,
    attestation: &ObservationAttestation,
    forecast_commit_ids: &[String],
    signature: &ObservationSignature,
) -> bool {
    let message = forecast_score_message(chain_id, network_id, attestation, forecast_commit_ids);
    Signature::from_hex(signature.ed25519.trim()).is_ok_and(|s| public_key.verify(&message, &s))
}

/// The score payload: signatures sorted by oracle (one each), forecast commit
/// ids sorted and deduplicated, and the whole payload shape-checked.
pub fn build_score_payload(
    attestation: ObservationAttestation,
    mut signatures: Vec<ObservationSignature>,
    forecast_commit_ids: impl IntoIterator<Item = String>,
) -> Result<ForecastScorePayload> {
    signatures.sort_by(|a, b| a.oracle.as_bytes().cmp(b.oracle.as_bytes()));
    if let Some(pair) = signatures.windows(2).find(|p| p[0].oracle == p[1].oracle) {
        bail!("oracle {} signed twice", pair[0].oracle);
    }
    ensure!(
        signatures.len() <= MAX_OBSERVATION_SIGNATURES,
        "{} signatures; a score carries at most {MAX_OBSERVATION_SIGNATURES}",
        signatures.len()
    );
    let ids = normalize_forecast_ids(forecast_commit_ids);
    ensure!(
        ids.len() <= MAX_FORECASTS_PER_SCORE_TX,
        "{} forecasts; a score lists at most {MAX_FORECASTS_PER_SCORE_TX}",
        ids.len()
    );
    let payload = ForecastScorePayload { attestation, signatures, forecast_commit_ids: ids };
    payload.validate().map_err(|e| anyhow!("score payload refused: {e}"))?;
    Ok(payload)
}

/// The relayable `ForecastScore` transaction for `payload`. `relay` is only a
/// label (nothing is debited or paid to it). The one input is the outpoint
/// core4 derives from the signed payload (`score_outpoint`), which mempool
/// admission requires: one pending transaction per signed payload.
pub fn build_score_transaction(
    payload: &ForecastScorePayload,
    chain_id: &str,
    network_id: &str,
    relay: Address,
    submitted_at: i64,
) -> Result<Transaction> {
    payload.validate().map_err(|e| anyhow!("score payload refused: {e}"))?;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: score_outpoint(chain_id, network_id, payload),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput { amount: 0, address: relay.to_string(), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = TransactionType::ForecastScore;
    tx.sender_address = relay;
    tx.timestamp = chrono::DateTime::from_timestamp(submitted_at, 0)
        .ok_or_else(|| anyhow!("{submitted_at} is not a valid unix time"))?;
    tx.data = Some(serde_json::to_string(payload)?);
    tx.set_id();
    ensure!(
        ForecastScorePayload::parse(&tx).as_ref() == Some(payload),
        "built transaction does not parse back"
    );
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const CHAIN: &str = "wrfcoin-testnet";
    const NET: &str = "testnet";

    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn golden_case() -> Value {
        let raw = std::fs::read_to_string(root().join("shared-protocol/tests/fixtures/golden/signing_preimage_v2.json"))
            .expect("golden vectors");
        let all: Value = serde_json::from_str(&raw).unwrap();
        all["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "forecast_score_attestation_v2")
            .expect("forecast observation case")
            .clone()
    }

    fn rule3() -> Vec<Value> {
        let raw = std::fs::read_to_string(root().join("forecast-score/vectors/forecast-skill-vectors.json"))
            .expect("skill vectors");
        let all: Value = serde_json::from_str(&raw).unwrap();
        all["rule3_scenarios"].as_array().unwrap().clone()
    }

    #[test]
    fn signing_reproduces_the_golden_vector_and_a_tamper_fails() {
        let case = golden_case();
        let key = PrivateKey::from_hex(case["signer_secret_hex"].as_str().unwrap()).unwrap();
        let input = &case["input"];
        let (chain, net) = (input["chain_id"].as_str().unwrap(), input["network_id"].as_str().unwrap());
        let attestation: ObservationAttestation = serde_json::from_value(input["attestation"].clone()).unwrap();
        let ids: Vec<String> = serde_json::from_value(input["forecast_commit_ids"].clone()).unwrap();
        assert_eq!(
            hex::encode(forecast_score_message(chain, net, &attestation, &ids)),
            case["preimage_hex"].as_str().unwrap()
        );
        let signed = sign_observation(&key, "o1", chain, net, &attestation, &ids).unwrap();
        assert_eq!(signed.ed25519, case["signature_hex"].as_str().unwrap(), "ed25519 is deterministic");
        assert_eq!(default_oracle_identity(&key), case["signer_public_key_hex"].as_str().unwrap());
        let public = PublicKey::from_private_key(&key);
        assert!(verify_observation_signature(&public, chain, net, &attestation, &ids, &signed));
        // The tampered twin keeps the observation and changes the forecast list.
        let tampered_ids: Vec<String> =
            serde_json::from_value(case["tampered_input"]["forecast_commit_ids"].clone()).unwrap();
        assert_ne!(tampered_ids, ids);
        assert!(!verify_observation_signature(&public, chain, net, &attestation, &tampered_ids, &signed));
        let mut changed = attestation.clone();
        changed.truth += 1;
        assert!(!verify_observation_signature(&public, chain, net, &changed, &ids, &signed), "value-bound");
        assert!(!verify_observation_signature(&public, "acr888", net, &attestation, &ids, &signed), "chain-bound");
        assert!(!verify_observation_signature(&public, chain, "devnet", &attestation, &ids, &signed), "network-bound");
        // Signing normalizes the list as the payload lists it.
        let mut shuffled = ids.clone();
        shuffled.reverse();
        shuffled.push(ids[0].clone());
        assert_eq!(sign_observation(&key, "o1", chain, net, &attestation, &shuffled).unwrap(), signed);
    }

    #[test]
    fn a_malformed_attestation_is_never_signed() {
        let case = golden_case();
        let key = PrivateKey::from_hex(case["signer_secret_hex"].as_str().unwrap()).unwrap();
        let mut attestation: ObservationAttestation =
            serde_json::from_value(case["input"]["attestation"].clone()).unwrap();
        attestation.target += 1;
        assert!(sign_observation(&key, "o1", CHAIN, NET, &attestation, &["f1".to_string()]).is_err());
    }

    fn full_day() -> (Vec<ChainSample>, CommitBlock) {
        let case = rule3().into_iter().find(|c| c["name"] == "a full day: 24 hourly samples and a truth").unwrap();
        let samples: Vec<ChainSample> = serde_json::from_value(case["readings"].clone()).unwrap();
        let block = CommitBlock {
            station_id: "st1".into(),
            variable: serde_json::from_value(case["variable"].clone()).unwrap(),
            lead_hours: case["lead_hours"].as_u64().unwrap() as u32,
            commit_height: case["commit_height"].as_u64().unwrap(),
            commit_time: case["commit_time"].as_i64().unwrap(),
        };
        (samples, block)
    }

    #[test]
    fn the_oracle_job_matches_the_rule3_vectors() {
        for case in rule3() {
            let samples: Vec<ChainSample> = serde_json::from_value(case["readings"].clone()).unwrap();
            let block = CommitBlock {
                station_id: "st1".into(),
                variable: serde_json::from_value(case["variable"].clone()).unwrap(),
                lead_hours: case["lead_hours"].as_u64().unwrap() as u32,
                commit_height: case["commit_height"].as_u64().unwrap(),
                commit_time: case["commit_time"].as_i64().unwrap(),
            };
            let expect = &case["expect"];
            let expected_window: Vec<Option<i64>> = serde_json::from_value(expect["window"].clone()).unwrap();
            let samples_in_window = expected_window.iter().flatten().count();
            match attest_observation(&samples, &block) {
                Ok(report) => {
                    assert_eq!(report.attestation.truth_commit_id, expect["truth"].as_str().unwrap(), "{case}");
                    assert_eq!(report.window, expected_window, "{case}");
                    assert_eq!(report.last_commit_id, expect["last"].as_str().unwrap(), "{case}");
                    let candidates: Vec<String> = serde_json::from_value(expect["truth_candidates"].clone()).unwrap();
                    assert_eq!(report.truth_candidates, candidates, "{case}");
                    assert_eq!(
                        report.attestation.window_hash,
                        hex::encode(window_hash(&expected_window).unwrap()),
                        "{case}"
                    );
                },
                Err(error) => assert!(
                    expect["truth"].is_null() || samples_in_window < 12 || expect["last"].is_null(),
                    "{} refused: {error}",
                    case["name"]
                ),
            }
        }
    }

    #[test]
    fn a_full_day_attests_and_builds_a_relayable_score() {
        let (samples, block) = full_day();
        let report = attest_observation(&samples, &block).expect("a full day is attestable");
        let a = &report.attestation;
        assert_eq!(report.samples, 24);
        assert_eq!(a.window_commit_ids.len(), 24);
        assert!(a.window_commit_ids.windows(2).all(|p| p[0].as_bytes() < p[1].as_bytes()));
        assert_eq!(a.target, block.commit_time + 6 * 3_600);
        let reference = reference_from_window(&report.window, 6).unwrap().unwrap();
        assert_eq!((a.reference, a.mean24), (reference.reference, reference.mean24));
        assert_eq!(a.last, 1690);

        let keys: Vec<PrivateKey> = (1u8..=3).map(|s| PrivateKey::from_bytes(&[s; 32]).unwrap()).collect();
        // Signed out of order on purpose: the builder sorts by oracle.
        let signatures: Vec<ObservationSignature> = keys
            .iter()
            .rev()
            .map(|k| {
                sign_observation(k, &default_oracle_identity(k), CHAIN, NET, a, &["f2".to_string(), "f1".to_string()])
                    .unwrap()
            })
            .collect();
        let payload =
            build_score_payload(a.clone(), signatures.clone(), ["f2".to_string(), "f1".into(), "f2".into()]).unwrap();
        assert_eq!(payload.forecast_commit_ids, ["f1", "f2"]);
        assert!(payload.signatures.windows(2).all(|p| p[0].oracle < p[1].oracle));
        let relay = Address([0x77; 20]);
        let tx = build_score_transaction(&payload, CHAIN, NET, relay, 1_800_200_000).unwrap();
        let again = build_score_transaction(&payload, CHAIN, NET, relay, 1_800_200_000).unwrap();
        assert_eq!(tx.id, again.id, "reproducible");
        assert_eq!(tx.tx_type, TransactionType::ForecastScore);
        assert_eq!(tx.inputs[0].txid, again.inputs[0].txid);
        let later = build_score_transaction(&payload, CHAIN, NET, Address([0x78; 20]), 1_800_200_001).unwrap();
        assert_eq!(tx.inputs[0].txid, later.inputs[0].txid, "same score, same outpoint: relays conflict");
        assert_eq!(tx.inputs[0].txid, score_outpoint(CHAIN, NET, &payload), "core4's derived outpoint");
        let all_verify = payload.signatures.iter().all(|sig| keys.iter().any(|k| verify_observation_signature(
            &PublicKey::from_private_key(k),
            CHAIN,
            NET,
            a,
            &payload.forecast_commit_ids,
            sig
        )));
        assert!(all_verify);
        assert_eq!(ForecastScorePayload::parse(&tx).unwrap(), payload);

        let mut twice = signatures.clone();
        twice.push(signatures[0].clone());
        assert!(build_score_payload(a.clone(), twice, ["f1".to_string()]).is_err());
        assert!(build_score_payload(a.clone(), signatures, Vec::<String>::new()).is_err());
    }

    #[test]
    fn the_oracle_job_refuses_what_the_rule_leaves_unscored() {
        let (samples, block) = full_day();
        // Too few hourly samples.
        let thin: Vec<ChainSample> =
            samples.iter().filter(|s| s.id == "t" || s.id.as_str() < "h11").cloned().collect();
        let error = attest_observation(&thin, &block).unwrap_err().to_string();
        assert!(error.contains("12 are needed"), "{error}");
        // No truth.
        let no_truth: Vec<ChainSample> = samples.iter().filter(|s| s.id != "t").cloned().collect();
        assert!(attest_observation(&no_truth, &block).is_err());
        // A lead the chain does not score.
        let mut bad_lead = block.clone();
        bad_lead.lead_hours = 3;
        assert!(attest_observation(&samples, &bad_lead).is_err());
    }
}
