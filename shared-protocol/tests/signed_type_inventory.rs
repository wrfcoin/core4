//! Source-derived signed-type inventory (#2122, addressing review on PR #2231).
//!
//! # Why this file exists
//!
//! The first version of #2122 built its inventory by hand and then tested that
//! the hand-written rows were internally consistent. Review found the flaw
//! precisely: the suite passed 56/56 while omitting five live signed types and
//! binding `FinalityVote` to an unsigned struct. **A registry that cannot detect
//! its own omissions is self-consistent and still wrong**, and it rots silently
//! as new signed types are added.
//!
//! So this test does not read the registry to decide what to check. It reads
//! **the source tree**, finds every type carrying a signature field, and
//! requires each one to be in exactly one of two places:
//!
//! 1. [`domain_registry::SIGNED_OBJECT_REGISTRY`] — a protocol-level signed
//!    object with a domain tag, or
//! 2. [`UNREGISTERED`] below — an explicit, reasoned exclusion.
//!
//! A signed type in neither **fails this test**. That is the property the
//! original submission lacked: adding a new signed type without a tag now
//! breaks the build rather than quietly widening the gap.
//!
//! # Why a source scan rather than a macro registry
//!
//! A `linkme`/`inventory`-style distributed slice would only collect types that
//! *opted in* — which is the same failure mode as counting call sites, the
//! thing #2065 already proved does not work. Only something that reads the
//! source can see a type that did nothing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wrfcoin_shared_protocol::crypto::domain_registry::SIGNED_OBJECT_REGISTRY;

mod common;
#[path = "common/slash_evidence_vectors.rs"]
mod slash_evidence_vectors;

#[test]
fn slash_evidence_report_has_executed_golden_bytes() {
    slash_evidence_vectors::assert_pinned_vector();
}

/// A type found by the scan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SignedType {
    name: String,
    /// Workspace-relative path.
    file: String,
    line: usize,
}

/// Crates excluded from the scan entirely, with the reason.
///
/// These are not part of the protocol's signed surface.
const SKIPPED_CRATES: &[(&str, &str)] = &[
    (
        "test_server",
        "a standalone test fixture server, not shipped",
    ),
    ("security-demo", "a demo binary, not shipped"),
    (
        "src-tauri",
        "desktop shell, excluded from the cargo workspace",
    ),
    ("fuzz", "fuzz harness crate"),
    ("target", "build output"),
    ("sdks", "generated SDK bindings, not protocol definitions"),
    (
        "tests",
        "workspace test fixtures and mocks, not protocol definitions",
    ),
    (
        "tools",
        "CLI and developer tooling, not protocol definitions",
    ),
    ("dev-mocks", "mock services"),
    ("dev-tools", "developer tooling"),
];

/// Canonical production files the source-derived audit must actually traverse.
///
/// A broad hit-count alone is not a completeness proof: the scanner could
/// inspect one large crate, silently skip another, and still clear the count.
/// These sentinels span the shared payload definitions and every cross-crate
/// `DomainSeparated` implementation that cannot live in shared-protocol.
const REQUIRED_PRODUCTION_SOURCES: &[&str] = &[
    "shared-protocol/src/accounts/native_smart_accounts.rs",
    "shared-protocol/src/blockchain/transaction.rs",
    "shared-protocol/src/crypto/domain_registry.rs",
    "consensus-engines/src/selection/bft_finality.rs",
    "forecast-score/src/blockchain.rs",
    "keeper-proof/src/lib.rs",
    "oracle-packet/src/lib.rs",
    "sensor-attestation/src/lib.rs",
];

/// Fail if traversal unexpectedly collapses to a narrow subset of the
/// workspace. The current corpus is over 460 Rust files; 400 leaves room for
/// legitimate consolidation while making a skipped production subtree loud.
const MIN_PRODUCTION_RUST_SOURCE_FILES: usize = 400;

/// Signed types deliberately **not** in the domain-tag registry, each with a
/// reason. Reviewed as part of #2122.
///
/// Three categories, and the distinction matters:
///
/// - **Signature containers** — a struct whose whole purpose is to carry a
///   signature alongside its subject (`TransactionInput`, `ValidatorSignature`).
///   The *subject* is the signed object and is registered; the container is not
///   separately signed.
/// - **Transport/request wrappers** — API DTOs that relay a signature produced
///   over an inner payload. The inner payload is the registered signed object.
/// - **Consensus-engine internal messages** — HotStuff/PBFT/gossip traffic.
///   Genuinely signed, but a separate inventory at a different layer; tracked by
///   the issue named in the reason rather than silently dropped.
const UNREGISTERED: &[(&str, &str)] = &[
    (
        "NodeBondLifecycle",
        "transport signature container (Node S2): owner, governance and payout keys sign \
         the registered NodeBondAuthorization; dedicated online keys sign the registered \
         role-bound NodeBondPossession. The container is never a signing preimage",
    ),
    (
        "NodeReceiptSubmission",
        "signature container: owner signs the registered NodeReceiptSubmissionAuthorization; batch contains existing signed ticket, receipt, manifest and endorsement domains",
    ),
    (
        "BondedServiceClaim",
        "signature container: server signs NodeServiceTicket; consumer signs NodeSettlementEndorsement; inner receipt and snapshot preserve legacy domains",
    ),
    (
        "SignedNodeBondAuthorization",
        "signature container: owner signs NodeBondAuthorization; dedicated online keys sign role-bound NodeBondPossession",
    ),
    (
        "NativeSignedAccount",
        "signature container (#2167): actor signs NativeAccountSubject, sponsor signs \
         NativeSponsorSubject, granted session key signs NativeSessionPossession; \
         no container is separately signed",
    ),
    (
        "SlashEvidenceReportPayload",
        "signature container (Refs #3398); reporter signs the checked immutable \
         SlashEvidenceReport body, whose DomainSeparated tag is registered",
    ),
    // --- signature containers -------------------------------------------
    (
        "PaymasterData",
        "signature container (#2168); the sponsor signs the UserOperation hash, \
         so the signed object is the operation, not this struct",
    ),
    (
        "TransactionInput",
        "signature container; the Transaction is the signed object",
    ),
    (
        "SignedUpgradeNotice",
        "signature container (#3094): carries one GovernanceSignature per governance \
         signer over the DomainSeparated UpgradeNotice, which is the signed object and \
         carries the domain tag; the Vec is there so #2156 threshold signing is additive",
    ),
    (
        "SignedFeatureScheduleExtension",
        "signature container (#3277): carries one GovernanceSignature per governance \
         signer over the DomainSeparated FeatureScheduleExtension, which is the signed \
         object and carries the domain tag; the Vec is there so #2156 threshold signing \
         is additive",
    ),
    (
        "FaucetSinkPossessionProof",
        "signature container (#3361 F3): carries the faucet key's public key + signature \
         over the DomainSeparated FaucetSinkPossession, which is the signed object and \
         carries the domain tag; off-chain, verified only by the extension signer",
    ),
    (
        "FaucetSinkPopError",
        "error enum whose BadSignature variant merely names a signature in prose (#3361 F3, \
         the DelegationError::BadSignature class): its `reason: String` field is not \
         signature material, and the type signs nothing — FaucetSinkPossession is the \
         signed object and carries the domain tag",
    ),
    (
        "GovernanceSignature",
        "one governance signer's public key + signature hex over an UpgradeNotice \
         preimage (#3094); it signs nothing itself — UpgradeNotice is the signed object",
    ),
    (
        "BondedFetchAuth",
        "off-chain request authorization for the P2P bonded snapshot fetch protocol \
         (p2p/src/bonded_fetch.rs, owner decision 2026-10-31): a registered bond consumer \
         key signs (server peer id, client peer id, time, nonce) framed by \
         domain_preimage_bound under its own tag wrfcoin.p2p_bonded_fetch_auth.v1 at V2. \
         Verified only by the serving node before answering a fetch; never a chain object, \
         never gossiped, so it carries no registry DOMAIN_TAG by design",
    ),
    (
        "ProducerPossessionProof",
        "ceremony-only proof of possession (#3096): a producer key signs the \
         producer_key_pop challenge domain over the ceremony id, the bundle's registration \
         commitment and a timestamp; verified by the coordinator, never a chain object and \
         never gossiped, so it carries no DOMAIN_TAG by design",
    ),
    (
        "WebAuthnAssertion",
        "WebAuthn P-256 assertion container (#2169): `signature` is an ECDSA-P256 \
         signature over authenticatorData || SHA256(clientDataJSON), whose \
         challenge commits to `wrfcoin_passkey_transfer_auth_v1`. Not an ed25519 \
         preimage, so it has no money_path_v2 golden case; its bytes are pinned by \
         tests/passkey_webauthn_2169.rs (Wycheproof P-256 vectors + a deterministic \
         software authenticator) and the tag is asserted distinct from the ed25519 \
         transfer tag there",
    ),
    (
        "PasskeyAuthorization",
        "hex wire form of WebAuthnAssertion carried in \
         TransparentTransferPayload.passkey_auth (#2169); same coverage as \
         WebAuthnAssertion",
    ),
    (
        "AttestationSubmission",
        "wire-shape renderer for OracleAttestationPayload (#2474); it copies the \
         already-signed `signature` field and renames commit_id -> commit_hash for \
         the route, and signs nothing itself — OracleAttestationPayload is the \
         signed object and carries the domain tag",
    ),
    (
        "ValidatorSignature",
        "signature container inside FinalityCheckpoint",
    ),
    (
        "IndividualSignature",
        "signature container inside a QuorumCertificate",
    ),
    (
        "AggregatedSignature",
        "signature container inside a QuorumCertificate",
    ),
    (
        "ProposalSignature",
        "signature container inside GovernanceProposal",
    ),
    (
        "ProducerSetChangeEnvelope",
        "signature container; the ProducerSetChange is the signed object (tag wrfcoin.producer_set_change.v1, #2290)",
    ),
    (
        "TimeoutSignature",
        "signature container inside a TimeoutCertificate",
    ),
    (
        "AuthorizedSignature",
        "signature container in the security manager",
    ),
    (
        "ChannelParticipant",
        "holds a participant's channel signature, not itself signed",
    ),
    // --- transport / request wrappers ------------------------------------
    (
        "OracleAttestationRequest",
        "API DTO relaying a signed OracleAttestationPayload",
    ),
    (
        "LegacyTransactionSubmitRequest",
        "API DTO relaying a signed Transaction",
    ),
    (
        "PermissionlessSubmitRequest",
        "API DTO relaying a signed Transaction",
    ),
    (
        "ContributionCommitRequest",
        "API DTO relaying a signed CommitPayload",
    ),
    (
        "ForecastCommitRequest",
        "API DTO relaying a signed CommitPayload, the forecast twin of \
         ContributionCommitRequest; it carries auth_pubkey/auth_signature straight into \
         the CommitPayload the route builds and signs nothing itself - CommitPayload is \
         the signed object and carries the canonical commit-authorization domain tag, \
         which the route re-checks via verify_authorization() before submission",
    ),
    (
        "WrfcoinMessage",
        "p2p transport envelope; the inner payload is the signed object",
    ),
    // --- consensus-engine internal messages (core4#2249) ------------------
    (
        "HotStuffProposal",
        "consensus-engine internal message — core4#2249",
    ),
    (
        "HotStuffVote",
        "consensus-engine internal message — core4#2249",
    ),
    (
        "NewViewMessage",
        "consensus-engine internal message — core4#2249",
    ),
    (
        "PbftMessage",
        "consensus-engine internal message — core4#2249",
    ),
    (
        "GovernanceProposal",
        "consensus-engine internal message — core4#2249",
    ),
    (
        "EnhancedVote",
        "consensus-engine internal message — core4#2249",
    ),
    ("Message", "consensus-engine gossip message — core4#2249"),
    (
        "BuilderBid",
        "proposer/builder separation message — core4#2249",
    ),
    (
        "BlindedBlock",
        "proposer/builder separation message — core4#2249",
    ),
    (
        "QuorumCertificate",
        "aggregate of validator signatures — core4#2249",
    ),
    (
        "TimeoutCertificate",
        "aggregate of timeout signatures — core4#2249",
    ),
    (
        "FinalityCheckpoint",
        "aggregate of validator signatures — core4#2249",
    ),
    (
        "HotStuffConsensus",
        "engine state holding in-flight signatures — core4#2249",
    ),
    (
        "AuthorizationLevel",
        "consensus-engine security-manager authorization — core4#2249",
    ),
    (
        "TransitionTrigger",
        "carries an emergency override_signature — core4#2249",
    ),
    (
        "ConsensusThreatType",
        "threat report listing invalid_signatures for diagnostics; the listed \
         signatures are evidence, not a signed object",
    ),
    // --- other -----------------------------------------------------------
    (
        "UserOperation",
        "ERC-4337 style account-abstraction op — core4#2249",
    ),
    (
        "ComputationalNFT",
        "NFT record carrying a mint signature — core4#2249",
    ),
    ("StateProof", "layer2 proof aggregate — core4#2249"),
    (
        "ProvenanceLink",
        "dataset manifest provenance link — core4#2249",
    ),
    (
        "MlsagVerifyItem",
        "ring-signature verification work item, not a signed object",
    ),
    // GamificationClaimPayload and OracleRegistrationPayload are NOT excluded —
    // they are accounted as first-class authorization-message signed surfaces
    // in AUTHORIZATION_MESSAGE_SURFACES below, each tied to its OWN domain tag
    // with byte-pinned golden coverage. The exclusions in this list are legacy
    // debt (signed sub-payloads predating the golden fixture); Dan's #2370
    // review is explicit that they are precedent, NOT evidence of coverage.
    (
        "SignedNodeServiceReceipt",
        "signature envelope over the registered NodeServiceReceipt domain-separated body",
    ),
    (
        "SignedRetrievalAuditReceipt",
        "signature envelope over the registered RetrievalAuditReceipt domain-separated body (keeper-proof, feature retrieval-audit)",
    ),
    (
        "SignedSnapshotSyncPayload",
        "wrapper over the registered SnapshotSyncPayload",
    ),
];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/shared-protocol.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("shared-protocol must have a parent directory")
        .to_path_buf()
}

fn is_skipped(rel: &str) -> bool {
    SKIPPED_CRATES
        .iter()
        .any(|(crate_name, _)| rel.starts_with(&format!("{crate_name}/")))
}

/// Enumerate the production Rust corpus once, failing closed on traversal
/// errors and proving that the scan reached the required cross-crate surfaces.
///
/// Returning an empty/partial set is never a successful audit result. Earlier
/// versions used `let Ok(..) = .. else { continue; }` and `entries.flatten()`,
/// which converted unreadable directories and entries into silent exclusions.
fn production_rust_sources() -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    let manifest_path = root.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        panic!(
            "source inventory cannot read workspace manifest {}: {error}",
            manifest_path.display()
        )
    });
    assert!(
        manifest.contains("[workspace]"),
        "source inventory root {} is not the Cargo workspace",
        root.display()
    );

    let mut sources = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|error| {
            panic!(
                "source inventory cannot read directory {}: {error}",
                dir.display()
            )
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| {
                panic!(
                    "source inventory cannot read an entry under {}: {error}",
                    dir.display()
                )
            });
            let path = entry.path();
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with('.') || is_skipped(&rel) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push((path, rel));
            }
        }
    }

    sources.sort_by(|left, right| left.1.cmp(&right.1));
    sources.dedup_by(|left, right| left.1 == right.1);
    assert!(
        sources.len() >= MIN_PRODUCTION_RUST_SOURCE_FILES,
        "source inventory reached only {} Rust files; expected at least {}. A production \
         subtree was probably skipped or the workspace root is wrong",
        sources.len(),
        MIN_PRODUCTION_RUST_SOURCE_FILES
    );

    let scanned: BTreeSet<&str> = sources.iter().map(|(_, rel)| rel.as_str()).collect();
    for required in REQUIRED_PRODUCTION_SOURCES {
        assert!(
            scanned.contains(required),
            "source inventory did not traverse mandatory production file {required}"
        );
    }
    sources
}

/// True when `line` declares a field whose *name* mentions a signature and
/// whose *type* actually holds signature bytes.
///
/// The type check is what keeps configuration knobs out: `signature_threshold:
/// usize` and `require_signatures: bool` name a signature but hold a policy
/// value, and an early version of this scan reported six of them.
fn is_signature_field(line: &str) -> bool {
    let trimmed = line.trim().trim_start_matches("pub ").trim();
    let Some((name, ty)) = trimmed.split_once(':') else {
        return false;
    };
    let name = name.trim().to_ascii_lowercase();
    let ty = ty.trim().trim_end_matches(',');
    let ty = ty.strip_prefix("Option<").unwrap_or(ty);
    // A type whose name mentions Signature usually holds signature bytes
    // (`Signature`, `SignatureBytes`, `AggregatedSignature`, `Vec<TimeoutSignature>`).
    // The exceptions are configuration types that merely *name* an algorithm —
    // a deny-list, because it is a short closed set and an over-clever prefix
    // rule dropped six real signature types the first time.
    const NOT_SIGNATURE_BYTES: &[&str] =
        &["SignatureScheme", "SignatureAlgorithm", "SignaturePolicy"];
    let mentions_signature_type =
        ty.contains("Signature") && !NOT_SIGNATURE_BYTES.iter().any(|deny| ty.contains(deny));
    if !(name.contains("signature")
        || name == "verifier_attestation"
        || name == "sig"
        || name == "sigs"
        || (name == "proof" && mentions_signature_type))
    {
        return false;
    }
    ty.starts_with("String")
        || ty.starts_with("Vec<u8>")
        || ty.starts_with("Vec<String>")
        || mentions_signature_type
        || (ty.starts_with("[u8;") && ty.contains(']'))
}

/// Scan one file for types carrying a signature field.
///
/// Brace-depth scoped: a signature field only counts for the item it is
/// syntactically inside. Without that, a struct declaration followed much later
/// by an unrelated signature field produced phantom hits.
fn scan_file(path: &Path, rel: &str) -> Vec<SignedType> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("source inventory cannot read {rel}: {error}"));
    let mut out = Vec::new();
    let mut current: Option<(String, usize)> = None;
    let mut depth: i32 = 0;
    let mut recorded = false;

    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let is_decl = trimmed.starts_with("pub struct ") || trimmed.starts_with("pub enum ");

        if is_decl && depth == 0 {
            let rest = trimmed
                .trim_start_matches("pub struct ")
                .trim_start_matches("pub enum ");
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                current = Some((name, idx + 1));
                recorded = false;
            }
        }

        if current.is_some() && !recorded && is_signature_field(line) {
            let (name, line_no) = current.clone().unwrap();
            out.push(SignedType {
                name,
                file: rel.to_string(),
                line: line_no,
            });
            recorded = true;
        }

        // Count braces on the CODE portion only. Doc comments routinely
        // contain braces (`/// e.g. { "a": 1 }`), and counting those skewed the
        // depth so a declaration leaked forward and captured an unrelated
        // struct's signature field — that is how `HotStuffConfig` and
        // `ConsensusThreatType` first showed up as phantom signed types.
        let code = line.split("//").next().unwrap_or("");
        depth += code.matches('{').count() as i32;
        depth -= code.matches('}').count() as i32;
        if depth <= 0 {
            depth = 0;
            if current.is_some() && !is_decl {
                current = None;
            }
        }
    }
    out
}

fn discover_signed_types() -> Vec<SignedType> {
    let mut found: Vec<SignedType> = Vec::new();
    for (path, rel) in production_rust_sources() {
        found.extend(scan_file(&path, &rel));
    }
    found.sort();
    found.dedup();
    found
}

fn registered_type_names() -> BTreeSet<String> {
    SIGNED_OBJECT_REGISTRY
        .iter()
        .filter_map(|e| e.rust_type.rsplit("::").next())
        .filter(|n| *n != "(no type)")
        .map(str::to_string)
        .collect()
}

fn excluded_names() -> BTreeSet<String> {
    UNREGISTERED.iter().map(|(n, _)| n.to_string()).collect()
}

/// First-class authorization-message signed surfaces (#2370, Dan's review):
/// payloads that sign their OWN chain-bound domain-tagged authorization
/// message — verified on live transition / block-verification paths — and are
/// therefore NOT covered by the outer registered Transaction preimage. Each is
/// tied here to its actual domain tag and its byte-pinned golden case, so the
/// audit accounts for it with real coverage rather than a "covered by
/// Transaction" exclusion that would be false.
struct AuthSurface {
    /// The struct the scanner finds (has a signature field).
    rust_type: &'static str,
    /// The exact domain tag the payload's authorization message is signed under.
    domain_tag: &'static str,
    /// The `kind` of its `money_path_v2` case in the golden fixture.
    golden_kind: &'static str,
}

const AUTHORIZATION_MESSAGE_SURFACES: &[AuthSurface] = &[
    AuthSurface {
        rust_type: "CommitPayload",
        domain_tag: "wrfcoin_commit_auth_v2",
        golden_kind: "commit_authorization_v2",
    },
    AuthSurface {
        rust_type: "TransparentTransferPayload",
        domain_tag: "wrfcoin_transparent_transfer_auth_v2",
        golden_kind: "transparent_transfer_authorization_v2",
    },
    AuthSurface {
        rust_type: "TransparentTransferPayload",
        domain_tag: "wrfcoin_system_transfer_auth_v2",
        golden_kind: "system_transfer_authorization_v2",
    },
    AuthSurface {
        rust_type: "StakeAuthPayload",
        domain_tag: "wrfcoin_stake_auth_v2",
        golden_kind: "stake_authorization_v2",
    },
    AuthSurface {
        rust_type: "GamificationClaimPayload",
        domain_tag: "wrfcoin_gamification_auth_v1",
        golden_kind: "gamification_authorization_v1",
    },
    AuthSurface {
        rust_type: "OracleRegistrationPayload",
        domain_tag: "wrfcoin_oracle_registration_auth_v1",
        golden_kind: "oracle_registration_authorization_v1",
    },
    AuthSurface {
        rust_type: "StationRegistrationPayload",
        domain_tag: "wrfcoin_station_registration_auth_v1",
        golden_kind: "station_registration_authorization_v1",
    },
    AuthSurface {
        rust_type: "StationRegistrationPayload",
        domain_tag: "wrfcoin_station_registration_auth_v2",
        golden_kind: "station_registration_authorization_v2",
    },
    AuthSurface {
        rust_type: "RewardClaimPayload",
        domain_tag: "wrfcoin_reward_claim_auth_v2",
        golden_kind: "reward_claim_authorization_v2",
    },
    AuthSurface {
        rust_type: "RotationPlan",
        domain_tag: "wrfcoin.validator_key_rotation_pop.v2",
        golden_kind: "validator_key_rotation_pop_v2",
    },
    AuthSurface {
        rust_type: "SelectiveDisclosureProofPayload",
        domain_tag: "wrfcoin_selective_disclosure_auth_v2",
        golden_kind: "selective_disclosure_authorization_v2",
    },
    AuthSurface {
        rust_type: "ConfidentialTransferPayload",
        domain_tag: "wrfcoin_confidential_transfer_auth_v2",
        golden_kind: "confidential_transfer_authorization_v2",
    },
    AuthSurface {
        rust_type: "GuardianApproval",
        domain_tag: "wrfcoin.account_recovery_guardian_approval.v1",
        golden_kind: "account_recovery_guardian_approval_v2",
    },
    AuthSurface {
        rust_type: "RecoveryAction",
        domain_tag: "wrfcoin.account_recovery_owner_set_policy.v1",
        golden_kind: "account_recovery_owner_set_policy_v2",
    },
    AuthSurface {
        rust_type: "RecoveryAction",
        domain_tag: "wrfcoin.account_recovery_owner_cancel.v1",
        golden_kind: "account_recovery_owner_cancel_v2",
    },
    AuthSurface {
        rust_type: "RecoveryAction",
        domain_tag: "wrfcoin.account_recovery_owner_cancel_policy_change.v1",
        golden_kind: "account_recovery_owner_cancel_policy_change_v2",
    },
    AuthSurface {
        rust_type: "SessionKeyAction",
        domain_tag: "wrfcoin.session_key_register.v1",
        golden_kind: "session_key_register_v2",
    },
    AuthSurface {
        rust_type: "SessionKeyAction",
        domain_tag: "wrfcoin.session_key_revoke.v1",
        golden_kind: "session_key_revoke_v2",
    },
    AuthSurface {
        rust_type: "SessionKeyAction",
        domain_tag: "wrfcoin.session_key_use.v1",
        golden_kind: "session_key_use_v2",
    },
    AuthSurface {
        rust_type: "DeclareAuthorization",
        domain_tag: "wrfcoin.declare_authorization.v1",
        golden_kind: "declare_authorization_v2",
    },
    // #3345 / #3347 owner ledger: extracted by the ONE shared constructor
    // `Transaction::station_owner_ledger_transition` (unscheduled features).
    AuthSurface {
        rust_type: "StationOwnerTransferPayload",
        domain_tag: "wrfcoin_station_owner_transfer_v1",
        golden_kind: "station_owner_transfer_v1",
    },
    AuthSurface {
        rust_type: "StationOwnerTransferPayload",
        domain_tag: "wrfcoin_station_owner_transfer_accept_v1",
        golden_kind: "station_owner_transfer_accept_v1",
    },
    AuthSurface {
        rust_type: "StationOwnerRecoveryFilingPayload",
        domain_tag: "wrfcoin_station_owner_recovery_file_v1",
        golden_kind: "station_owner_recovery_file_v1",
    },
    AuthSurface {
        rust_type: "StationOwnerRecoveryFilingPayload",
        domain_tag: "wrfcoin_station_owner_recovery_accept_v1",
        golden_kind: "station_owner_recovery_accept_v1",
    },
    AuthSurface {
        rust_type: "StationOwnerRecoveryCancelPayload",
        domain_tag: "wrfcoin_station_owner_recovery_cancel_v1",
        golden_kind: "station_owner_recovery_cancel_v1",
    },
    AuthSurface {
        rust_type: "StationOwnerActionPayload",
        domain_tag: "wrfcoin_station_owner_action_v1",
        golden_kind: "station_owner_action_v1",
    },
    // Sponsored first sends: carried in TransparentTransferPayload.sponsorship,
    // read by state apply only once `paymaster` is active.
    AuthSurface {
        rust_type: "SponsorAuthorization",
        domain_tag: "wrfcoin_sponsored_send_auth_v1",
        golden_kind: "sponsored_send_authorization_v1",
    },
];

fn auth_surface_names() -> BTreeSet<String> {
    AUTHORIZATION_MESSAGE_SURFACES
        .iter()
        .map(|s| s.rust_type.to_string())
        .collect()
}

// ---------------------------------------------------------------------------

#[test]
fn source_scan_corpus_is_nonempty_and_spans_required_production_crates() {
    // All assertions live inside the enumerator so every consumer gets the
    // same fail-closed behavior. This named test makes the completeness
    // contract independently executable and RED-controllable.
    let _ = production_rust_sources();
}

#[test]
fn the_scan_finds_types_at_all() {
    // Guards the guard. If the scanner silently found nothing — a bad path, a
    // broken regex — every assertion below would pass vacuously, which is the
    // exact failure mode this whole file exists to correct.
    let found = discover_signed_types();
    assert!(
        found.len() > 20,
        "the source scan found only {} signed types; it is probably broken \
         (wrong root, or the field matcher stopped matching)",
        found.len()
    );
}

#[test]
fn the_scan_finds_the_types_review_identified_as_missing() {
    // Regression for the specific omissions found in review of PR #2231. If the
    // scanner ever stops seeing these, it has regressed to the state that let
    // them through.
    let found = discover_signed_types();
    let names: BTreeSet<&str> = found.iter().map(|t| t.name.as_str()).collect();
    for required in [
        "ComputeReceipt",
        "ForecastVerificationReceipt",
        "DeviceAttestation",
        "StorageAttestation",
        "OraclePacket",
        "FinalityVote",
    ] {
        assert!(
            names.contains(required),
            "the scanner no longer finds {required}, which review identified as \
             an omission — the scan has regressed"
        );
    }
}

#[test]
fn every_signed_type_is_registered_or_explicitly_excluded() {
    // THE test. A signed type that is neither tagged nor excluded fails here.
    let found = discover_signed_types();
    let registered = registered_type_names();
    let excluded = excluded_names();
    let auth_surfaces = auth_surface_names();

    let mut unaccounted: Vec<&SignedType> = found
        .iter()
        .filter(|t| {
            !registered.contains(&t.name)
                && !excluded.contains(&t.name)
                && !auth_surfaces.contains(&t.name)
        })
        .collect();
    unaccounted.sort();
    unaccounted.dedup_by(|a, b| a.name == b.name);

    assert!(
        unaccounted.is_empty(),
        "{} signed type(s) have no domain tag and no recorded exclusion:\n{}\n\n\
         Every type carrying a signature must either get a DOMAIN_TAG in \
         SIGNED_OBJECT_REGISTRY (and a row in docs/security/DOMAIN_TAGS.md), or \
         an entry in UNREGISTERED with a reason. Do not delete this assertion.",
        unaccounted.len(),
        unaccounted
            .iter()
            .map(|t| format!("  {} ({}:{})", t.name, t.file, t.line))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A `DomainSeparated` impl found in the source: the type and the tag it claims.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DomainImpl {
    rust_type: String,
    tag: String,
    file: String,
    line: usize,
}

/// Scan the workspace for every `impl DomainSeparated for T` and the
/// `DOMAIN_TAG` it declares.
///
/// # Why the registry check was not enough
///
/// `domain_registry::tests::registry_is_injective_on_rust_type` inspects
/// registry **rows**. It cannot see an `impl` that claims a tag without a
/// matching row — which is exactly how `FinalitySignal` kept
/// `wrfcoin.finality_vote.v1` after the registry rebound that tag to the real
/// `FinalityVote`, leaving two distinct types answering to one tag.
///
/// Two types sharing a tag is the cross-protocol replay hole #2122 exists to
/// close: a signature over one verifies as the other. This is the same
/// source-derived technique that catches omissions, applied from the opposite
/// direction — the registry should catch both.
fn discover_domain_impls() -> Vec<DomainImpl> {
    let mut found = Vec::new();
    for (path, rel) in production_rust_sources() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("domain-impl inventory cannot read {rel}: {error}"));
        let lines: Vec<&str> = text.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("impl DomainSeparated for ") else {
                continue;
            };
            let rust_type: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if rust_type.is_empty() {
                continue;
            }
            // The tag is declared inside the impl body, normally the next
            // few lines. Bound the search so a missing const cannot silently
            // absorb an unrelated one further down the file.
            for probe in lines.iter().skip(idx + 1).take(8) {
                if let Some(pos) = probe.find("DOMAIN_TAG") {
                    if let Some(open) = probe[pos..].find("b\"") {
                        let after = &probe[pos + open + 2..];
                        if let Some(close) = after.find('"') {
                            found.push(DomainImpl {
                                rust_type: rust_type.clone(),
                                tag: after[..close].to_string(),
                                file: rel.clone(),
                                line: idx + 1,
                            });
                        }
                    }
                    break;
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_impl_scan_finds_impls_at_all() {
    // Guards the guard: a broken scan would make both assertions below vacuous.
    let impls = discover_domain_impls();
    assert!(
        impls.len() >= 10,
        "the DomainSeparated impl scan found only {} impls; it is probably broken",
        impls.len()
    );
}

#[test]
fn impl_tags_are_injective_across_the_source() {
    // THE property. Two distinct types claiming one tag means a signature over
    // one verifies as the other — the exact hole domain separation closes.
    //
    // Test-only fixture types are excluded: they live in `#[cfg(test)]` modules
    // and deliberately use throwaway tags.
    let impls: Vec<DomainImpl> = discover_domain_impls()
        .into_iter()
        .filter(|i| !i.tag.contains("test_"))
        .collect();

    let mut by_tag: std::collections::BTreeMap<String, Vec<&DomainImpl>> =
        std::collections::BTreeMap::new();
    for i in &impls {
        by_tag.entry(i.tag.clone()).or_default().push(i);
    }

    let collisions: Vec<String> = by_tag
        .iter()
        .filter(|(_, claimants)| claimants.len() > 1)
        .map(|(tag, claimants)| {
            format!(
                "  {tag} claimed by {}",
                claimants
                    .iter()
                    .map(|c| format!("{} ({}:{})", c.rust_type, c.file, c.line))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            )
        })
        .collect();

    assert!(
        collisions.is_empty(),
        "{} domain tag(s) are claimed by more than one type — a signature over \
         one would verify as the other:\n{}",
        collisions.len(),
        collisions.join("\n")
    );
}

#[test]
fn every_domain_impl_agrees_with_its_registry_row() {
    // An impl whose tag drifts from the registry, or which has no row at all, is
    // how `FinalitySignal` kept a tag the registry had reassigned.
    let impls: Vec<DomainImpl> = discover_domain_impls()
        .into_iter()
        .filter(|i| !i.tag.contains("test_"))
        .collect();

    let mut problems = Vec::new();
    for i in &impls {
        let row = SIGNED_OBJECT_REGISTRY
            .iter()
            .find(|e| e.rust_type.rsplit("::").next() == Some(i.rust_type.as_str()));
        match row {
            None => problems.push(format!(
                "  {} ({}:{}) implements tag {} but has no registry row",
                i.rust_type, i.file, i.line, i.tag
            )),
            Some(e) => {
                let registered = String::from_utf8_lossy(e.tag).to_string();
                if registered != i.tag {
                    problems.push(format!(
                        "  {} ({}:{}) implements {} but the registry binds it to {}",
                        i.rust_type, i.file, i.line, i.tag, registered
                    ));
                }
            },
        }
    }

    assert!(
        problems.is_empty(),
        "{} DomainSeparated impl(s) disagree with the registry:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// #2249 reverse guard: `every_domain_impl_agrees_with_its_registry_row` only
/// walks impl → registry, so it is silent if a `CrossCrate` impl is *deleted*
/// from its owning crate (there is then no impl to check). This walks
/// registry → impl: every `CrossCrate` row must have a discovered
/// `impl DomainSeparated for <type>` somewhere in the source, with the reserved
/// tag. Deleting any of the four (`FinalityVote`, `DeviceAttestation`,
/// `StorageAttestation`, `OraclePacket`) fails the build.
#[test]
fn every_cross_crate_row_has_a_matching_impl_in_the_source_2249() {
    use wrfcoin_shared_protocol::crypto::domain_registry::DomainCoverage;

    let impls = discover_domain_impls();
    let mut problems = Vec::new();
    for entry in SIGNED_OBJECT_REGISTRY {
        if !matches!(entry.coverage, DomainCoverage::CrossCrate { .. }) {
            continue;
        }
        let expected_name = entry
            .rust_type
            .rsplit("::")
            .next()
            .unwrap_or(entry.rust_type);
        let expected_tag = String::from_utf8_lossy(entry.tag).to_string();
        match impls.iter().find(|i| i.rust_type == expected_name) {
            None => problems.push(format!(
                "  CrossCrate row {expected_name} ({expected_tag}) has NO \
                 `impl DomainSeparated for {expected_name}` anywhere in the source (#2249)"
            )),
            Some(i) if i.tag != expected_tag => problems.push(format!(
                "  {expected_name} impls tag {} but the registry reserves {expected_tag}",
                i.tag
            )),
            Some(_) => {},
        }
    }
    assert!(
        problems.is_empty(),
        "CrossCrate registry->impl gaps (#2249):\n{}",
        problems.join("\n")
    );
}

#[test]
fn exclusions_all_carry_a_reason() {
    for (name, reason) in UNREGISTERED {
        assert!(
            reason.len() > 15,
            "exclusion for {name} has no meaningful reason: {reason:?}"
        );
    }
}

#[test]
fn exclusions_are_not_stale() {
    // An exclusion naming a type that no longer exists is dead weight that
    // hides a future type of the same name.
    let found = discover_signed_types();
    let names: BTreeSet<&str> = found.iter().map(|t| t.name.as_str()).collect();
    let stale: Vec<&str> = UNREGISTERED
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !names.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "these exclusions name types the scan no longer finds — remove them: {stale:?}"
    );
}

#[test]
fn no_type_is_both_registered_and_excluded() {
    let registered = registered_type_names();
    let overlap: Vec<String> = UNREGISTERED
        .iter()
        .map(|(n, _)| n.to_string())
        .filter(|n| registered.contains(n))
        .collect();
    assert!(
        overlap.is_empty(),
        "these types are both registered and excluded — pick one: {overlap:?}"
    );
}

/// Registered objects that carry no signature field of their own, and what
/// actually signs them.
///
/// This table exists because of the defect review found: the `FinalityVote` row
/// was bound to `consensus::finality::FinalitySignal`, a struct with no
/// signature field and — per #2226 — nothing that signs it. Requiring an answer
/// to "if this type has no signature field, what signs it?" is what makes that
/// unanswerable rather than invisible.
const SIGNED_VIA_ENVELOPE: &[(&str, &str)] = &[
    (
        "NativeAccountSubject",
        "actor signature carried by NativeSignedAccount (#2167)",
    ),
    (
        "NativeSponsorSubject",
        "sponsor signature carried by NativeSignedAccount (#2167)",
    ),
    (
        "NativeSessionPossession",
        "session possession signature carried by NativeSignedAccount (#2167)",
    ),
    (
        "SlashEvidenceReport",
        "signed by SlashEvidenceReportPayload.reporter_signature over the checked \
         immutable canonical body; no invalid wire value implements the trait",
    ),
    (
        "Transaction",
        "signed per-input; TransactionInput carries the signature",
    ),
    (
        "Block",
        "signed by the producing validator over the block hash",
    ),
    (
        "NodeServiceReceipt",
        "signed by SignedNodeServiceReceipt.signature over the registered domain-separated body",
    ),
    (
        "RetrievalAuditReceipt",
        "signed by SignedRetrievalAuditReceipt.signature over the registered domain-separated body",
    ),
    (
        "NodeServiceTicket",
        "signed by its detached S1 signature container under the registered V2 domain",
    ),
    (
        "NodeSettlementEndorsement",
        "signed by its detached S1 signature container under the registered V2 domain",
    ),
    (
        "NodeBondAuthorization",
        "signed by its detached S1 signature container under the registered V2 domain",
    ),
    (
        "NodeReceiptSubmissionAuthorization",
        "offline owner signs the explicit registered batch hash and account nonce subject",
    ),
    (
        "NodeBondPossession",
        "signed by its detached S1 signature container under the registered V2 domain",
    ),
    (
        "SnapshotSyncPayload",
        "signed by the SignedSnapshotSyncPayload wrapper",
    ),
    (
        "WeatherReading",
        "signed by the enclosing oracle attestation envelope",
    ),
    (
        "Proposal",
        "signed by the submitting governance transaction",
    ),
    (
        "BridgeMessage",
        "signed by the bridge validator set over the message id",
    ),
    (
        "ValidatorSetChange",
        "signed detached via the DomainSeparated frame; the authorizing governance / \
         validator-set-change transaction carries the signature, not a field on the struct (#2227)",
    ),
    (
        "ProducerSetChange",
        "signed via ProducerSetChangeEnvelope (#2290): the governance authorizer signs the \
         DomainSeparated preimage; the envelope carries the authorizer key + signature",
    ),
    (
        "UpgradeNotice",
        "signed via SignedUpgradeNotice (#3094): each governance signer signs the \
         DomainSeparated preimage; the envelope carries the signer keys + signatures",
    ),
    (
        "FeatureScheduleExtension",
        "signed via SignedFeatureScheduleExtension (#3277): each governance signer signs \
         the DomainSeparated preimage; the envelope carries the signer keys + signatures",
    ),
    (
        "FaucetSinkPossession",
        "signed via FaucetSinkPossessionProof (#3361 F3): the faucet account's key signs \
         the DomainSeparated preimage; the proof carries its public key + signature",
    ),
];

#[test]
fn every_registered_type_has_a_signature_field_or_a_named_envelope() {
    // The FinalityVote/FinalitySignal defect, generalised. A registry row must
    // bind to a type that either carries a signature itself, or has a written
    // answer to "what signs it". `FinalitySignal` could satisfy neither, which
    // is exactly why the wrong binding survived the original suite.
    let found = discover_signed_types();
    let names: BTreeSet<&str> = found.iter().map(|t| t.name.as_str()).collect();
    let envelope: BTreeSet<&str> = SIGNED_VIA_ENVELOPE.iter().map(|(n, _)| *n).collect();

    let unexplained: Vec<&str> = SIGNED_OBJECT_REGISTRY
        .iter()
        .filter(|e| e.rust_type != "(no type)")
        .filter_map(|e| e.rust_type.rsplit("::").next())
        .filter(|n| !names.contains(n) && !envelope.contains(n))
        .collect();

    assert!(
        unexplained.is_empty(),
        "these registry rows bind to types that carry no signature field and \
         have no entry in SIGNED_VIA_ENVELOPE explaining what signs them — \
         this is the FinalityVote/FinalitySignal defect: {unexplained:?}"
    );
}

#[test]
fn envelope_explanations_are_not_stale_or_redundant() {
    // An entry here for a type that DOES carry its own signature is a stale
    // explanation that would mask a future regression.
    let found = discover_signed_types();
    let names: BTreeSet<&str> = found.iter().map(|t| t.name.as_str()).collect();
    let redundant: Vec<&str> = SIGNED_VIA_ENVELOPE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| names.contains(n))
        .collect();
    assert!(
        redundant.is_empty(),
        "these types carry their own signature field, so the envelope \
         explanation is stale — remove it: {redundant:?}"
    );
    for (name, why) in SIGNED_VIA_ENVELOPE {
        assert!(
            why.len() > 15,
            "envelope explanation for {name} is not an explanation"
        );
    }
}

#[test]
fn signature_field_matcher_rejects_configuration_knobs() {
    // Direct unit coverage of the filter that keeps the scan honest.
    assert!(is_signature_field("    pub signature: Option<String>,"));
    assert!(is_signature_field("    pub signature: Vec<u8>,"));
    assert!(is_signature_field("    pub hardware_signature: Vec<u8>,"));
    assert!(is_signature_field("        signature: Vec<u8>,"));
    assert!(is_signature_field("    pub sig: [u8; 64],"));
    assert!(is_signature_field("    pub proof: Option<Signature>,"));

    // Policy values that merely mention signatures.
    assert!(!is_signature_field("    pub signature_threshold: usize,"));
    assert!(!is_signature_field("    pub require_signatures: bool,"));
    assert!(!is_signature_field("    pub min_signatures: u32,"));
    assert!(!is_signature_field("    pub proof: Option<String>,"));
    assert!(!is_signature_field("    pub height: u64,"));
}

/// #2034 cut prep: the inventory question upgrades from "is every signed type
/// registered" to "does every #2226-migration type produce PINNED bytes".
///
/// The golden-vector fixture (`tests/fixtures/golden/signing_preimage_v2.json`,
/// executed byte-for-byte by `tests/signing_preimage_v2_golden.rs`) is the
/// cross-repo signer-drift gate (GENESIS_CUT_PLAN.md §3.4/§7). This test makes
/// the AUDIT binary fail too when a `MigrationDeferred { issue: 2226 }`
/// registry row has no golden case — so adding a type to the migration set
/// without pinning its bytes is caught here even if the golden test file is
/// deleted or ignored wholesale.
#[test]
fn every_2226_migration_row_produces_its_pinned_golden_bytes() {
    use wrfcoin_shared_protocol::crypto::domain_registry::{
        DomainCoverage, MIGRATION_TRACKING_ISSUE,
    };

    let fixture = common::load_fixture();
    assert_eq!(
        fixture["schema"].as_str(),
        Some(common::SCHEMA),
        "fixture schema mismatch — the audit lane must recognize the fixture version"
    );
    let cases = fixture["cases"].as_array().expect("cases");

    for entry in SIGNED_OBJECT_REGISTRY {
        if !matches!(
            entry.coverage,
            DomainCoverage::MigrationDeferred { issue } if issue == MIGRATION_TRACKING_ISSUE
        ) {
            continue;
        }
        let kind = entry.kind.as_str();
        let case = cases
            .iter()
            .find(|c| {
                c["kind"].as_str() == Some(kind) && c["frame"].as_str() == Some("composed_v2")
            })
            .unwrap_or_else(|| {
                panic!(
                    "{kind} is in the #2226 migration set but has no composed-frame golden \
                     case — its preimage bytes are unpinned and cross-repo signers can drift \
                     (#2076 class). Add it to signing_preimage_v2_golden.rs and regenerate."
                )
            });
        // The §7 upgrade, made non-vacuous: recompute canonical_body, V1/V2
        // preimages and the signature from the case's logical input and assert
        // they equal the pinned bytes — then that the tampered twin diverges
        // and fails verification. Corrupting any pinned byte fails THIS audit,
        // not only the separate fixture binary. (Verified: flipping one nibble
        // of a preimage_v2_hex turns this test RED.)
        common::check_case_by_kind(case);
    }
}

/// #2370 (Dan's review): every first-class authorization-message surface has a
/// byte-pinned golden case under ITS OWN domain tag — recomputed from the case
/// input, not merely present. This is what makes them registered-with-coverage
/// rather than excluded-as-covered-by-Transaction (which was false). Byte drift
/// on a gamification/oracle-registration authorization signer fails HERE.
#[test]
fn every_authorization_message_surface_has_byte_pinned_golden_coverage() {
    let fixture = common::load_fixture();
    let cases = fixture["cases"].as_array().expect("cases");
    let discovered = discover_signed_types();
    let scanned: BTreeSet<&str> = discovered.iter().map(|t| t.name.as_str()).collect();

    for surface in AUTHORIZATION_MESSAGE_SURFACES {
        // The type must actually exist in the source (guard against a stale
        // surface entry naming a type that was renamed/removed).
        assert!(
            scanned.contains(surface.rust_type),
            "{} is listed as an authorization-message surface but the scanner no \
             longer finds it — update AUTHORIZATION_MESSAGE_SURFACES",
            surface.rust_type
        );
        let case = cases
            .iter()
            .find(|c| {
                c["kind"].as_str() == Some(surface.golden_kind)
                    && c["frame"].as_str() == Some("money_path_v2")
            })
            .unwrap_or_else(|| {
                panic!(
                    "{} has no golden case '{}' — its authorization signer is \
                     unpinned and can drift (#2076 class)",
                    surface.rust_type, surface.golden_kind
                )
            });
        assert_eq!(
            case["tag"].as_str(),
            Some(surface.domain_tag),
            "{}: golden case tag does not match its declared domain tag",
            surface.rust_type
        );
        // Recompute every pinned byte from the input and check the tampered
        // twin diverges — the non-vacuous half.
        common::check_case_by_kind(case);
    }
}

/// Authorization-message tags must be unique among themselves AND disjoint from
/// every registered DomainSeparated tag — a shared tag is the one-tag-two-
/// meanings defect in another guise.
#[test]
fn authorization_message_tags_are_unique_and_disjoint_from_the_registry() {
    use wrfcoin_shared_protocol::crypto::domain_registry::SIGNED_OBJECT_REGISTRY;

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for surface in AUTHORIZATION_MESSAGE_SURFACES {
        assert!(
            seen.insert(surface.domain_tag),
            "duplicate authorization-message tag {}",
            surface.domain_tag
        );
    }
    let registry_tags: BTreeSet<String> = SIGNED_OBJECT_REGISTRY
        .iter()
        .map(|e| String::from_utf8_lossy(e.tag).into_owned())
        .collect();
    for surface in AUTHORIZATION_MESSAGE_SURFACES {
        assert!(
            !registry_tags.contains(surface.domain_tag),
            "authorization-message tag {} collides with a registered \
             DomainSeparated tag",
            surface.domain_tag
        );
    }
}

#[test]
fn node_service_receipt_has_executed_golden_coverage() {
    let fixture = common::load_fixture();
    let cases: Vec<_> = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "NodeServiceReceipt")
        .collect();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["tag"], "wrfcoin.node_service_receipt.v1");
    common::check_case_by_kind(cases[0]);
}

#[path = "common/node_share_vectors.rs"]
mod node_share_vectors;

#[test]
fn node_share_contracts_have_executed_goldens() {
    node_share_vectors::assert_pinned_vectors();
}

#[path = "common/node_receipt_vectors.rs"]
mod node_receipt_vectors;
#[test]
fn node_receipt_submission_has_executed_golden() { node_receipt_vectors::check_pins(); }
