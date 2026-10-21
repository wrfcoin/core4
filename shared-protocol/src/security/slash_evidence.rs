//! Bounded, chain-bound physical-range evidence wire type (Refs #3398).
//! This slice defines signed bytes only; production dispatch remains disabled.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction,
    commit_authorization_message_with_domain,
};
use crate::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, SigningProtocolVersion, feature_active,
};
use crate::crypto::domain_signing::{DomainSeparated, domain_preimage};
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};

pub const MAX_REPORT_BYTES: usize = 16 * 1024;
pub const MAX_REVEAL_BYTES: usize = 8 * 1024;
pub const MAX_REPORT_ATTESTATIONS: usize = 16;
pub const REPORT_GRACE_BLOCKS: u64 = 16;
pub const SLASH_EVIDENCE_MIN_FEE: u128 = 1;

/// State-apply refusal text for a report carried at `height`: the code from
/// [`dispatch_refusal`] plus the type-only slice's explanation.
pub fn refusal_message(height: u64) -> String {
    format!(
        "{}: evidence apply is not enabled",
        dispatch_refusal(height)
    )
}

/// Refusal code block verification and state apply return for a report
/// carried at `height`. Mempool admission keeps the unconditional
/// `SP_SLASH_EVIDENCE_NOT_SUPPORTED` refusal until admission is implemented.
///
/// With [`ConsensusFeature::SlashEvidenceReports`] inactive (every height on
/// the live chain) this is the type-only slice's code, byte for byte. With it
/// active the type is still refused, under a distinct code, because the
/// capture/verify/apply slices have not landed: scheduling the gate early can
/// never admit an unverified report (refuse by default).
pub fn dispatch_refusal(height: u64) -> &'static str {
    if feature_active(ConsensusFeature::SlashEvidenceReports, height) {
        "SP_SLASH_EVIDENCE_APPLY_PENDING"
    } else {
        "SP_SLASH_EVIDENCE_NOT_SUPPORTED"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    #[error("SP_SLASH_EVIDENCE_SHAPE")]
    Shape,
    #[error("SP_SLASH_EVIDENCE_BOUNDS")]
    Bounds,
    #[error("SP_SLASH_EVIDENCE_ENCODING")]
    Encoding,
    #[error("SP_SLASH_EVIDENCE_AUTHENTICATION")]
    Authentication,
}

/// Signature container for the checked `SlashEvidenceReport` signed object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SlashEvidenceReportPayload {
    pub version: u16,
    pub commit_id: String,
    pub signed_commit: CommitPayload,
    pub reveal: RevealPayload,
    pub attestations: Vec<OracleAttestationPayload>,
    pub context_hash: String,
    pub reporter_pubkey: String,
    pub reporter_nonce: u64,
    pub fee: u128,
    pub reporter_signature: Option<String>,
}

/// Checked immutable body. No trait method can panic on hostile wire values.
#[derive(Debug, Clone)]
pub struct SlashEvidenceReport {
    body: Vec<u8>,
}

impl DomainSeparated for SlashEvidenceReport {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.slash_evidence_report.v1";

    fn canonical_body(&self) -> Vec<u8> {
        self.body.clone()
    }
}

pub(crate) fn canonical_hex<const N: usize>(text: &str) -> Result<[u8; N], EvidenceError> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(EvidenceError::Shape);
    }
    let mut bytes = [0; N];
    hex::decode_to_slice(text, &mut bytes).map_err(|_| EvidenceError::Shape)?;
    Ok(bytes)
}

// Explicit recursive key order, independent of serde_json's preserve_order feature.
fn sorted_json(value: Value, depth: usize) -> Result<Value, EvidenceError> {
    if depth > 32 {
        return Err(EvidenceError::Bounds);
    }
    Ok(match value {
        Value::Object(object) => {
            let sorted: std::collections::BTreeMap<_, _> = object.into_iter().collect();
            let mut result = serde_json::Map::new();
            for (key, value) in sorted {
                result.insert(key, sorted_json(value, depth + 1)?);
            }
            Value::Object(result)
        },
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|v| sorted_json(v, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        other => other,
    })
}

pub(crate) fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, EvidenceError> {
    // Report call sites validate every permitted typed float (attestation
    // confidence) before conversion. Reveal data is already serde_json::Value:
    // its Number cannot represent NaN/Infinity. Forecast manifest/receipt
    // floats are excluded from v1; remaining reveal metadata has no floats.
    let value = serde_json::to_value(value).map_err(|_| EvidenceError::Encoding)?;
    serde_json::to_vec(&sorted_json(value, 0)?).map_err(|_| EvidenceError::Encoding)
}

fn push_field(body: &mut Vec<u8>, value: &[u8]) -> Result<(), EvidenceError> {
    let len = u32::try_from(value.len()).map_err(|_| EvidenceError::Bounds)?;
    body.extend_from_slice(&len.to_le_bytes());
    body.extend_from_slice(value);
    Ok(())
}

impl TryFrom<&SlashEvidenceReportPayload> for SlashEvidenceReport {
    type Error = EvidenceError;

    fn try_from(payload: &SlashEvidenceReportPayload) -> Result<Self, Self::Error> {
        if payload.version != 1
            || payload.reveal.manifest.is_some()
            || payload.reveal.verification_receipt.is_some()
        {
            return Err(EvidenceError::Shape);
        }
        if serde_json::to_vec(payload)
            .map_err(|_| EvidenceError::Encoding)?
            .len()
            > MAX_REPORT_BYTES
            || payload.attestations.len() > MAX_REPORT_ATTESTATIONS
        {
            return Err(EvidenceError::Bounds);
        }
        let commit_id = canonical_hex::<32>(&payload.commit_id)?;
        let context = canonical_hex::<32>(&payload.context_hash)?;
        let reporter = canonical_hex::<32>(&payload.reporter_pubkey)?;
        canonical_hex::<32>(
            payload
                .signed_commit
                .auth_pubkey
                .as_deref()
                .ok_or(EvidenceError::Shape)?,
        )?;
        canonical_hex::<64>(
            payload
                .signed_commit
                .auth_signature
                .as_deref()
                .ok_or(EvidenceError::Shape)?,
        )?;
        if let Some(signature) = &payload.reporter_signature {
            canonical_hex::<64>(signature)?;
        }
        let reveal = canonical_json(&payload.reveal)?;
        if reveal.len() > MAX_REVEAL_BYTES {
            return Err(EvidenceError::Bounds);
        }
        let mut previous = None;
        for attestation in &payload.attestations {
            let key = canonical_hex::<32>(&attestation.oracle_id)?;
            if previous.is_some_and(|previous| previous >= key) || !attestation.is_signable() {
                return Err(EvidenceError::Shape);
            }
            canonical_hex::<64>(
                attestation
                    .signature
                    .as_deref()
                    .ok_or(EvidenceError::Shape)?,
            )?;
            previous = Some(key);
        }
        let mut body = payload.version.to_le_bytes().to_vec();
        body.extend_from_slice(&commit_id);
        body.extend_from_slice(&context);
        push_field(&mut body, &canonical_json(&payload.signed_commit)?)?;
        push_field(&mut body, &reveal)?;
        body.extend_from_slice(&(payload.attestations.len() as u32).to_le_bytes());
        for attestation in &payload.attestations {
            push_field(&mut body, &canonical_json(attestation)?)?;
        }
        body.extend_from_slice(&reporter);
        body.extend_from_slice(&payload.reporter_nonce.to_le_bytes());
        body.extend_from_slice(&payload.fee.to_le_bytes());
        Ok(Self { body })
    }
}

impl SlashEvidenceReportPayload {
    pub fn signing_preimage_for(&self, identity: &ChainIdentity) -> Result<Vec<u8>, EvidenceError> {
        Ok(SlashEvidenceReport::try_from(self)?
            .signing_preimage_for(identity, SigningProtocolVersion::V2))
    }

    pub fn sign_for(
        &mut self,
        signer: &PrivateKey,
        identity: &ChainIdentity,
    ) -> Result<(), EvidenceError> {
        if PublicKey::from_private_key(signer).to_hex() != self.reporter_pubkey {
            return Err(EvidenceError::Authentication);
        }
        let signature = signer.sign(&self.signing_preimage_for(identity)?);
        self.reporter_signature = Some(signature.to_hex());
        SlashEvidenceReport::try_from(&*self)?;
        Ok(())
    }

    /// Verify reporter, commit contributor and optional attester signatures,
    /// plus the reveal commitment. Auxiliary reveal metadata conveys no authority.
    /// Owner authority, retained policy/window and replay checks require chain state.
    pub fn verify_for(&self, identity: &ChainIdentity) -> Result<(), EvidenceError> {
        let message = self.signing_preimage_for(identity)?;
        let key = PublicKey::from_hex(&self.reporter_pubkey)
            .map_err(|_| EvidenceError::Authentication)?;
        let signature = Signature::from_hex(
            self.reporter_signature
                .as_deref()
                .ok_or(EvidenceError::Authentication)?,
        )
        .map_err(|_| EvidenceError::Authentication)?;
        if !key.verify(&message, &signature) {
            return Err(EvidenceError::Authentication);
        }
        let commit = &self.signed_commit;
        let reveal = &self.reveal;
        if self.commit_id != commit.commit_id
            || self.commit_id != reveal.commit_id
            || commit.station_id != reveal.station_id
            || commit.timestamp != reveal.timestamp
            || commit.contribution_type != reveal.contribution_type
            || Transaction::compute_commit_id(
                &reveal.data,
                &reveal.nonce,
                &reveal.station_id,
                reveal.timestamp,
                &reveal.contribution_type,
            ) != self.commit_id
        {
            return Err(EvidenceError::Authentication);
        }
        let contributor =
            PublicKey::from_hex(commit.auth_pubkey.as_deref().ok_or(EvidenceError::Shape)?)
                .map_err(|_| EvidenceError::Authentication)?;
        let signature = Signature::from_hex(
            commit
                .auth_signature
                .as_deref()
                .ok_or(EvidenceError::Shape)?,
        )
        .map_err(|_| EvidenceError::Authentication)?;
        let authorization = commit_authorization_message_with_domain(
            &identity.chain_id,
            &identity.network_id,
            &commit.station_id,
            &commit.commit_id,
            &commit.contribution_type,
        );
        if !contributor.verify(&authorization, &signature) {
            return Err(EvidenceError::Authentication);
        }
        for annotation in &self.attestations {
            if annotation.commit_id != self.commit_id {
                return Err(EvidenceError::Authentication);
            }
            let key = PublicKey::from_hex(&annotation.oracle_id)
                .map_err(|_| EvidenceError::Authentication)?;
            let signature = Signature::from_hex(
                annotation
                    .signature
                    .as_deref()
                    .ok_or(EvidenceError::Shape)?,
            )
            .map_err(|_| EvidenceError::Authentication)?;
            let message = annotation
                .canonical_signing_message_with_domain(&identity.chain_id, &identity.network_id);
            if !key.verify(message.as_bytes(), &signature) {
                return Err(EvidenceError::Authentication);
            }
        }
        Ok(())
    }

    pub fn evidence_id(&self) -> Result<[u8; 32], EvidenceError> {
        let commitment = canonical_hex::<32>(&self.commit_id)?;
        Ok(*blake3::hash(&domain_preimage(
            b"wrfcoin.slash_evidence_id.v1",
            &commitment,
        ))
        .as_bytes())
    }
}
