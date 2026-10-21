//! Governance-signed, domain-separated upgrade notices (#3094).
//!
//! One artifact that tells an operator the owner cannot reach by SSH what to
//! install and by when: the activation height, the minimum protocol version
//! the release carries, the features being activated, and the release
//! (commit + image digests). It is trusted **by signature only** — never by
//! the transport that delivered it — against the governance allowlist the
//! node already trusts (`WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`).
//!
//! The preimage is the [`DomainSeparated`] construction under its own tag
//! (`wrfcoin.upgrade_notice.v1`, [`DomainSigningAction::UpgradeNotice`]),
//! bound to the chain and network **named in the notice** at
//! [`SigningProtocolVersion::V2`]. Binding to the notice's own identity
//! rather than the verifier's ambient environment is what makes offline
//! verification honest: a notice for another chain fails
//! [`UpgradeNoticeError::WrongChain`] on the identity check and would fail
//! the signature check too.
//!
//! `signatures` is a `Vec` from day one so #2156 threshold signing is
//! additive; today every signature must verify and every signer must be on
//! the allowlist. Transport (file / peer endpoint) is #3095's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::{DomainSeparated, DomainSigningAction};
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use crate::crypto::signer::BlockSigner;

/// The signing protocol version every notice is bound at. Notices are
/// chain-bound by construction; there is no unbound (V1) form.
pub const UPGRADE_NOTICE_SIGNING_VERSION: SigningProtocolVersion = SigningProtocolVersion::V2;

/// What an operator installs, and by when.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UpgradeNotice {
    /// `upgrade-<activation_height>-<short commit>`; see
    /// [`UpgradeNotice::notice_id_for`].
    pub notice_id: String,
    pub chain_id: String,
    pub network_id: String,
    /// The height at which the scheduled features activate. A node on an
    /// older protocol version stops cleanly at `activation_height - 1`
    /// (#3093).
    pub activation_height: u64,
    /// The `PROTOCOL_VERSION` (#3093) the announced release carries.
    pub min_protocol_version: u32,
    /// `ConsensusFeature` names being activated (#3092).
    pub features: Vec<String>,
    /// Full core4 commit SHA of the release.
    pub release_commit: String,
    /// `"native-chain-amd64" -> "sha256:…"`, from the release digest ledger
    /// (wrfcoin/infra#938). `BTreeMap` so the preimage order is canonical.
    pub image_digests: BTreeMap<String, String>,
    /// Unix seconds.
    pub issued_at: i64,
    /// Unix seconds; `None` never expires.
    pub expires_at: Option<i64>,
}

impl UpgradeNotice {
    /// The canonical notice id: `upgrade-<activation_height>-<first 12 hex of commit>`.
    pub fn notice_id_for(activation_height: u64, release_commit: &str) -> String {
        let short: String = release_commit.trim().chars().take(12).collect();
        format!("upgrade-{activation_height}-{short}")
    }

    /// Whether the notice has expired at `now`. A notice with no expiry never
    /// does.
    pub fn is_expired_at(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|expires| now > expires)
    }

    /// The identity this notice is bound to.
    pub fn identity(&self) -> ChainIdentity {
        ChainIdentity::new(self.chain_id.clone(), self.network_id.clone())
    }

    /// The exact bytes a governance signer signs.
    pub fn preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(&self.identity(), UPGRADE_NOTICE_SIGNING_VERSION)
    }
}

impl DomainSeparated for UpgradeNotice {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.upgrade_notice.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Every field, length-prefixed by BodyBuilder; the two collections
        // are count-prefixed and in canonical order (features as given —
        // order is part of what was signed; digests sorted by the BTreeMap).
        let mut builder = BodyBuilder::new()
            .str_field(&self.notice_id)
            .str_field(&self.chain_id)
            .str_field(&self.network_id)
            .u64_field(self.activation_height)
            .u32_field(self.min_protocol_version)
            .u64_field(self.features.len() as u64);
        for feature in &self.features {
            builder = builder.str_field(feature);
        }
        builder = builder
            .str_field(&self.release_commit)
            .u64_field(self.image_digests.len() as u64);
        for (image, digest) in &self.image_digests {
            builder = builder.str_field(image).str_field(digest);
        }
        builder = builder
            .i64_field(self.issued_at)
            .bool_field(self.expires_at.is_some())
            .i64_field(self.expires_at.unwrap_or(0));
        builder.finish()
    }
}

/// One governance signer's signature over an [`UpgradeNotice`] preimage.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GovernanceSignature {
    /// Lowercase hex ed25519 public key, as listed in the allowlist.
    pub signer_pubkey: String,
    /// Hex ed25519 signature over [`UpgradeNotice::preimage`].
    pub signature: String,
}

/// A notice plus its governance signatures — the artifact that travels.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SignedUpgradeNotice {
    pub notice: UpgradeNotice,
    pub signatures: Vec<GovernanceSignature>,
}

/// Why a signed notice was refused. Every case is a named error; none panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpgradeNoticeError {
    /// The notice names another chain or network than the verifier's.
    WrongChain {
        expected: ChainIdentity,
        found: ChainIdentity,
    },
    /// A signer is not on the governance allowlist.
    SignerNotAuthorized {
        signer_pubkey: String,
    },
    /// `expires_at` is in the past at the verifier's `now`.
    Expired { expires_at: i64, now: i64 },
    /// No signatures at all.
    NoSignatures,
    /// A signature does not verify over the notice — the notice was altered
    /// after signing, the key or signature hex is malformed, or a signer is
    /// listed twice.
    BadSignature {
        signer_pubkey: String,
        reason: String,
    },
    /// The signer could not produce a signature (an HSM refusal, say).
    SignerFailed { reason: String },
}

impl std::fmt::Display for UpgradeNoticeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongChain { expected, found } => write!(
                f,
                "upgrade notice is for chain {found:?}, this node is on {expected:?}"
            ),
            Self::SignerNotAuthorized { signer_pubkey } => write!(
                f,
                "upgrade notice signer {signer_pubkey} is not in the governance allowlist"
            ),
            Self::Expired { expires_at, now } => {
                write!(f, "upgrade notice expired at {expires_at} (now {now})")
            },
            Self::NoSignatures => write!(f, "upgrade notice carries no signatures"),
            Self::BadSignature {
                signer_pubkey,
                reason,
            } => write!(
                f,
                "upgrade notice signature by {signer_pubkey} does not verify: {reason}"
            ),
            Self::SignerFailed { reason } => write!(f, "upgrade notice signer failed: {reason}"),
        }
    }
}

impl std::error::Error for UpgradeNoticeError {}

/// Sign a notice with a [`BlockSigner`] (env key, key file, or HSM under
/// `--features hsm`). The signature binds the chain and network named in the
/// notice, so signing reads no ambient configuration.
pub fn sign_upgrade_notice(
    notice: &UpgradeNotice,
    signer: &dyn BlockSigner,
) -> Result<GovernanceSignature, UpgradeNoticeError> {
    let signature = signer
        .sign(&notice.preimage())
        .map_err(|e| UpgradeNoticeError::SignerFailed {
            reason: e.to_string(),
        })?;
    Ok(GovernanceSignature {
        signer_pubkey: signer.public_key().to_hex().to_ascii_lowercase(),
        signature: signature.to_hex(),
    })
}

/// [`sign_upgrade_notice`] with a raw key — the governance key file the
/// oracle ceremony already uses (`oracle build-registrations --governance-key`).
pub fn sign_upgrade_notice_with_key(notice: &UpgradeNotice, key: &PrivateKey) -> GovernanceSignature {
    GovernanceSignature {
        signer_pubkey: PublicKey::from_private_key(key)
            .to_hex()
            .to_ascii_lowercase(),
        signature: key.sign(&notice.preimage()).to_hex(),
    }
}

/// Verify a signed notice against the verifier's chain identity and the
/// governance allowlist (lowercase hex public keys), at time `now`.
///
/// Checks, in order: chain identity, at least one signature, every signature
/// (allowlisted signer, well-formed, verifies over the preimage, no signer
/// twice), then expiry — so a tampered notice is reported as tampered even
/// when it is also stale. `Ok(())` means every signature is a valid
/// governance signature; how many are required is #2156's threshold rule.
pub fn verify_upgrade_notice(
    signed: &SignedUpgradeNotice,
    chain_id: &str,
    network_id: &str,
    allowlist: &[String],
    now: i64,
) -> Result<(), UpgradeNoticeError> {
    let expected = ChainIdentity::new(chain_id, network_id);
    let found = signed.notice.identity();
    if found != expected {
        return Err(UpgradeNoticeError::WrongChain { expected, found });
    }
    if signed.signatures.is_empty() {
        return Err(UpgradeNoticeError::NoSignatures);
    }
    let allow: Vec<String> = allowlist
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    let preimage = signed.notice.preimage();
    let mut seen: Vec<String> = Vec::with_capacity(signed.signatures.len());
    for entry in &signed.signatures {
        let signer_pubkey = entry.signer_pubkey.trim().to_ascii_lowercase();
        if !allow.iter().any(|k| *k == signer_pubkey) {
            return Err(UpgradeNoticeError::SignerNotAuthorized { signer_pubkey });
        }
        if seen.contains(&signer_pubkey) {
            return Err(UpgradeNoticeError::BadSignature {
                signer_pubkey,
                reason: "signer listed twice".into(),
            });
        }
        let public_key = PublicKey::from_hex(&signer_pubkey).map_err(|e| {
            UpgradeNoticeError::BadSignature {
                signer_pubkey: signer_pubkey.clone(),
                reason: format!("signer public key is not valid hex ed25519: {e}"),
            }
        })?;
        let signature = Signature::from_hex(entry.signature.trim()).map_err(|e| {
            UpgradeNoticeError::BadSignature {
                signer_pubkey: signer_pubkey.clone(),
                reason: format!("signature is not valid hex: {e}"),
            }
        })?;
        if !public_key.verify(&preimage, &signature) {
            return Err(UpgradeNoticeError::BadSignature {
                signer_pubkey,
                reason: "signature does not verify over the notice preimage — the notice was \
                         modified after it was signed, or was signed for another chain"
                    .into(),
            });
        }
        seen.push(signer_pubkey);
    }
    if let Some(expires_at) = signed.notice.expires_at {
        if now > expires_at {
            return Err(UpgradeNoticeError::Expired { expires_at, now });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_domain_tag_is_the_action_domain() {
        assert_eq!(
            // secret-leak-ok: test; `DomainSigningAction` is an enum type name, no key material
            UpgradeNotice::DOMAIN_TAG,
            DomainSigningAction::UpgradeNotice.domain().as_bytes()
        );
    }

    #[test]
    fn notice_id_uses_the_short_commit() {
        assert_eq!(
            UpgradeNotice::notice_id_for(14_000, "8d29ea85b0123456789abcdef"),
            "upgrade-14000-8d29ea85b012"
        );
    }
}
