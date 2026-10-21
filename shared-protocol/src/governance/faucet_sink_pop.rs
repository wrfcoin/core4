//! Faucet-sink proof of possession (#3361 F3, owner ruling Q4 2026-09-28).
//!
//! Before governance signs the extension that points 1% of emission at a
//! [`LEGACY_FAUCET_BINDINGS`](crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS)
//! row's address (E0f), whoever holds that address's key signs a
//! [`FaucetSinkPossession`] claim. A typo'd or keyless address would
//! otherwise mint into a void for at least `MIN_RUNUP_BLOCKS` until a
//! rotation activates.
//!
//! **Which key.** A WRFCoin account address is
//! `0x ‖ hex(SHA3-256(ed25519 public key)[..20])`
//! ([`Address::from_public_key`]; the wallet derives the key by SLIP-0010
//! from a BIP-39 seed, core4#3234). The faucet address is such an account,
//! so possession is proven by an **ed25519** signature from the key whose
//! public key hashes to it — the same key the faucet service uses to sign
//! its transparent transfers. The verifier recomputes the address from the
//! presented public key and compares it with the row; a key that does not
//! control the address is refused before any curve operation.
//!
//! **Not consensus.** The proof never rides in a block and no node checks
//! it. It gates the SIGNING of the governance extension: the
//! `wrfcoin governance schedule-extension sign` CLI and every library
//! signer of a `faucet_sink_binding*` extension
//! ([`super::schedule_extension::sign_faucet_sink_binding_extension`]).
//!
//! **Preimage.** Its own domain tag, `wrfcoin.faucet_sink_pop.v1`
//! ([`DomainSigningAction::FaucetSinkPossession`]), framed by
//! [`domain_preimage_bound`](crate::crypto::chain_binding::domain_preimage_bound)
//! at [`SigningProtocolVersion::V2`] over the chain and network NAMED IN THE
//! CLAIM, with an explicit length-prefixed [`BodyBuilder`] body — never
//! bincode. The body repeats `chain_id` and `network_id` and adds the
//! block-1 hash and the address, so the proof is bound to one chain
//! INSTANCE and one address: it cannot be replayed on a re-cut chain that
//! reuses the id, for another address, or as a signature in any other
//! domain (a transfer authorization, a schedule extension).

use serde::{Deserialize, Serialize};

use crate::common::types::Address;
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::DomainSeparated;
#[cfg(doc)]
use crate::crypto::domain_signing::DomainSigningAction;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};

use super::schedule_extension::is_canonical_genesis_hash;

/// The signing protocol version every possession claim is bound at: chain
/// bound by construction, like the extension it unlocks.
pub const FAUCET_SINK_POP_SIGNING_VERSION: SigningProtocolVersion = SigningProtocolVersion::V2;

/// What the faucet key attests: "I control `address`, and I consent to it
/// being the faucet sink of the chain instance `genesis_hash`".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FaucetSinkPossession {
    pub chain_id: String,
    pub network_id: String,
    /// Lowercase hex of the chain's durable block-1 hash, no prefix.
    pub genesis_hash: String,
    /// Canonical `0x` + 40 lowercase hex digits ([`Address`]'s `Display`).
    pub address: String,
}

impl FaucetSinkPossession {
    /// A claim in canonical form: the address is rendered by [`Address`],
    /// so one address has exactly one signable spelling.
    pub fn new(
        chain_id: impl Into<String>,
        network_id: impl Into<String>,
        genesis_hash: impl Into<String>,
        address: Address,
    ) -> Self {
        Self {
            chain_id: chain_id.into(),
            network_id: network_id.into(),
            genesis_hash: genesis_hash.into(),
            address: address.to_string(),
        }
    }

    pub fn identity(&self) -> ChainIdentity {
        ChainIdentity::new(self.chain_id.clone(), self.network_id.clone())
    }

    /// The exact bytes the faucet key signs.
    pub fn preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(&self.identity(), FAUCET_SINK_POP_SIGNING_VERSION)
    }
}

impl DomainSeparated for FaucetSinkPossession {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.faucet_sink_pop.v1";

    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .str_field(&self.chain_id)
            .str_field(&self.network_id)
            .str_field(&self.genesis_hash)
            .str_field(&self.address)
            .finish()
    }
}

/// A claim plus the faucet key's signature over its preimage — the artifact
/// the faucet key holder hands to the governance signer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FaucetSinkPossessionProof {
    pub possession: FaucetSinkPossession,
    /// Lowercase hex ed25519 public key of the faucet account.
    pub public_key: String,
    /// Hex ed25519 signature over [`FaucetSinkPossession::preimage`].
    pub signature: String,
}

/// Why a possession proof was refused. Every case is a named error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaucetSinkPopError {
    /// The proof names another chain or network.
    WrongChain { expected: ChainIdentity, found: ChainIdentity },
    /// The proof names another chain instance.
    WrongGenesis { expected: String, found: String },
    /// The proof names another address.
    WrongAddress { expected: String, found: String },
    /// The expected claim itself is malformed (not a canonical block-1
    /// hash, or not a canonical address).
    MalformedClaim { reason: String },
    /// The presented key is not valid ed25519 hex.
    MalformedPublicKey { reason: String },
    /// The presented key does not hash to the claimed address.
    KeyDoesNotControlAddress { address: String, key_address: String },
    /// The signature is malformed or does not verify over this claim's
    /// preimage (tampered, or signed in another domain or for another
    /// chain).
    BadSignature { reason: String },
}

impl std::fmt::Display for FaucetSinkPopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongChain { expected, found } => write!(
                f,
                "faucet proof of possession is for chain {found:?}, expected {expected:?}"
            ),
            Self::WrongGenesis { expected, found } => write!(
                f,
                "faucet proof of possession is bound to genesis {found}, expected {expected}"
            ),
            Self::WrongAddress { expected, found } => write!(
                f,
                "faucet proof of possession is for address {found}, the binding row names {expected}"
            ),
            Self::MalformedClaim { reason } => {
                write!(f, "faucet possession claim is malformed: {reason}")
            },
            Self::MalformedPublicKey { reason } => write!(
                f,
                "faucet proof of possession public key is not valid ed25519 hex: {reason}"
            ),
            Self::KeyDoesNotControlAddress {
                address,
                key_address,
            } => write!(
                f,
                "faucet proof of possession key controls {key_address}, not {address}"
            ),
            Self::BadSignature { reason } => {
                write!(f, "faucet proof of possession signature is invalid: {reason}")
            },
        }
    }
}

impl std::error::Error for FaucetSinkPopError {}

/// Sign `possession` with the faucet account's key.
pub fn sign_faucet_sink_possession(
    possession: &FaucetSinkPossession,
    key: &PrivateKey,
) -> FaucetSinkPossessionProof {
    FaucetSinkPossessionProof {
        possession: possession.clone(),
        public_key: PublicKey::from_private_key(key)
            .to_hex()
            .to_ascii_lowercase(),
        signature: key.sign(&possession.preimage()).to_hex(),
    }
}

/// Verify `proof` against the claim the VERIFIER expects (built from the
/// extension's identity and the binding row, never from the proof): the
/// proof names the same chain, instance and address; the presented key
/// hashes to that address; and its signature verifies over the EXPECTED
/// claim's preimage.
pub fn verify_faucet_sink_possession(
    proof: &FaucetSinkPossessionProof,
    expected: &FaucetSinkPossession,
) -> Result<(), FaucetSinkPopError> {
    if !is_canonical_genesis_hash(&expected.genesis_hash) {
        return Err(FaucetSinkPopError::MalformedClaim {
            reason: format!("genesis hash {:?} is not canonical", expected.genesis_hash),
        });
    }
    let expected_address =
        Address::from_hex(&expected.address).map_err(|e| FaucetSinkPopError::MalformedClaim {
            reason: format!("address {:?}: {e}", expected.address),
        })?;
    if expected_address.to_string() != expected.address {
        return Err(FaucetSinkPopError::MalformedClaim {
            reason: format!("address {:?} is not in canonical 0x-lowercase form", expected.address),
        });
    }

    let claim = &proof.possession;
    if claim.identity() != expected.identity() {
        return Err(FaucetSinkPopError::WrongChain {
            expected: expected.identity(),
            found: claim.identity(),
        });
    }
    if claim.genesis_hash != expected.genesis_hash {
        return Err(FaucetSinkPopError::WrongGenesis {
            expected: expected.genesis_hash.clone(),
            found: claim.genesis_hash.clone(),
        });
    }
    if claim.address != expected.address {
        return Err(FaucetSinkPopError::WrongAddress {
            expected: expected.address.clone(),
            found: claim.address.clone(),
        });
    }

    let public_key = PublicKey::from_hex(proof.public_key.trim()).map_err(|e| {
        FaucetSinkPopError::MalformedPublicKey {
            reason: e.to_string(),
        }
    })?;
    let key_address = Address::from_public_key(&public_key);
    if key_address != expected_address {
        return Err(FaucetSinkPopError::KeyDoesNotControlAddress {
            address: expected.address.clone(),
            key_address: key_address.to_string(),
        });
    }
    let signature = Signature::from_hex(proof.signature.trim()).map_err(|e| {
        FaucetSinkPopError::BadSignature {
            reason: format!("not valid hex: {e}"),
        }
    })?;
    if !public_key.verify(&expected.preimage(), &signature) {
        return Err(FaucetSinkPopError::BadSignature {
            reason: "does not verify over the faucet possession preimage — the claim was \
                     modified after signing, or the signature is from another domain or chain"
                .into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::chain_binding::domain_preimage_bound;
    use crate::crypto::domain_signing::push_length_prefixed;
    use crate::crypto::domain_signing::DomainSigningAction;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const GENESIS: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash

    fn faucet_key() -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(0x3361_f3))
    }

    fn faucet_address() -> Address {
        Address::from_public_key(&PublicKey::from_private_key(&faucet_key()))
    }

    fn claim() -> FaucetSinkPossession {
        FaucetSinkPossession::new("777", "testnet", GENESIS, faucet_address())
    }

    #[test]
    fn the_domain_tag_is_the_action_domain_and_is_its_own() {
        assert_eq!(
            // secret-leak-ok: test; `DomainSigningAction` / `DOMAIN_TAG` are enum and constant names, no key material
            FaucetSinkPossession::DOMAIN_TAG,
            DomainSigningAction::FaucetSinkPossession.domain().as_bytes()
        );
        assert_ne!(
            FaucetSinkPossession::DOMAIN_TAG,
            super::super::schedule_extension::FeatureScheduleExtension::DOMAIN_TAG
        );
    }

    /// Byte-pins the body against a hand-assembled encoding and the full
    /// preimage against the bound frame; a reordered or re-typed field, or a
    /// bincode "simplification", fails here.
    #[test]
    fn the_preimage_layout_is_pinned() {
        let possession = FaucetSinkPossession::new(
            "777",
            "testnet",
            GENESIS,
            Address::from_hex("f72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3").unwrap(), // pragma: allowlist secret -- public faucet ADDRESS
        );
        assert_eq!(possession.address, "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"); // pragma: allowlist secret -- public faucet ADDRESS
        let mut body = Vec::new();
        push_length_prefixed(&mut body, b"777");
        push_length_prefixed(&mut body, b"testnet");
        push_length_prefixed(&mut body, GENESIS.as_bytes());
        push_length_prefixed(&mut body, b"0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"); // pragma: allowlist secret -- public faucet ADDRESS
        assert_eq!(possession.canonical_body(), body);
        assert_eq!( // secret-leak-ok: compares the PoP preimage, built only from public chain id, network id, block-1 hash and the public faucet address; no key material
            possession.preimage(),
            domain_preimage_bound(
                b"wrfcoin.faucet_sink_pop.v1",
                &ChainIdentity::new("777", "testnet"),
                SigningProtocolVersion::V2,
                &body,
            )
        );
        // Golden: the full preimage, frozen. Any change here invalidates
        // every proof already produced offline by the faucet key holder.
        assert_eq!(hex::encode(possession.preimage()), FAUCET_SINK_POP_PREIMAGE_GOLDEN);
    }

    /// The pinned claim's full preimage, hex. Regenerate ONLY with a new
    /// domain tag (`…pop.v2`), never in place.
    const FAUCET_SINK_POP_PREIMAGE_GOLDEN: &str = "1a00000000000000777266636f696e2e6661756365745f73696e6b5f706f702e763103000000000000003737370700000000000000746573746e657402000000940000000000000003000000000000003737370700000000000000746573746e65744000000000000000366164343336333836636663366337343631343666366566326639643332646462376139336631386566643336333536303661626432376339636131376462382a00000000000000307866373230353963336234613664336334333139323063336366336534306462663638613662356433"; // pragma: allowlist secret -- public signing preimage (tag, chain, genesis hash, address), no key material

    #[test]
    fn a_proof_by_the_faucet_key_verifies() {
        let proof = sign_faucet_sink_possession(&claim(), &faucet_key());
        assert_eq!(verify_faucet_sink_possession(&proof, &claim()), Ok(()));
        // JSON round trip (the CLI artifact).
        let json = serde_json::to_string_pretty(&proof).unwrap();
        let back: FaucetSinkPossessionProof = serde_json::from_str(&json).unwrap();
        assert_eq!(verify_faucet_sink_possession(&back, &claim()), Ok(()));
    }

    #[test]
    fn a_proof_by_another_key_is_refused_even_over_the_right_claim() {
        let stranger = PrivateKey::generate(&mut StdRng::seed_from_u64(0xbad));
        let proof = sign_faucet_sink_possession(&claim(), &stranger);
        assert!(matches!(
            verify_faucet_sink_possession(&proof, &claim()),
            Err(FaucetSinkPopError::KeyDoesNotControlAddress { .. })
        ));
        // A stranger's signature presented under the faucet's public key.
        let forged = FaucetSinkPossessionProof {
            public_key: PublicKey::from_private_key(&faucet_key()).to_hex(),
            ..proof
        };
        assert!(matches!(
            verify_faucet_sink_possession(&forged, &claim()),
            Err(FaucetSinkPopError::BadSignature { .. })
        ));
    }

    #[test]
    fn a_proof_for_another_chain_instance_or_address_is_refused() {
        let key = faucet_key();
        let other_chain = FaucetSinkPossession {
            chain_id: "778".into(),
            ..claim()
        };
        let other_network = FaucetSinkPossession {
            network_id: "mainnet".into(),
            ..claim()
        };
        let other_genesis = FaucetSinkPossession {
            genesis_hash: "cd".repeat(32),
            ..claim()
        };
        let other_address = FaucetSinkPossession {
            address: Address([0x39; 20]).to_string(),
            ..claim()
        };
        for (what, signed_claim, expect) in [
            ("chain", other_chain, "WrongChain"),
            ("network", other_network, "WrongChain"),
            ("genesis", other_genesis, "WrongGenesis"),
            ("address", other_address, "WrongAddress"),
        ] {
            let proof = sign_faucet_sink_possession(&signed_claim, &key);
            let err = verify_faucet_sink_possession(&proof, &claim()).unwrap_err();
            assert!(format!("{err:?}").starts_with(expect), "{what}: {err:?}");
            // Relabelling the claim to the expected one does not help: the
            // signature is over the other instance's preimage.
            let relabelled = FaucetSinkPossessionProof {
                possession: claim(),
                ..proof
            };
            assert!(
                matches!(
                    verify_faucet_sink_possession(&relabelled, &claim()),
                    Err(FaucetSinkPopError::BadSignature { .. })
                ),
                "{what}"
            );
        }
    }

    /// A signature by the faucet key in ANOTHER domain over the same fields —
    /// the unframed body, the same body under another tag, or the V1
    /// (unbound) frame — never verifies as a possession proof.
    #[test]
    fn a_replayed_domain_is_refused() {
        let key = faucet_key();
        let body = claim().canonical_body();
        let identity = claim().identity();
        let foreign: Vec<(&str, Vec<u8>)> = vec![
            ("raw body", body.clone()),
            (
                "schedule-extension tag",
                domain_preimage_bound(
                    b"wrfcoin.feature_schedule_extension.v1",
                    &identity,
                    SigningProtocolVersion::V2,
                    &body,
                ),
            ),
            (
                "upgrade-notice tag",
                domain_preimage_bound(
                    b"wrfcoin.upgrade_notice.v1",
                    &identity,
                    SigningProtocolVersion::V2,
                    &body,
                ),
            ),
            ("V1 frame", claim().signing_preimage_for(&identity, SigningProtocolVersion::V1)),
        ];
        for (what, message) in foreign {
            assert_ne!(message, claim().preimage(), "{what}");
            let proof = FaucetSinkPossessionProof {
                possession: claim(),
                public_key: PublicKey::from_private_key(&key).to_hex(),
                signature: key.sign(&message).to_hex(),
            };
            assert!(
                matches!(
                    verify_faucet_sink_possession(&proof, &claim()),
                    Err(FaucetSinkPopError::BadSignature { .. })
                ),
                "{what}"
            );
        }
    }

    #[test]
    fn malformed_inputs_are_named_errors() {
        let proof = sign_faucet_sink_possession(&claim(), &faucet_key());
        let bad_key = FaucetSinkPossessionProof {
            public_key: "zz".into(),
            ..proof.clone()
        };
        assert!(matches!(
            verify_faucet_sink_possession(&bad_key, &claim()),
            Err(FaucetSinkPopError::MalformedPublicKey { .. })
        ));
        let bad_sig = FaucetSinkPossessionProof {
            signature: "zz".into(),
            ..proof.clone()
        };
        assert!(matches!(
            verify_faucet_sink_possession(&bad_sig, &claim()),
            Err(FaucetSinkPopError::BadSignature { .. })
        ));
        for expected in [
            FaucetSinkPossession {
                genesis_hash: GENESIS.to_ascii_uppercase(),
                ..claim()
            },
            FaucetSinkPossession {
                address: claim().address.to_ascii_uppercase().replace("0X", "0x"),
                ..claim()
            },
            FaucetSinkPossession {
                address: "0x1234".into(),
                ..claim()
            },
        ] {
            assert!(matches!(
                verify_faucet_sink_possession(&proof, &expected),
                Err(FaucetSinkPopError::MalformedClaim { .. })
            ));
        }
    }
}
