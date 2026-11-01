//! Golden vectors for `passkey_bond_payout`: the WebAuthn challenge a passkey
//! payout account signs for a node-bond Deposit/Update, and the passkey
//! address derivation it must match.
//!
//! `tests/fixtures/passkey_bond_payout_vectors.json` is the normative,
//! cross-repo fixture. The wrfcoin.com approval page pins its TypeScript port
//! against it. This file checks it three ways:
//! 1. every value equals what the shipped Rust functions compute;
//! 2. the preimages are rebuilt here byte by byte from the frozen grammar
//!    (length-prefixed u64-LE fields, sha3-256), independent of the library
//!    framing code;
//! 3. the example assertion in the fixture verifies under the consensus
//!    verifier (`verify_payout_passkey`) and fails for every other case.
//!
//! Regenerate only on an intentional, reviewed grammar change:
//! `WRFCOIN_REGEN_GOLDEN_VECTORS=1 cargo test -p wrfcoin-shared-protocol --test passkey_bond_payout_vectors`
//! then review every byte of the diff.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sha3::Sha3_256;
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{
    PASSKEY_ADDRESS_DOMAIN, PasskeyAuthorization, base64url_nopad, passkey_address_from_parts,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{
    NodeBondLifecycle, node_bond_escrow_address, verify_payout_passkey,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::{
    NodeBondAction, NodeBondAuthorization, NodeBondPayoutPasskeyApproval,
    SignedNodeBondAuthorization, node_bond_payout_passkey_challenge, node_contract_digest,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/passkey_bond_payout_vectors.json"
);
const SCHEMA: &str = "wrfcoin.passkey_bond_payout_vectors.v1";
const PASSKEY_SECRET: [u8; 32] = [
    0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80,
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90,
];
const RP_ID: &str = "wrfcoin.com";

fn pk(byte: u8) -> [u8; 32] {
    PublicKey::from_private_key(&PrivateKey::from_bytes(&[byte; 32]).unwrap())
        .as_bytes()
        .try_into()
        .unwrap()
}

fn authenticator() -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&PASSKEY_SECRET, RP_ID)
}

struct Case {
    name: &'static str,
    chain_id: &'static str,
    network_id: &'static str,
    authorization: NodeBondAuthorization,
}

fn cases() -> Vec<Case> {
    let payout = authenticator().account();
    let owner = Address::from_public_key(&PublicKey::from_bytes(&pk(10)).unwrap());
    let deployment = [0x5a; 32];
    let base = NodeBondAuthorization {
        genesis_hash: [0x88; 32],
        deployment_id: deployment,
        owner_account: owner,
        escrow_account: node_bond_escrow_address(&deployment),
        payout_account: payout,
        server_key: pk(11),
        consumer_key: pk(12),
        service_alias: "newcomer-node-1".into(),
        action: NodeBondAction::Deposit,
        amount_units: wrfcoin_shared_protocol::consensus::node_bond_registry::NODE_BOND_UNITS,
        expected_nonce: 7,
        effective_epoch: 3,
    };
    let update = NodeBondAuthorization {
        action: NodeBondAction::Update,
        amount_units: 0,
        expected_nonce: 9,
        effective_epoch: 5,
        ..base.clone()
    };
    vec![
        Case { name: "deposit_888_testnet", chain_id: "888", network_id: "testnet", authorization: base },
        Case { name: "update_888_testnet", chain_id: "888", network_id: "testnet", authorization: update },
    ]
}

fn lp(buf: &mut Vec<u8>, field: &[u8]) {
    buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
    buf.extend_from_slice(field);
}

/// The frozen grammar, rebuilt without the library's framing helpers.
fn independent_authorization_body(a: &NodeBondAuthorization) -> Vec<u8> {
    let mut b = Vec::new();
    lp(&mut b, &a.genesis_hash);
    lp(&mut b, &a.deployment_id);
    lp(&mut b, &a.owner_account.0);
    lp(&mut b, &a.escrow_account.0);
    lp(&mut b, &a.payout_account.0);
    lp(&mut b, &a.server_key);
    lp(&mut b, &a.consumer_key);
    lp(&mut b, a.service_alias.as_bytes());
    b.extend_from_slice(&(a.action as u32).to_le_bytes());
    b.extend_from_slice(&a.amount_units.to_le_bytes());
    b.extend_from_slice(&a.expected_nonce.to_le_bytes());
    b.extend_from_slice(&a.effective_epoch.to_le_bytes());
    b
}

fn independent_v2_preimage(tag: &[u8], chain: &str, network: &str, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    lp(&mut p, tag);
    lp(&mut p, chain.as_bytes());
    lp(&mut p, network.as_bytes());
    p.extend_from_slice(&2u32.to_le_bytes());
    lp(&mut p, body);
    p
}

fn unsigned_lifecycle(a: &NodeBondAuthorization) -> NodeBondLifecycle {
    NodeBondLifecycle {
        signed: SignedNodeBondAuthorization {
            authorization: a.clone(),
            owner_signature: vec![],
            server_signature: vec![],
            consumer_signature: vec![],
        },
        owner_key: pk(10),
        governance_key: Some(pk(99)),
        governance_signature: vec![],
        payout_key: None,
        payout_signature: vec![],
        payout_passkey: None,
    }
}

fn build() -> Value {
    let auth = authenticator();
    let xy = auth.key.public_key().to_raw_xy();
    let rp_id_hash: [u8; 32] = Sha256::digest(RP_ID.as_bytes()).into();
    let mut address_preimage = PASSKEY_ADDRESS_DOMAIN.to_vec();
    address_preimage.extend_from_slice(&rp_id_hash);
    address_preimage.push(0x04);
    address_preimage.extend_from_slice(&xy);
    let address_digest: [u8; 32] = Sha3_256::digest(&address_preimage).into();
    let address = Address(address_digest[..20].try_into().unwrap());
    assert_eq!(address, passkey_address_from_parts(&rp_id_hash, &xy));
    assert_eq!(address, auth.account());

    let mut out = Vec::new();
    for case in cases() {
        let identity = ChainIdentity::new(case.chain_id, case.network_id);
        let a = &case.authorization;
        let body = a.canonical_body();
        assert_eq!(body, independent_authorization_body(a), "{}", case.name);
        let auth_preimage = a.signing_preimage_for(&identity, SigningProtocolVersion::V2);
        assert_eq!(
            auth_preimage,
            independent_v2_preimage(NodeBondAuthorization::DOMAIN_TAG, case.chain_id, case.network_id, &body)
        );
        let auth_hash: [u8; 32] = Sha3_256::digest(&auth_preimage).into();
        assert_eq!(auth_hash, node_contract_digest(a, &identity));
        let approval = NodeBondPayoutPasskeyApproval { authorization_preimage_hash: auth_hash };
        let mut approval_body = Vec::new();
        lp(&mut approval_body, &auth_hash);
        assert_eq!(approval.canonical_body(), approval_body);
        let approval_preimage = independent_v2_preimage(
            b"wrfcoin.node_bond_payout_passkey.v1",
            case.chain_id,
            case.network_id,
            &approval_body,
        );
        assert_eq!(
            approval_preimage,
            approval.signing_preimage_for(&identity, SigningProtocolVersion::V2)
        );
        let challenge: [u8; 32] = Sha3_256::digest(&approval_preimage).into();
        assert_eq!(challenge, node_bond_payout_passkey_challenge(a, &identity));

        let assertion = auth.assert(&challenge);
        let example = auth.authorization_for(&assertion);
        verify_payout_passkey(a, &example, &identity).expect("example verifies");

        let unsigned = unsigned_lifecycle(a);
        out.push(json!({
            "name": case.name,
            "chain_id": case.chain_id,
            "network_id": case.network_id,
            "signing_version": 2,
            "authorization": serde_json::to_value(a).unwrap(),
            "unsigned_lifecycle": serde_json::to_value(&unsigned).unwrap(),
            "unsigned_lifecycle_json": serde_json::to_string(&unsigned).unwrap(),
            "authorization_canonical_body_hex": hex::encode(&body),
            "authorization_v2_preimage_hex": hex::encode(&auth_preimage),
            "authorization_preimage_hash_hex": hex::encode(auth_hash),
            "approval_canonical_body_hex": hex::encode(&approval_body),
            "approval_v2_preimage_hex": hex::encode(&approval_preimage),
            "challenge_hex": hex::encode(challenge),
            "challenge_base64url": base64url_nopad(&challenge),
            "payout_account_hex": a.payout_account.to_hex(),
            "example_payout_passkey": serde_json::to_value(&example).unwrap(),
        }));
    }
    json!({
        "schema": SCHEMA,
        "description": "passkey_bond_payout: the WebAuthn challenge a passkey payout account signs for a node-bond Deposit/Update. challenge = sha3_256(V2 preimage of NodeBondPayoutPasskeyApproval { authorization_preimage_hash: sha3_256(V2 preimage of NodeBondAuthorization) }). V2 preimage = lp(tag) || lp(chain_id) || lp(network_id) || u32le(2) || lp(canonical_body); lp(x) = u64le(len(x)) || x. NodeBondAuthorization body = lp(genesis_hash) lp(deployment_id) lp(owner) lp(escrow) lp(payout) lp(server_key) lp(consumer_key) lp(alias utf8) u32le(action: Deposit=0 Update=1 Exit=2 Withdraw=3) u64le(amount_units) u64le(expected_nonce) u64le(effective_epoch). Approval body = lp(authorization_preimage_hash). In JSON, [u8;N] and Address fields are arrays of numbers. Pass the 32 challenge bytes to navigator.credentials.get; clientDataJSON.challenge is challenge_base64url. The filled lifecycle sets payout_key null, payout_signature [], payout_passkey {hex fields}. The example assertion is from a deterministic software authenticator (RFC 6979, low-s); a real authenticator's signature differs but verifies the same way.",
        "authorization_domain_tag": "wrfcoin.node_bond_authorization.v1",
        "approval_domain_tag": "wrfcoin.node_bond_payout_passkey.v1",
        "passkey_address": {
            "rp_id": RP_ID,
            "rp_id_hash_hex": hex::encode(rp_id_hash),
            "public_key_p256_xy_hex": hex::encode(xy),
            "address_domain": String::from_utf8(PASSKEY_ADDRESS_DOMAIN.to_vec()).unwrap(),
            "address_preimage_hex": hex::encode(&address_preimage),
            "address_hex": address.to_hex(),
            "address_bytes": serde_json::to_value(address).unwrap(),
        },
        "cases": out,
    })
}

#[test]
fn passkey_bond_payout_golden_vectors_match_the_fixture() {
    let built = build();
    let rendered = serde_json::to_string_pretty(&built).unwrap() + "\n";
    if std::env::var_os("WRFCOIN_REGEN_GOLDEN_VECTORS").is_some() {
        std::fs::write(FIXTURE, &rendered).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(FIXTURE).expect("fixture present");
    let parsed: Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(parsed["schema"], SCHEMA);
    assert_eq!(parsed, built, "fixture drifted from the Rust grammar");
}

#[test]
fn fixture_lifecycles_parse_and_example_assertions_bind_only_their_case() {
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(FIXTURE).unwrap()).unwrap();
    let cases = on_disk["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let identity = ChainIdentity::new(
            case["chain_id"].as_str().unwrap(),
            case["network_id"].as_str().unwrap(),
        );
        let unsigned: NodeBondLifecycle =
            serde_json::from_str(case["unsigned_lifecycle_json"].as_str().unwrap()).unwrap();
        // An absent payout_passkey is not serialized at all.
        assert!(!case["unsigned_lifecycle_json"].as_str().unwrap().contains("payout_passkey"));
        let a = &unsigned.signed.authorization;
        assert_eq!(
            hex::encode(node_bond_payout_passkey_challenge(a, &identity)),
            case["challenge_hex"].as_str().unwrap()
        );
        let example: PasskeyAuthorization =
            serde_json::from_value(case["example_payout_passkey"].clone()).unwrap();
        verify_payout_passkey(a, &example, &identity).unwrap();
        // The same assertion is refused on another chain, network, and for the
        // other case's authorization (other action, nonce and epoch).
        assert!(verify_payout_passkey(a, &example, &ChainIdentity::new("777", "testnet")).is_err());
        assert!(verify_payout_passkey(a, &example, &ChainIdentity::new("888", "mainnet")).is_err());
        let other: NodeBondLifecycle = serde_json::from_str(
            cases[(i + 1) % cases.len()]["unsigned_lifecycle_json"].as_str().unwrap(),
        )
        .unwrap();
        assert!(verify_payout_passkey(&other.signed.authorization, &example, &identity).is_err());
        // The filled lifecycle round-trips through JSON with the member intact.
        let mut filled = unsigned.clone();
        filled.payout_passkey = Some(example.clone());
        let json = serde_json::to_string(&filled).unwrap();
        let back: NodeBondLifecycle = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payout_passkey, Some(example));
    }
}
