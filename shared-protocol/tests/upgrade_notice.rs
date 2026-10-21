//! #3094: governance-signed, domain-separated upgrade notices — sign/verify
//! round trip and every named negative case, through public API only.

use std::collections::BTreeMap;

use rand::SeedableRng;
use rand::rngs::StdRng;
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_shared_protocol::crypto::domain_signing::{DomainSeparated, DomainSigningAction};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    sign_upgrade_notice, sign_upgrade_notice_with_key, verify_upgrade_notice,
    GovernanceSignature, SignedUpgradeNotice, UpgradeNotice, UpgradeNoticeError,
};

const CHAIN: &str = "777";
const NETWORK: &str = "testnet";
const NOW: i64 = 1_800_000_000;

fn key(seed: u64) -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
}

fn pubkey_hex(key: &PrivateKey) -> String {
    PublicKey::from_private_key(key).to_hex().to_ascii_lowercase()
}

fn notice() -> UpgradeNotice {
    let mut image_digests = BTreeMap::new();
    image_digests.insert("native-chain-amd64".to_string(), "sha256:aaaa".to_string());
    image_digests.insert("native-chain-arm64".to_string(), "sha256:bbbb".to_string());
    UpgradeNotice {
        notice_id: UpgradeNotice::notice_id_for(14_000, "8d29ea85b0123456"),
        chain_id: CHAIN.into(),
        network_id: NETWORK.into(),
        activation_height: 14_000,
        min_protocol_version: 3,
        features: vec!["domain_separated_preimages".into()],
        release_commit: "8d29ea85b0123456".into(),
        image_digests,
        issued_at: NOW - 3_600,
        expires_at: Some(NOW + 86_400),
    }
}

fn signed_by(keys: &[&PrivateKey]) -> SignedUpgradeNotice {
    let notice = notice();
    let signatures = keys
        .iter()
        .map(|k| sign_upgrade_notice_with_key(&notice, k))
        .collect();
    SignedUpgradeNotice { notice, signatures }
}

fn allowlist(keys: &[&PrivateKey]) -> Vec<String> {
    keys.iter().map(|k| pubkey_hex(k)).collect()
}

#[test]
fn sign_verify_round_trip() {
    let governance = key(1);
    let signed = signed_by(&[&governance]);
    assert_eq!(signed.signatures[0].signer_pubkey, pubkey_hex(&governance));
    assert_eq!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allowlist(&[&governance]), NOW),
        Ok(())
    );
    // The allowlist comparison is case-insensitive, as the env allowlist is.
    let upper = vec![pubkey_hex(&governance).to_ascii_uppercase()];
    assert_eq!(verify_upgrade_notice(&signed, CHAIN, NETWORK, &upper, NOW), Ok(()));
}

#[test]
fn the_json_artifact_round_trips_and_still_verifies() {
    let governance = key(1);
    let signed = signed_by(&[&governance]);
    let json = serde_json::to_string_pretty(&signed).unwrap();
    let back: SignedUpgradeNotice = serde_json::from_str(&json).unwrap();
    assert_eq!(back, signed);
    assert_eq!(
        verify_upgrade_notice(&back, CHAIN, NETWORK, &allowlist(&[&governance]), NOW),
        Ok(())
    );
}

#[test]
fn a_notice_for_another_chain_is_rejected_before_any_signature_check() {
    let governance = key(1);
    let signed = signed_by(&[&governance]);
    let err = verify_upgrade_notice(&signed, "888", NETWORK, &allowlist(&[&governance]), NOW)
        .unwrap_err();
    assert_eq!(
        err,
        UpgradeNoticeError::WrongChain {
            expected: ChainIdentity::new("888", NETWORK),
            found: ChainIdentity::new(CHAIN, NETWORK),
        }
    );
    let err = verify_upgrade_notice(&signed, CHAIN, "mainnet", &allowlist(&[&governance]), NOW)
        .unwrap_err();
    assert!(matches!(err, UpgradeNoticeError::WrongChain { .. }));
}

#[test]
fn a_signer_outside_the_allowlist_is_rejected() {
    let governance = key(1);
    let stranger = key(2);
    let signed = signed_by(&[&stranger]);
    assert_eq!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allowlist(&[&governance]), NOW),
        Err(UpgradeNoticeError::SignerNotAuthorized {
            signer_pubkey: pubkey_hex(&stranger)
        })
    );
    // An empty allowlist authorizes nobody (fail-closed).
    assert!(matches!(
        verify_upgrade_notice(&signed_by(&[&governance]), CHAIN, NETWORK, &[], NOW),
        Err(UpgradeNoticeError::SignerNotAuthorized { .. })
    ));
}

#[test]
fn any_tampered_field_is_a_bad_signature() {
    let governance = key(1);
    let allow = allowlist(&[&governance]);
    let base = signed_by(&[&governance]);
    let mut variants: Vec<(&str, SignedUpgradeNotice)> = Vec::new();

    let mut v = base.clone();
    v.notice.activation_height += 1;
    variants.push(("activation_height", v));
    let mut v = base.clone();
    v.notice.min_protocol_version += 1;
    variants.push(("min_protocol_version", v));
    let mut v = base.clone();
    v.notice.features.push("another".into());
    variants.push(("features", v));
    let mut v = base.clone();
    v.notice.release_commit.push('0');
    variants.push(("release_commit", v));
    let mut v = base.clone();
    v.notice
        .image_digests
        .insert("native-chain-amd64".into(), "sha256:evil".into());
    variants.push(("image_digests", v));
    let mut v = base.clone();
    v.notice.issued_at += 1;
    variants.push(("issued_at", v));
    let mut v = base.clone();
    v.notice.expires_at = None;
    variants.push(("expires_at", v));
    let mut v = base.clone();
    v.notice.notice_id.push('x');
    variants.push(("notice_id", v));

    for (field, tampered) in variants {
        match verify_upgrade_notice(&tampered, CHAIN, NETWORK, &allow, NOW) {
            Err(UpgradeNoticeError::BadSignature { signer_pubkey, .. }) => {
                assert_eq!(signer_pubkey, pubkey_hex(&governance), "{field}")
            },
            other => panic!("tampering {field} must be BadSignature, got {other:?}"),
        }
    }
}

#[test]
fn malformed_or_duplicate_signatures_are_bad_signatures_not_panics() {
    let governance = key(1);
    let allow = allowlist(&[&governance]);
    let base = signed_by(&[&governance]);

    let mut bad_sig = base.clone();
    bad_sig.signatures[0].signature = "zz".into();
    assert!(matches!(
        verify_upgrade_notice(&bad_sig, CHAIN, NETWORK, &allow, NOW),
        Err(UpgradeNoticeError::BadSignature { .. })
    ));

    let mut wrong_key_sig = base.clone();
    wrong_key_sig.signatures[0].signature = key(9).sign(b"unrelated").to_hex();
    assert!(matches!(
        verify_upgrade_notice(&wrong_key_sig, CHAIN, NETWORK, &allow, NOW),
        Err(UpgradeNoticeError::BadSignature { .. })
    ));

    let mut duplicated = base.clone();
    duplicated.signatures.push(duplicated.signatures[0].clone());
    match verify_upgrade_notice(&duplicated, CHAIN, NETWORK, &allow, NOW) {
        Err(UpgradeNoticeError::BadSignature { reason, .. }) => {
            assert!(reason.contains("twice"), "{reason}")
        },
        other => panic!("duplicate signer must be BadSignature, got {other:?}"),
    }

    let mut bad_key = base;
    bad_key.signatures[0].signer_pubkey = "not-hex".into();
    assert!(matches!(
        verify_upgrade_notice(&bad_key, CHAIN, NETWORK, &["not-hex".to_string()], NOW),
        Err(UpgradeNoticeError::BadSignature { .. })
    ));
}

#[test]
fn an_expired_notice_is_rejected_and_one_without_expiry_never_expires() {
    let governance = key(1);
    let allow = allowlist(&[&governance]);
    let signed = signed_by(&[&governance]);
    let expires_at = signed.notice.expires_at.unwrap();
    assert_eq!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allow, expires_at + 1),
        Err(UpgradeNoticeError::Expired {
            expires_at,
            now: expires_at + 1
        })
    );
    assert_eq!(verify_upgrade_notice(&signed, CHAIN, NETWORK, &allow, expires_at), Ok(()));

    let mut forever = notice();
    forever.expires_at = None;
    let sig = sign_upgrade_notice_with_key(&forever, &governance);
    let signed = SignedUpgradeNotice {
        notice: forever,
        signatures: vec![sig],
    };
    assert_eq!(verify_upgrade_notice(&signed, CHAIN, NETWORK, &allow, i64::MAX), Ok(()));
}

#[test]
fn no_signatures_is_a_named_error() {
    let signed = SignedUpgradeNotice {
        notice: notice(),
        signatures: vec![],
    };
    assert_eq!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allowlist(&[&key(1)]), NOW),
        Err(UpgradeNoticeError::NoSignatures)
    );
}

#[test]
fn two_governance_signatures_verify_together_preparing_threshold_signing() {
    let a = key(1);
    let b = key(2);
    let signed = signed_by(&[&a, &b]);
    assert_eq!(signed.signatures.len(), 2);
    assert_eq!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allowlist(&[&a, &b]), NOW),
        Ok(())
    );
    // One of the two off the allowlist: the whole notice is refused.
    assert!(matches!(
        verify_upgrade_notice(&signed, CHAIN, NETWORK, &allowlist(&[&a]), NOW),
        Err(UpgradeNoticeError::SignerNotAuthorized { .. })
    ));
}

#[test]
fn the_preimage_is_chain_bound_and_domain_tagged() {
    let n = notice();
    let preimage = n.preimage();
    assert!(!preimage.is_empty());
    assert_eq!(
        UpgradeNotice::DOMAIN_TAG,
        DomainSigningAction::UpgradeNotice.domain().as_bytes()
    );
    // Same fields, different chain: different bytes, so a signature cannot
    // be moved between chains.
    let mut other = n.clone();
    other.chain_id = "888".into();
    assert_ne!(other.preimage(), preimage);
    // A signature made under the domain-tagged preimage verifies through the
    // BlockSigner path as well as the raw-key path.
    let governance = key(1);
    let signer = wrfcoin_shared_protocol::crypto::signer::EnvKeySigner::from_private_key(
        governance.clone(),
        "TEST_GOVERNANCE_KEY",
    );
    let via_signer = sign_upgrade_notice(&n, &signer).expect("sign");
    let via_key = sign_upgrade_notice_with_key(&n, &governance);
    assert_eq!(via_signer, via_key);
    let _: GovernanceSignature = via_signer;
}
