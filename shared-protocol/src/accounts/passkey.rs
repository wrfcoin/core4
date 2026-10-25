//! Passkey / WebAuthn (P-256) account authorization (#2169, epic #2167).
//!
//! # What this is
//!
//! A weather-station operator will never custody a seed phrase. This module
//! lets a WRFCoin account be **controlled by a platform passkey**: the account
//! address is derived from a WebAuthn credential's P-256 public key, and a
//! transparent transfer debiting that account is authorised by a WebAuthn
//! assertion whose challenge is bound to the transfer — not by an ed25519
//! signature.
//!
//! # What it is NOT
//!
//! - It does **not** weaken or touch the ed25519 path. An ed25519-controlled
//!   account is still authorised only by `TransparentTransferPayload::
//!   verify_authorization`, byte-for-byte as before. A passkey account and an
//!   ed25519 account can never share an address (the derivation is
//!   domain-separated: [`PASSKEY_ADDRESS_DOMAIN`] is folded in front of the key,
//!   and the ed25519 derivation hashes the bare 32-byte key), so a P-256 key can
//!   never claim an ed25519 account nor the reverse.
//! - It is **not** a bare P-256 signature check. [`verify_webauthn`] performs the
//!   full WebAuthn assertion verification: `clientDataJSON.type ==
//!   "webauthn.get"`, the challenge equals the base64url of the transfer's
//!   challenge, the RP ID hash in `authenticatorData` equals the credential's,
//!   the user-present flag is set, user-verified is set when the policy demands
//!   it, the signature counter has not regressed, and the ECDSA-P256-SHA256
//!   signature over `authenticatorData || SHA256(clientDataJSON)` verifies in
//!   low-`s` form.
//! - It is **not** active until scheduled. Every consensus site (block
//!   validation, state apply) asks `feature_active(ConsensusFeature::
//!   PasskeyP256Auth, height)`; below the activation height a passkey-authorised
//!   transfer is refused exactly as an unauthorised transfer is today.
//!
//! # Domain separation
//!
//! | tag | purpose |
//! |---|---|
//! | [`PASSKEY_TRANSFER_AUTH_DOMAIN`] | the transfer authorization message a passkey assertion's challenge commits to; distinct from `wrfcoin_transparent_transfer_auth_v2` (ed25519) so neither signature is a valid authorization under the other scheme |
//! | [`PASSKEY_ADDRESS_DOMAIN`] | address derivation from `(rp_id_hash, P-256 key)` |
//!
//! # Signature-counter policy
//!
//! WebAuthn §6.1.1: a decreasing counter indicates a cloned authenticator.
//! [`verify_webauthn`] enforces the specification's rule — if either the stored
//! counter or the asserted counter is non-zero, the asserted counter must be
//! strictly greater than the stored one; authenticators that do not implement
//! counters report `0` and are accepted. On the **transaction path the chain
//! keeps no per-credential counter state yet** (that is credential-registry
//! state, which arrives with recovery/delegation and needs the #3091 migration
//! framework), so `PasskeyAuthorization::verify_for_transfer` verifies with a
//! stored counter of `0`: replay of a transfer is prevented by the account
//! nonce and the challenge binding, and counter regression becomes enforceable
//! the moment a stored counter exists — the verifier already implements it and
//! is tested against it.
//!
//! # Malleability
//!
//! ECDSA is malleable. A relayer that flipped `s` to `n - s` would produce a
//! second, distinct transaction body carrying an equally valid signature. The
//! verifier therefore refuses high-`s` signatures
//! ([`PasskeyError::Curve`]`(P256Error::HighS)`); clients normalise `s` before
//! submitting (every authenticator's output is equally valid after
//! normalisation, so this rejects no legitimate user).

use crate::common::types::Address;
use crate::crypto::chain_binding::{ConsensusFeature, feature_active, feature_scheduled};
use crate::crypto::domain_signing::{signing_chain_id, signing_network_id};
use crate::crypto::hash::hash_bytes;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

pub use wrfcoin_privacy::p256_ecdsa::{
    P256Error, P256PublicKey, P256Signature, P256SigningKey, RAW_XY_LEN, verify_sha256,
    verify_sha256_low_s,
};

/// Domain separator for the transfer authorization a passkey assertion's
/// challenge commits to. Distinct from the ed25519
/// `wrfcoin_transparent_transfer_auth_v2` tag.
pub const PASSKEY_TRANSFER_AUTH_DOMAIN: &[u8] = b"wrfcoin_passkey_transfer_auth_v1";

/// Domain separator for passkey account-address derivation.
pub const PASSKEY_ADDRESS_DOMAIN: &[u8] = b"wrfcoin.passkey_address.v1";

/// The WebAuthn ceremony an authorization must come from.
pub const WEBAUTHN_GET: &str = "webauthn.get";

/// `rpIdHash (32) || flags (1) || signCount (4)`; extensions and attested
/// credential data may follow.
pub const AUTHENTICATOR_DATA_MIN_LEN: usize = 37;

/// `authenticatorData` flag bits (WebAuthn §6.1).
pub const FLAG_USER_PRESENT: u8 = 0x01;
/// `authenticatorData` flag bits (WebAuthn §6.1).
pub const FLAG_USER_VERIFIED: u8 = 0x04;

/// Length of the RP ID hash.
pub const RP_ID_HASH_LEN: usize = 32;

/// Upper bound on `clientDataJSON` and `authenticatorData` a verifier will
/// parse. Real values are a few hundred bytes; this bounds the work a
/// crafted transaction can force before the (cheap) structural checks fail.
pub const MAX_WEBAUTHN_FIELD_LEN: usize = 4096;

/// A registered WebAuthn credential that controls an account.
///
/// Carries no secret material. `credential_id` is the authenticator's PUBLIC
/// identifier — the relying party stores it and sends it back in
/// `allowCredentials` on every assertion, so it is published by design — and
/// `public_key_p256` is a public key. The private key never leaves the
/// authenticator and has no representation in this process at all.
#[derive(Clone, Debug, PartialEq, Eq)] // secret-leak-ok: `credential_id` matches the secret-name heuristic, but a WebAuthn credential ID is a public handle by specification (WebAuthn §5.1); no field here is secret material
pub struct PasskeyCredential {
    /// The authenticator's credential ID (opaque; informational on-chain).
    pub credential_id: Vec<u8>,
    /// Raw `X || Y` P-256 public key (the COSE EC2 coordinates).
    pub public_key_p256: [u8; RAW_XY_LEN],
    /// `SHA-256(rpId)` the credential was created for.
    pub rp_id_hash: [u8; RP_ID_HASH_LEN],
    /// Last signature counter seen for this credential (`0` when none).
    pub sign_count: u32,
}

/// A WebAuthn assertion (`AuthenticatorAssertionResponse`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebAuthnAssertion {
    /// `authenticatorData`, raw.
    pub authenticator_data: Vec<u8>,
    /// `clientDataJSON`, raw UTF-8 bytes exactly as the client produced them
    /// (the signature covers their SHA-256, so they are never re-serialised).
    pub client_data_json: Vec<u8>,
    /// ASN.1 DER `ECDSA-Sig-Value`, low-`s` normalised.
    pub signature: Vec<u8>,
}

/// What a verifier demands of the authenticator flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebAuthnPolicy {
    /// Require the user-verified flag (biometric / PIN), not just presence.
    pub require_user_verification: bool,
}

/// The consensus policy for funds-moving authorizations: presence AND
/// verification. A session-key story (#2170) may relax it for scoped keys.
pub const CONSENSUS_WEBAUTHN_POLICY: WebAuthnPolicy = WebAuthnPolicy {
    require_user_verification: true,
};

impl Default for WebAuthnPolicy {
    fn default() -> Self {
        CONSENSUS_WEBAUTHN_POLICY
    }
}

/// Why an assertion was refused. Every variant is a distinct, tested reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasskeyError {
    /// A field exceeds [`MAX_WEBAUTHN_FIELD_LEN`].
    FieldTooLong { field: &'static str, len: usize },
    /// `clientDataJSON` is not a JSON object with `type` and `challenge`.
    ClientDataNotJson,
    /// `clientDataJSON.type` is not `webauthn.get`.
    WrongCeremonyType { found: String },
    /// `clientDataJSON.challenge` is not the base64url of the expected challenge.
    ChallengeMismatch,
    /// `authenticatorData` is shorter than [`AUTHENTICATOR_DATA_MIN_LEN`].
    AuthenticatorDataTooShort { len: usize },
    /// `authenticatorData.rpIdHash` differs from the credential's.
    RpIdMismatch,
    /// The user-present flag is clear.
    UserNotPresent,
    /// The policy requires user verification and the flag is clear.
    UserNotVerified,
    /// The asserted signature counter did not advance past the stored one.
    CounterRegression { previous: u32, observed: u32 },
    /// The P-256 key or signature is malformed, high-`s`, or does not verify.
    Curve(P256Error),
}

impl std::fmt::Display for PasskeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FieldTooLong { field, len } => {
                write!(
                    f,
                    "webauthn {field} is {len} bytes, over the {MAX_WEBAUTHN_FIELD_LEN} limit"
                )
            },
            Self::ClientDataNotJson => {
                f.write_str("clientDataJSON is not a valid client-data object")
            },
            Self::WrongCeremonyType { found } => {
                write!(
                    f,
                    "clientDataJSON.type is {found:?}, expected {WEBAUTHN_GET:?}"
                )
            },
            Self::ChallengeMismatch => {
                f.write_str("clientDataJSON.challenge is not the expected challenge")
            },
            Self::AuthenticatorDataTooShort { len } => {
                write!(
                    f,
                    "authenticatorData is {len} bytes, under the {AUTHENTICATOR_DATA_MIN_LEN} minimum"
                )
            },
            Self::RpIdMismatch => {
                f.write_str("authenticatorData.rpIdHash does not match the credential")
            },
            Self::UserNotPresent => f.write_str("user-present flag is not set"),
            Self::UserNotVerified => f.write_str("user-verified flag is required and not set"),
            Self::CounterRegression { previous, observed } => write!(
                f,
                "signature counter regressed: stored {previous}, asserted {observed} (cloned authenticator?)"
            ),
            Self::Curve(inner) => write!(f, "p256: {inner}"),
        }
    }
}

impl std::error::Error for PasskeyError {}

impl From<P256Error> for PasskeyError {
    fn from(inner: P256Error) -> Self {
        Self::Curve(inner)
    }
}

/// What a successful verification established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedAssertion {
    /// The counter the authenticator asserted (store it as the new
    /// `sign_count`).
    pub sign_count: u32,
    /// Whether the user-verified flag was set.
    pub user_verified: bool,
}

/// The subset of `CollectedClientData` the verifier reads. Unknown members
/// (`origin`, `crossOrigin`, `tokenBinding`, …) are tolerated: the signature
/// covers the raw bytes, so nothing here is re-serialised.
#[derive(Deserialize)]
struct CollectedClientData {
    #[serde(rename = "type")]
    ceremony: String,
    challenge: String,
}

/// base64url without padding (RFC 4648 §5), the encoding WebAuthn uses for
/// `clientDataJSON.challenge`.
pub fn base64url_nopad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        }
    }
    out
}

/// Full WebAuthn assertion verification against `credential` for `challenge`.
///
/// Order of checks is cheapest-first so a crafted assertion fails before any
/// curve arithmetic: lengths, client data, RP ID, flags, counter, then the
/// signature.
pub fn verify_webauthn(
    credential: &PasskeyCredential,
    assertion: &WebAuthnAssertion,
    challenge: &[u8],
    policy: WebAuthnPolicy,
) -> Result<VerifiedAssertion, PasskeyError> {
    if assertion.client_data_json.len() > MAX_WEBAUTHN_FIELD_LEN {
        return Err(PasskeyError::FieldTooLong {
            field: "clientDataJSON",
            len: assertion.client_data_json.len(),
        });
    }
    if assertion.authenticator_data.len() > MAX_WEBAUTHN_FIELD_LEN {
        return Err(PasskeyError::FieldTooLong {
            field: "authenticatorData",
            len: assertion.authenticator_data.len(),
        });
    }

    // 1. clientDataJSON: ceremony type and challenge.
    let client_data: CollectedClientData = serde_json::from_slice(&assertion.client_data_json)
        .map_err(|_| PasskeyError::ClientDataNotJson)?;
    if client_data.ceremony != WEBAUTHN_GET {
        return Err(PasskeyError::WrongCeremonyType {
            found: client_data.ceremony,
        });
    }
    if client_data.challenge != base64url_nopad(challenge) {
        return Err(PasskeyError::ChallengeMismatch);
    }

    // 2. authenticatorData: RP ID, flags, counter.
    let auth_data = &assertion.authenticator_data;
    if auth_data.len() < AUTHENTICATOR_DATA_MIN_LEN {
        return Err(PasskeyError::AuthenticatorDataTooShort {
            len: auth_data.len(),
        });
    }
    if auth_data[..RP_ID_HASH_LEN] != credential.rp_id_hash {
        return Err(PasskeyError::RpIdMismatch);
    }
    let flags = auth_data[RP_ID_HASH_LEN];
    if flags & FLAG_USER_PRESENT == 0 {
        return Err(PasskeyError::UserNotPresent);
    }
    let user_verified = flags & FLAG_USER_VERIFIED != 0;
    if policy.require_user_verification && !user_verified {
        return Err(PasskeyError::UserNotVerified);
    }
    let observed = u32::from_be_bytes([auth_data[33], auth_data[34], auth_data[35], auth_data[36]]);
    // WebAuthn §6.1.1: if either counter is non-zero, the asserted counter
    // must be strictly greater than the stored one.
    if (credential.sign_count != 0 || observed != 0) && observed <= credential.sign_count {
        return Err(PasskeyError::CounterRegression {
            previous: credential.sign_count,
            observed,
        });
    }

    // 3. Signature over authenticatorData || SHA256(clientDataJSON).
    let key = P256PublicKey::from_raw_xy(&credential.public_key_p256)?;
    let signature = P256Signature::from_der(&assertion.signature)?;
    let mut signed = Vec::with_capacity(auth_data.len() + 32);
    signed.extend_from_slice(auth_data);
    signed.extend_from_slice(&Sha256::digest(&assertion.client_data_json));
    verify_sha256_low_s(&key, &signed, &signature)?;

    Ok(VerifiedAssertion {
        sign_count: observed,
        user_verified,
    })
}

/// Derive the account address a passkey credential controls.
///
/// `sha3_256(PASSKEY_ADDRESS_DOMAIN || rp_id_hash || 0x04 || X || Y)[..20]`.
/// Folding the RP ID hash means the same P-256 key registered under two relying
/// parties controls two accounts, and folding the domain tag means no ed25519
/// key (whose address is `sha3_256(key)[..20]`) can derive a passkey address.
pub fn passkey_address_from_parts(
    rp_id_hash: &[u8; RP_ID_HASH_LEN],
    public_key_p256: &[u8; RAW_XY_LEN],
) -> Address {
    let mut preimage =
        Vec::with_capacity(PASSKEY_ADDRESS_DOMAIN.len() + RP_ID_HASH_LEN + 1 + RAW_XY_LEN);
    preimage.extend_from_slice(PASSKEY_ADDRESS_DOMAIN);
    preimage.extend_from_slice(rp_id_hash);
    preimage.push(0x04);
    preimage.extend_from_slice(public_key_p256);
    let digest = hash_bytes(&preimage);
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&digest.as_bytes()[..20]);
    Address(bytes)
}

/// [`passkey_address_from_parts`] for a credential.
pub fn passkey_account_address(credential: &PasskeyCredential) -> Address {
    passkey_address_from_parts(&credential.rp_id_hash, &credential.public_key_p256)
}

/// The transfer authorization message a passkey assertion commits to, under
/// the runtime chain identity.
pub fn passkey_transfer_authorization_message(
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    passkey_transfer_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        from,
        to,
        amount,
        nonce,
    )
}

/// Pure form of [`passkey_transfer_authorization_message`] with the chain
/// identity passed explicitly (golden vectors / cross-signer parity). Same
/// framing as the ed25519 message, under a different tag.
pub fn passkey_transfer_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(PASSKEY_TRANSFER_AUTH_DOMAIN.len() + 96);
    buf.extend_from_slice(PASSKEY_TRANSFER_AUTH_DOMAIN);
    buf.extend_from_slice(&(chain_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(chain_id.as_bytes());
    buf.extend_from_slice(&(network_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(network_id.as_bytes());
    buf.extend_from_slice(&(from.len() as u32).to_le_bytes());
    buf.extend_from_slice(from.as_bytes());
    buf.extend_from_slice(&(to.len() as u32).to_le_bytes());
    buf.extend_from_slice(to.as_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());
    buf.extend_from_slice(&nonce.to_le_bytes());
    buf
}

/// The WebAuthn challenge for a transfer: `sha3_256(message)`. A 32-byte
/// challenge is what a client passes to `navigator.credentials.get`.
pub fn passkey_transfer_challenge(message: &[u8]) -> [u8; 32] {
    *hash_bytes(message).as_bytes()
}

/// A passkey authorization as carried in a `TransparentTransferPayload`
/// (`passkey_auth`). All fields are hex so the payload stays a plain JSON
/// object; `client_data_json` is the raw bytes hex-encoded because the
/// signature covers their exact SHA-256.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyAuthorization {
    /// Hex of the raw `X || Y` P-256 public key (64 bytes).
    pub public_key_p256: String,
    /// Hex of `SHA-256(rpId)` (32 bytes).
    pub rp_id_hash: String,
    /// Hex of `authenticatorData`.
    pub authenticator_data: String,
    /// Hex of the raw `clientDataJSON` bytes.
    pub client_data_json: String,
    /// Hex of the DER-encoded, low-`s` ECDSA signature.
    pub signature: String,
}

/// Why a payload-level passkey authorization was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasskeyAuthorizationError {
    /// A hex field is malformed or of the wrong length.
    MalformedField(&'static str),
    /// `from` is not a parseable address.
    FromNotAnAddress,
    /// The credential does not derive the `from` account.
    FromNotBoundToCredential,
    /// The assertion failed verification.
    Assertion(PasskeyError),
}

impl std::fmt::Display for PasskeyAuthorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedField(field) => {
                write!(f, "passkey authorization field {field} is malformed")
            },
            Self::FromNotAnAddress => f.write_str("from is not an address"),
            Self::FromNotBoundToCredential => {
                f.write_str("the passkey credential does not control the from account")
            },
            Self::Assertion(inner) => write!(f, "webauthn: {inner}"),
        }
    }
}

impl std::error::Error for PasskeyAuthorizationError {}

fn hex_field<const N: usize>(
    value: &str,
    field: &'static str,
) -> Result<[u8; N], PasskeyAuthorizationError> {
    let bytes = hex::decode(value).map_err(|_| PasskeyAuthorizationError::MalformedField(field))?;
    bytes
        .try_into()
        .map_err(|_| PasskeyAuthorizationError::MalformedField(field))
}

impl PasskeyAuthorization {
    /// The credential this authorization names, with a stored counter of `0`
    /// (the transaction path keeps no credential state yet — see the module
    /// docs).
    pub fn credential(&self) -> Result<PasskeyCredential, PasskeyAuthorizationError> {
        Ok(PasskeyCredential {
            credential_id: Vec::new(),
            public_key_p256: hex_field::<RAW_XY_LEN>(&self.public_key_p256, "public_key_p256")?,
            rp_id_hash: hex_field::<RP_ID_HASH_LEN>(&self.rp_id_hash, "rp_id_hash")?,
            sign_count: 0,
        })
    }

    /// The assertion this authorization carries.
    pub fn assertion(&self) -> Result<WebAuthnAssertion, PasskeyAuthorizationError> {
        Ok(WebAuthnAssertion {
            authenticator_data: hex::decode(&self.authenticator_data)
                .map_err(|_| PasskeyAuthorizationError::MalformedField("authenticator_data"))?,
            client_data_json: hex::decode(&self.client_data_json)
                .map_err(|_| PasskeyAuthorizationError::MalformedField("client_data_json"))?,
            signature: hex::decode(&self.signature)
                .map_err(|_| PasskeyAuthorizationError::MalformedField("signature"))?,
        })
    }

    /// The account this authorization's credential controls.
    pub fn account(&self) -> Result<Address, PasskeyAuthorizationError> {
        Ok(passkey_account_address(&self.credential()?))
    }

    /// Verify this authorization for a transfer `from -> to` of `amount` at
    /// `nonce` under the runtime chain identity. Fail-closed: the credential
    /// must derive `from`, and the assertion must pass [`verify_webauthn`]
    /// under [`CONSENSUS_WEBAUTHN_POLICY`] for the transfer's challenge.
    ///
    /// This is the **ungated** cryptographic check; callers on the consensus
    /// path gate it by `feature_active(ConsensusFeature::PasskeyP256Auth, h)`
    /// (see `TransparentTransferPayload::verify_authorization_at_height`).
    pub fn verify_for_transfer(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
    ) -> Result<VerifiedAssertion, PasskeyAuthorizationError> {
        let credential = self.credential()?;
        let from_address = from
            .parse::<Address>()
            .map_err(|_| PasskeyAuthorizationError::FromNotAnAddress)?;
        if passkey_account_address(&credential) != from_address {
            return Err(PasskeyAuthorizationError::FromNotBoundToCredential);
        }
        let assertion = self.assertion()?;
        let message = passkey_transfer_authorization_message(from, to, amount, nonce);
        let challenge = passkey_transfer_challenge(&message);
        verify_webauthn(
            &credential,
            &assertion,
            &challenge,
            CONSENSUS_WEBAUTHN_POLICY,
        )
        .map_err(PasskeyAuthorizationError::Assertion)
    }
}

/// Lenient deserializer for the optional `passkey_auth` payload field.
///
/// A pre-#2169 binary ignores an unknown `passkey_auth` member entirely. For
/// pre-activation behaviour to stay byte-identical, a *malformed* member must
/// not turn a payload that the old binary parses (and, if ed25519-authorised,
/// accepts) into one the new binary refuses to parse. So a member that is not
/// a well-formed [`PasskeyAuthorization`] reads as `None` — which post-
/// activation is simply "no passkey authorization", refused unless the
/// ed25519 authorization is present and valid.
pub fn deserialize_lenient_passkey<'de, D>(
    deserializer: D,
) -> Result<Option<PasskeyAuthorization>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| serde_json::from_value::<PasskeyAuthorization>(value).ok()))
}

/// Whether [`ConsensusFeature::PasskeyP256Auth`] is in force at `height`.
pub fn passkey_auth_active(height: u64) -> bool {
    feature_active(ConsensusFeature::PasskeyP256Auth, height)
}

/// Whether this chain schedules [`ConsensusFeature::PasskeyP256Auth`] at all.
///
/// The mempool has no notion of height; it admits a passkey-authorised
/// transfer only on a chain that schedules the feature — in its genesis /
/// bootstrap base schedule OR through an applied governance schedule
/// extension (the same source of truth as [`passkey_auth_active`]) — and
/// leaves the exact-height gate to the producer self-screen, block validation
/// and state apply (all height-aware). On a chain that never schedules it,
/// this is `false` and admission behaves exactly as before #2169.
///
/// Policy only (mempool/API): no block-validity or state-apply rule reads it.
pub fn passkey_auth_scheduled() -> bool {
    feature_scheduled(ConsensusFeature::PasskeyP256Auth)
}

/// [`crate::accounts::abstraction::SignatureVerifier`] over WebAuthn: the
/// abstraction hook the delegation story (#2175) builds on.
///
/// Encoding: `signer_public_key` is `rp_id_hash (32) || X || Y (64)`;
/// `signature` is the JSON of a [`WebAuthnAssertion`]; `message` is the
/// challenge.
#[derive(Clone, Copy, Debug, Default)]
pub struct PasskeySignatureVerifier {
    /// Flag policy applied to every assertion.
    pub policy: WebAuthnPolicy,
}

impl crate::accounts::abstraction::SignatureVerifier for PasskeySignatureVerifier {
    fn verify(&self, message: &[u8], signature: &[u8], signer_public_key: &[u8]) -> bool {
        if signer_public_key.len() != RP_ID_HASH_LEN + RAW_XY_LEN {
            return false;
        }
        let mut rp_id_hash = [0u8; RP_ID_HASH_LEN];
        rp_id_hash.copy_from_slice(&signer_public_key[..RP_ID_HASH_LEN]);
        let mut public_key_p256 = [0u8; RAW_XY_LEN];
        public_key_p256.copy_from_slice(&signer_public_key[RP_ID_HASH_LEN..]);
        let credential = PasskeyCredential {
            credential_id: Vec::new(),
            public_key_p256,
            rp_id_hash,
            sign_count: 0,
        };
        let Ok(assertion) = serde_json::from_slice::<WebAuthnAssertion>(signature) else {
            return false;
        };
        verify_webauthn(&credential, &assertion, message, self.policy).is_ok()
    }
}

/// Fixture builders: a software authenticator that produces conforming
/// assertions. Test-only; the chain never signs P-256.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;

    /// A software authenticator for one credential.
    pub struct SoftwareAuthenticator {
        /// Its signing key.
        pub key: P256SigningKey,
        /// `SHA-256(rp_id)`.
        pub rp_id_hash: [u8; RP_ID_HASH_LEN],
        /// Counter it will assert next.
        pub next_sign_count: u32,
        /// Flags it will set.
        pub flags: u8,
        /// `origin` member written into `clientDataJSON`.
        pub origin: String,
    }

    impl SoftwareAuthenticator {
        /// A deterministic authenticator for `rp_id` from a fixed secret.
        pub fn new(secret: &[u8; 32], rp_id: &str) -> Self {
            Self {
                key: P256SigningKey::from_bytes(secret).expect("secret scalar in range"),
                rp_id_hash: Sha256::digest(rp_id.as_bytes()).into(),
                next_sign_count: 1,
                flags: FLAG_USER_PRESENT | FLAG_USER_VERIFIED,
                origin: format!("https://{rp_id}"),
            }
        }

        /// The credential a relying party would store.
        pub fn credential(&self, last_sign_count: u32) -> PasskeyCredential {
            PasskeyCredential {
                credential_id: b"fixture-credential".to_vec(),
                public_key_p256: self.key.public_key().to_raw_xy(),
                rp_id_hash: self.rp_id_hash,
                sign_count: last_sign_count,
            }
        }

        /// The account this authenticator controls.
        pub fn account(&self) -> Address {
            passkey_account_address(&self.credential(0))
        }

        /// `clientDataJSON` for `challenge`.
        pub fn client_data_json(&self, challenge: &[u8]) -> Vec<u8> {
            format!(
                "{{\"type\":\"{WEBAUTHN_GET}\",\"challenge\":\"{}\",\"origin\":\"{}\",\"crossOrigin\":false}}",
                base64url_nopad(challenge),
                self.origin
            )
            .into_bytes()
        }

        /// `authenticatorData` with this authenticator's flags and counter.
        pub fn authenticator_data(&self) -> Vec<u8> {
            let mut data = Vec::with_capacity(AUTHENTICATOR_DATA_MIN_LEN);
            data.extend_from_slice(&self.rp_id_hash);
            data.push(self.flags);
            data.extend_from_slice(&self.next_sign_count.to_be_bytes());
            data
        }

        /// A conforming assertion for `challenge`.
        pub fn assert(&self, challenge: &[u8]) -> WebAuthnAssertion {
            let authenticator_data = self.authenticator_data();
            let client_data_json = self.client_data_json(challenge);
            self.assert_with(authenticator_data, client_data_json)
        }

        /// Sign arbitrary `authenticator_data` / `client_data_json` (for
        /// building deliberately non-conforming assertions).
        pub fn assert_with(
            &self,
            authenticator_data: Vec<u8>,
            client_data_json: Vec<u8>,
        ) -> WebAuthnAssertion {
            let mut signed = authenticator_data.clone();
            signed.extend_from_slice(&Sha256::digest(&client_data_json));
            let signature = self.key.sign_sha256_low_s(&signed).to_der_bytes();
            WebAuthnAssertion {
                authenticator_data,
                client_data_json,
                signature,
            }
        }

        /// A payload-level authorization for a transfer from this account.
        pub fn authorize_transfer(
            &self,
            to: &str,
            amount: u64,
            nonce: u64,
        ) -> PasskeyAuthorization {
            let from = self.account().to_hex();
            let message = passkey_transfer_authorization_message(&from, to, amount, nonce);
            let challenge = passkey_transfer_challenge(&message);
            self.authorization_for(&self.assert(&challenge))
        }

        /// Wrap an assertion as a payload-level authorization.
        pub fn authorization_for(&self, assertion: &WebAuthnAssertion) -> PasskeyAuthorization {
            PasskeyAuthorization {
                public_key_p256: hex::encode(self.key.public_key().to_raw_xy()),
                rp_id_hash: hex::encode(self.rp_id_hash),
                authenticator_data: hex::encode(&assertion.authenticator_data),
                client_data_json: hex::encode(&assertion.client_data_json),
                signature: hex::encode(&assertion.signature),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::SoftwareAuthenticator;
    use super::*;

    const SECRET: [u8; 32] = [
        0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
        0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        0x48, 0x49,
    ];

    #[test]
    fn base64url_matches_rfc4648_vectors() {
        assert_eq!(base64url_nopad(b""), "");
        assert_eq!(base64url_nopad(b"f"), "Zg");
        assert_eq!(base64url_nopad(b"fo"), "Zm8");
        assert_eq!(base64url_nopad(b"foo"), "Zm9v");
        assert_eq!(base64url_nopad(b"foob"), "Zm9vYg");
        assert_eq!(base64url_nopad(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url_nopad(b"foobar"), "Zm9vYmFy");
        // URL alphabet: 0xfb 0xff -> "-_8" under base64url, "+/8" under base64.
        assert_eq!(base64url_nopad(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn a_conforming_assertion_verifies_and_reports_the_counter() {
        let auth = SoftwareAuthenticator::new(&SECRET, "wallet.wrfcoin.test");
        let challenge = [7u8; 32];
        let assertion = auth.assert(&challenge);
        let verified = verify_webauthn(
            &auth.credential(0),
            &assertion,
            &challenge,
            WebAuthnPolicy::default(),
        )
        .expect("conforming assertion");
        assert_eq!(verified.sign_count, 1);
        assert!(verified.user_verified);
    }

    #[test]
    fn the_address_derivation_is_domain_separated_from_ed25519() {
        let auth = SoftwareAuthenticator::new(&SECRET, "wallet.wrfcoin.test");
        let credential = auth.credential(0);
        let address = passkey_account_address(&credential);
        // Not the bare-key hash an ed25519 address uses…
        let bare = hash_bytes(&credential.public_key_p256);
        assert_ne!(&address.0[..], &bare.as_bytes()[..20]);
        // …and different under a different relying party.
        let other = SoftwareAuthenticator::new(&SECRET, "other.example");
        assert_ne!(other.account(), address);
        assert_eq!(auth.account(), address);
    }

    #[test]
    fn the_transfer_message_is_under_its_own_tag() {
        let ours = passkey_transfer_authorization_message_with_domain(
            "wrfcoin-testnet",
            "testnet",
            "0xaa",
            "0xbb",
            5,
            9,
        );
        let ed25519 =
            crate::blockchain::transaction::transparent_transfer_authorization_message_with_domain(
                "wrfcoin-testnet",
                "testnet",
                "0xaa",
                "0xbb",
                5,
                9,
            );
        assert!(ours.starts_with(PASSKEY_TRANSFER_AUTH_DOMAIN));
        assert_ne!(ours, ed25519);
        // Same framing after the tag: only the tag differs.
        assert_eq!(
            &ours[PASSKEY_TRANSFER_AUTH_DOMAIN.len()..],
            &ed25519[crate::blockchain::transaction::TRANSPARENT_TRANSFER_AUTH_DOMAIN.len()..]
        );
    }
}
