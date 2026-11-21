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
//!    samples, the reference, `mean24`, `last` and the window hash. The
//!    command first drops forecast-type reveals ([`weather_readings_only`]).
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
    ChainSample, WINDOW_HOURS, hourly_window_over, last_reading_before, nearest_reading,
    reference_for_lead, truth_candidates, window_candidates_over, window_hash,
    window_hours_for_lead,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{
    ForecastScorePayload, LateScoreVariable, MAX_FORECASTS_PER_SCORE_TX, MAX_OBSERVATION_SIGNATURES,
    LEAD_HOURS, MAX_WINDOW_READINGS, ObservationAttestation, ObservationSignature, forecast_score_message, score_outpoint,
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
    /// The 48 hourly samples, oldest first.
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
        .ok_or_else(|| anyhow!("lead must be 6 or 12 hours and commit_time non-negative"))?;
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

    // 48 hourly slots: the reference has a same-hour-yesterday term.
    let hours = window_hours_for_lead(block.lead_hours);
    let candidates =
        window_candidates_over(samples, variable, block.commit_time, block.commit_height, hours);
    let window =
        hourly_window_over(&candidates, block.commit_time, hours).map_err(|e| anyhow!("window: {e}"))?;
    let reference = reference_for_lead(&window, block.lead_hours)
        .map_err(|e| anyhow!("reference: {e}"))?
        .ok_or_else(|| {
            anyhow!(
                "only {} hourly samples in the 24 h before the commit; 12 are needed",
                window[window.len() - WINDOW_HOURS..].iter().flatten().count()
            )
        })?;
    let last = last_reading_before(&candidates, block.commit_time)
        .ok_or_else(|| anyhow!("no reading in the 24 h before the commit"))?
        .clone();

    // The readings the window and `last` were built from: the candidates
    // observed in [commit - window, commit), in byte order, once each.
    let start = block.commit_time - hours as i64 * SECS_PER_HOUR;
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

/// One station reading as the oracle job reads it: the chain sample plus the
/// reveal's `contribution_type` exactly as the chain holds it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TypedSample {
    #[serde(flatten)]
    pub sample: ChainSample,
    /// The reveal's contribution type (`"forecast"` for a forecast reveal).
    #[serde(default)]
    pub contribution_type: Option<String>,
}

/// What [`weather_readings_only`] kept and left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SampleFilter {
    pub kept: Vec<ChainSample>,
    /// Commit ids of forecast-type reveals that were left out.
    pub dropped_forecast_type: Vec<String>,
    /// Kept samples that carried no contribution type (only with `allow_untyped`).
    pub untyped: usize,
}

/// Keep only station readings: a forecast-type reveal is never a reading,
/// even when it carries weather fields. A forecast reveal applied before
/// `forecast_late_score_v2` was in force may hold a quality score, and the
/// chain would then accept it as a cited reading for up to 10,000 blocks
/// (FINDINGS-2 N4), so the oracle job must not cite one. The type is compared
/// trimmed and without case, so a near-miss spelling is left out too (leaving
/// a reading out can only thin the window, never make a score invalid).
///
/// A sample without a contribution type is refused unless `allow_untyped`:
/// the oracle job cannot tell it apart from a forecast.
pub fn weather_readings_only(samples: Vec<TypedSample>, allow_untyped: bool) -> Result<SampleFilter> {
    let mut kept = Vec::with_capacity(samples.len());
    let mut dropped_forecast_type = Vec::new();
    let mut untyped = Vec::new();
    for TypedSample { sample, contribution_type } in samples {
        match contribution_type.as_deref().map(str::trim) {
            Some(kind) if kind.eq_ignore_ascii_case("forecast") => dropped_forecast_type.push(sample.id),
            Some(kind) if !kind.is_empty() => kept.push(sample),
            _ => {
                untyped.push(sample.id.clone());
                kept.push(sample);
            },
        }
    }
    if !untyped.is_empty() && !allow_untyped {
        bail!(
            "{} sample(s) have no contribution_type (first: {}); give each reading the reveal's \
             contribution_type so forecast reveals are left out, or pass --allow-untyped",
            untyped.len(),
            untyped[0]
        );
    }
    Ok(SampleFilter { kept, dropped_forecast_type, untyped: untyped.len() })
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
            // The shared vectors also cover the 1 h and 24 h leads (the backend's
            // practice score); the chain pays neither, so the oracle job
            // refuses them.
            if !LEAD_HOURS.contains(&block.lead_hours) {
                let error = attest_observation(&samples, &block).unwrap_err().to_string();
                assert!(error.contains("lead must be"), "{error}");
                continue;
            }
            let expect = &case["expect"];
            let expected_window: Vec<Option<i64>> = serde_json::from_value(expect["window"].clone()).unwrap();
            let samples_in_window = expected_window.iter().flatten().count();
            match attest_observation(&samples, &block) {
                Ok(report) => {
                    assert_eq!(report.attestation.truth_commit_id, expect["truth"].as_str().unwrap(), "{case}");
                    // The vectors pin the 24 h window: the newest 24 of the 48.
                    assert_eq!(report.window[24..], expected_window[..], "{case}");
                    assert_eq!(report.last_commit_id, expect["last"].as_str().unwrap(), "{case}");
                    let candidates: Vec<String> = serde_json::from_value(expect["truth_candidates"].clone()).unwrap();
                    assert_eq!(report.truth_candidates, candidates, "{case}");
                    assert_eq!(
                        report.attestation.window_hash,
                        hex::encode(window_hash(&report.window).unwrap()),
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
        let reference = reference_for_lead(&report.window, 6).unwrap().unwrap();
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
        // The 1 h and 24 h leads are not scored.
        for lead in [1, 24] {
            bad_lead.lead_hours = lead;
            let error = attest_observation(&samples, &bad_lead).unwrap_err().to_string();
            assert!(error.contains("lead must be"), "{lead}: {error}");
        }
    }

    /// Two days of hourly readings ending just before `commit_time`, all
    /// landed below the commit height, and a truth reading at the target
    /// (`commit_time + lead_hours`).
    fn two_days(commit_time: i64, commit_height: u64, lead_hours: u32, value: impl Fn(usize) -> i64) -> Vec<ChainSample> {
        let mut samples: Vec<ChainSample> = (0..48)
            .map(|slot| {
                let observed_at = commit_time - (48 - slot as i64) * 3_600 + 600;
                ChainSample {
                    id: format!("w{slot:02}"),
                    observed_at,
                    value: value(slot),
                    height: commit_height - 100 + slot as u64,
                    block_time: observed_at + 60,
                    has_quorum: true,
                    provenance: None,
                }
            })
            .collect();
        let target = commit_time + i64::from(lead_hours) * 3_600;
        samples.push(ChainSample {
            id: "t".into(),
            observed_at: target,
            value: value(72),
            height: commit_height + 1_200,
            block_time: target + 60,
            has_quorum: true,
            provenance: None,
        });
        samples
    }

    fn typed(sample: &ChainSample, kind: Option<&str>) -> TypedSample {
        TypedSample { sample: sample.clone(), contribution_type: kind.map(str::to_string) }
    }

    #[test]
    fn forecast_type_reveals_are_never_cited_as_readings() {
        let commit_time = 1_800_000_000 - 1_800_000_000 % 3_600;
        let commit_height = 5_000;
        let samples = two_days(commit_time, commit_height, 6, |slot| 2_000 + slot as i64);
        let block = CommitBlock {
            station_id: "st1".into(),
            variable: LateScoreVariable::TemperatureC,
            lead_hours: 6,
            commit_height,
            commit_time,
        };
        // A forecast-type reveal with weather fields sits in the window, one
        // in a near-miss spelling, and one claims to be the truth.
        let mut input: Vec<TypedSample> = samples.iter().map(|s| typed(s, Some("weather"))).collect();
        let mut forecast = samples[47].clone();
        forecast.id = "f-window".into();
        forecast.observed_at -= 60;
        input.push(typed(&forecast, Some("forecast")));
        let mut shouted = samples[40].clone();
        shouted.id = "f-shout".into();
        input.push(typed(&shouted, Some(" Forecast ")));
        let mut fake_truth = samples[48].clone();
        fake_truth.id = "f-truth".into();
        input.push(typed(&fake_truth, Some("forecast")));

        let filtered = weather_readings_only(input.clone(), false).unwrap();
        assert_eq!(filtered.dropped_forecast_type, vec!["f-window", "f-shout", "f-truth"]);
        assert_eq!(filtered.kept, samples);
        assert_eq!(filtered.untyped, 0);
        let report = attest_observation(&filtered.kept, &block).unwrap();
        let cited: Vec<&str> = report
            .attestation
            .window_commit_ids
            .iter()
            .map(String::as_str)
            .chain([report.attestation.truth_commit_id.as_str()])
            .collect();
        assert!(cited.iter().all(|id| !id.starts_with("f-")), "{cited:?}");

        // Without the filter the forecast reveal would have been cited.
        let unfiltered: Vec<ChainSample> = input.into_iter().map(|t| t.sample).collect();
        let report = attest_observation(&unfiltered, &block).unwrap();
        assert!(report.attestation.window_commit_ids.iter().any(|id| id == "f-window"));
    }

    #[test]
    fn an_untyped_sample_is_refused_unless_allowed() {
        let samples = two_days(1_800_000_000 - 1_800_000_000 % 3_600, 5_000, 6, |_| 2_000);
        let mut input: Vec<TypedSample> = samples.iter().map(|s| typed(s, Some("weather"))).collect();
        input[3].contribution_type = None;
        input[5].contribution_type = Some("  ".into());
        let error = weather_readings_only(input.clone(), false).unwrap_err().to_string();
        assert!(error.contains("2 sample(s)") && error.contains("w03"), "{error}");
        let allowed = weather_readings_only(input, true).unwrap();
        assert_eq!((allowed.kept.len(), allowed.untyped), (samples.len(), 2));
    }

    #[test]
    fn the_samples_file_reads_the_contribution_type_next_to_the_sample() {
        let raw = serde_json::json!([
            {"id": "a", "observed_at": 1, "value": 2, "height": 3, "block_time": 4, "has_quorum": true,
             "provenance": null, "contribution_type": "forecast"},
            {"id": "b", "observed_at": 1, "value": 2, "height": 3, "block_time": 4, "has_quorum": true,
             "provenance": "device"}
        ]);
        let parsed: Vec<TypedSample> = serde_json::from_value(raw).unwrap();
        assert_eq!(parsed[0].contribution_type.as_deref(), Some("forecast"));
        assert_eq!(parsed[0].sample.id, "a");
        assert_eq!(parsed[1].contribution_type, None);
        assert_eq!(parsed[1].sample.provenance.as_deref(), Some("device"));
    }

    #[test]
    fn the_paid_leads_are_attested_from_a_48_hour_window() {
        let commit_time = 1_800_000_000 - 1_800_000_000 % 3_600;
        let commit_height = 5_000;
        let daily = |slot: usize| {
            let hour = (slot % 24) as i64;
            2_000 + 80 * if hour < 12 { hour } else { 24 - hour } + (slot as i64 * 7) % 5
        };
        for lead in [6u32, 12] {
            let samples = two_days(commit_time, commit_height, lead, daily);
            let block = CommitBlock {
                station_id: "st1".into(),
                variable: LateScoreVariable::TemperatureC,
                lead_hours: lead,
                commit_height,
                commit_time,
            };
            let report = attest_observation(&samples, &block).expect("two full days are attestable");
            let a = &report.attestation;
            assert_eq!(report.window.len(), 48, "{lead}");
            assert_eq!(a.window_commit_ids.len(), 48, "both days are cited");
            assert_eq!(a.window_hash, hex::encode(window_hash(&report.window).unwrap()));
            let reference = reference_for_lead(&report.window, lead).unwrap().unwrap();
            assert_eq!((a.reference, a.mean24), (reference.reference, reference.mean24));
            assert_eq!(reference.pairs, 24);
            // The lag-24 term is far below the one-day rule (spread or lag-lead
            // persistence) on a daily cycle.
            let old = wrfcoin_forecast_score::late_score::reference_from_window(&report.window[24..], lead as usize)
                .unwrap()
                .unwrap();
            assert!(a.reference < old.reference / 10, "{lead}: {} vs {}", a.reference, old.reference);
            assert_eq!(a.mean24, old.mean24);
            assert_eq!(a.last, daily(47));

            // Only one day of history: the lag-24 term has no pairs, so the
            // lead falls back to the one-day rule (still scored).
            let one_day: Vec<ChainSample> =
                samples.iter().filter(|s| s.id == "t" || s.id.as_str() >= "w24").cloned().collect();
            let thin = attest_observation(&one_day, &block).unwrap();
            assert_eq!(thin.attestation.reference, old.reference, "{lead}");
        }

        // The 24 h lead is not paid, so the oracle job refuses it even with
        // two full days of readings and a truth at its target.
        let samples = two_days(commit_time, commit_height, 24, daily);
        let block = CommitBlock {
            station_id: "st1".into(),
            variable: LateScoreVariable::TemperatureC,
            lead_hours: 24,
            commit_height,
            commit_time,
        };
        let error = attest_observation(&samples, &block).unwrap_err().to_string();
        assert!(error.contains("lead must be"), "{error}");
    }
}
