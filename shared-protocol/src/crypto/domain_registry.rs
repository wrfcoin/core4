//! Signed-object inventory and domain-tag registry (#2122).
//!
//! # Why this module exists
//!
//! #2065 found domain-separation gaps *despite* `crypto::domain_signing`
//! existing. The reason is structural: coverage was previously measured by
//! looking at who called the domain module. A signed type that never called it
//! was invisible to that measurement — the absence of a call site produced no
//! evidence of a gap.
//!
//! This registry inverts the direction. The inventory below is derived **from
//! the type system**: every object the protocol signs is listed here whether or
//! not it calls anything. A type is then in exactly one of three states, and
//! the tests in this module fail if a fourth appears:
//!
//! | State | Meaning |
//! |---|---|
//! | [`DomainCoverage::Implemented`] | Implements `DomainSeparated`; no conflicting legacy signing path. |
//! | [`DomainCoverage::MigrationDeferred`] | Implements `DomainSeparated`, but the *live* signing path still uses a legacy preimage. Switching is consensus-breaking, so it is issue-tracked, not silently applied. |
//! | [`DomainCoverage::Exception`] | The protocol signs this conceptually, but no Rust type exists to implement against. Issue-tracked. |
//!
//! `MigrationDeferred` is the honest state for most of this table today. The
//! trait impls added in #2122 are **additive**: they define the audited
//! preimage without changing any byte any node currently signs or verifies.
//! Cutting the live paths over is a protocol-version bump — see #2123 and
//! `docs/security/PREIMAGE_MIGRATION.md`.
//!
//! # Keeping the doc in sync
//!
//! `docs/security/DOMAIN_TAGS.md` is the human-readable registry.
//! [`tests::registry_doc_matches_code`] parses it out of the binary via
//! `include_str!` and asserts it matches [`SIGNED_OBJECT_REGISTRY`] exactly, so
//! the doc cannot drift from the code without failing the build.

use crate::crypto::domain_signing::DomainSeparated;

/// Every object kind the protocol signs.
///
/// Derived from the type system, not from call sites — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignedObjectKind {
    Transaction,
    BlockHeader,
    FinalityVote,
    ValidatorSetChange,
    /// Authorized block-producer set change (#2290) — a DISTINCT privilege from
    /// `ValidatorSetChange` (finality committee). See `consensus::producer_set`.
    ProducerSetChange,
    SlashingEvidence,
    SnapshotPayload,
    WeatherObservation,
    OracleAttestation,
    AttestationEnvelope,
    GovernanceProposal,
    GovernanceVote,
    BridgeMessage,
    WalletLoginChallenge,
    // --- added after review of PR #2231 found these live signed types omitted ---
    ComputeReceipt,
    ForecastVerificationReceipt,
    DeviceAttestation,
    StorageAttestation,
    /// The real `oracle_packet::OraclePacket`. The pre-existing kind named
    /// `OraclePacket` actually bound `OracleAttestationPayload` and has been
    /// renamed to `OracleAttestation` to match what it binds — review of PR
    /// #2231 caught the mismatch.
    OraclePacket,
    /// #3094: governance-signed upgrade notice (activation height, minimum
    /// protocol version, features, release commit + image digests).
    UpgradeNotice,
    /// #3277: governance-signed feature-schedule extension (future
    /// `(feature, height)` activations appended to consensus state).
    FeatureScheduleExtension,
    /// #3361 F3: the faucet key's proof of possession of a faucet-sink
    /// binding address (off-chain; gates signing the E0f extension).
    FaucetSinkPossession,
    NativeAccountSubject,
    NativeSponsorSubject,
    NativeSessionPossession,
    SlashEvidenceReport,
    /// Consumer-signed receipt that a named server delivered a snapshot page
    /// set or sync range (#3126); the settlement subject for #2848.
    NodeServiceReceipt,
    NodeServiceTicket,
    NodeSettlementEndorsement,
    NodeBondAuthorization,
    NodeBondPossession,
    /// The challenge a passkey payout account's WebAuthn assertion commits to
    /// (`passkey_bond_payout`).
    NodeBondPayoutPasskeyApproval,
    NodeReceiptSubmissionAuthorization,
    RetrievalAuditReceipt,
}

impl SignedObjectKind {
    /// Stable identifier used in `docs/security/DOMAIN_TAGS.md`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transaction => "Transaction",
            Self::BlockHeader => "BlockHeader",
            Self::FinalityVote => "FinalityVote",
            Self::ValidatorSetChange => "ValidatorSetChange",
            Self::ProducerSetChange => "ProducerSetChange",
            Self::SlashingEvidence => "SlashingEvidence",
            Self::SnapshotPayload => "SnapshotPayload",
            Self::WeatherObservation => "WeatherObservation",
            Self::OracleAttestation => "OracleAttestation",
            Self::AttestationEnvelope => "AttestationEnvelope",
            Self::GovernanceProposal => "GovernanceProposal",
            Self::GovernanceVote => "GovernanceVote",
            Self::BridgeMessage => "BridgeMessage",
            Self::WalletLoginChallenge => "WalletLoginChallenge",
            Self::ComputeReceipt => "ComputeReceipt",
            Self::ForecastVerificationReceipt => "ForecastVerificationReceipt",
            Self::DeviceAttestation => "DeviceAttestation",
            Self::StorageAttestation => "StorageAttestation",
            Self::OraclePacket => "OraclePacket",
            Self::UpgradeNotice => "UpgradeNotice",
            Self::FeatureScheduleExtension => "FeatureScheduleExtension",
            Self::FaucetSinkPossession => "FaucetSinkPossession",
            Self::NativeAccountSubject => "NativeAccountSubject",
            Self::NativeSponsorSubject => "NativeSponsorSubject",
            Self::NativeSessionPossession => "NativeSessionPossession",
            Self::SlashEvidenceReport => "SlashEvidenceReport",
            Self::NodeServiceReceipt => "NodeServiceReceipt",
            Self::NodeServiceTicket => "NodeServiceTicket",
            Self::NodeSettlementEndorsement => "NodeSettlementEndorsement",
            Self::NodeBondAuthorization => "NodeBondAuthorization",
            Self::NodeBondPossession => "NodeBondPossession",
            Self::NodeBondPayoutPasskeyApproval => "NodeBondPayoutPasskeyApproval",
            Self::NodeReceiptSubmissionAuthorization => "NodeReceiptSubmissionAuthorization",
            Self::RetrievalAuditReceipt => "RetrievalAuditReceipt",
        }
    }
}

/// Coverage state of a signed object kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainCoverage {
    /// Implements `DomainSeparated` and has no conflicting legacy signing path.
    Implemented,
    /// Implements `DomainSeparated`, but the live production signing path still
    /// uses a legacy preimage. Cutting over is consensus-breaking.
    MigrationDeferred { issue: u32 },
    /// No Rust type exists to implement the trait against.
    Exception { issue: u32 },
    /// The type lives in a crate that **depends on** `shared-protocol`, so the
    /// impl cannot live here — adding the dependency would be a cycle.
    ///
    /// This is a real architectural constraint, not a deferral of convenience:
    /// `consensus-engines`, `sensor-attestation`, `keeper-proof` and
    /// `oracle-packet` all depend on `shared-protocol`, so `impl
    /// DomainSeparated for <their type>` must be written **in those crates**
    /// (foreign trait, local type — which coherence permits). The tag is
    /// reserved here so they cannot collide, and the scan in
    /// `tests/signed_type_inventory.rs` still holds them accountable.
    CrossCrate { issue: u32 },
}

impl DomainCoverage {
    /// Whether a `DomainSeparated` impl exists **in this crate** for this kind.
    pub fn has_impl(self) -> bool {
        matches!(self, Self::Implemented | Self::MigrationDeferred { .. })
    }

    /// Tracking issue, if this kind is not fully live.
    pub fn issue(self) -> Option<u32> {
        match self {
            Self::Implemented => None,
            Self::MigrationDeferred { issue }
            | Self::Exception { issue }
            | Self::CrossCrate { issue } => Some(issue),
        }
    }

    /// Stable identifier used in `docs/security/DOMAIN_TAGS.md`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "Implemented",
            Self::MigrationDeferred { .. } => "MigrationDeferred",
            Self::Exception { .. } => "Exception",
            Self::CrossCrate { .. } => "CrossCrate",
        }
    }
}

/// One row of the signed-object inventory.
#[derive(Clone, Copy, Debug)]
pub struct SignedObjectEntry {
    pub kind: SignedObjectKind,
    /// `DOMAIN_TAG` of the implementing type, or the tag reserved for it when
    /// coverage is [`DomainCoverage::Exception`].
    pub tag: &'static [u8],
    /// Fully-qualified Rust type, or `"(no type)"` for reserved tags.
    pub rust_type: &'static str,
    pub coverage: DomainCoverage,
}

/// Issue tracking the consensus-breaking cutover of every `MigrationDeferred`
/// row to its `DomainSeparated` preimage.
pub const MIGRATION_TRACKING_ISSUE: u32 = 2226;
// #2227 closed: `ValidatorSetChange` now exists and implements `DomainSeparated`
// (`consensus::validator_set::ValidatorSetChange`), so its registry row is
// `Implemented` and no reserved-exception issue constant is needed.
/// Issue: no wallet login-challenge type exists in the workspace.
///
/// Investigated for #2228: wallet login is **EVM secp256k1 / EIP-191**
/// (`personal_sign`, verified via `ethers.verifyMessage` / `ecrecover`), not an
/// ed25519 spending-key challenge. The backend mounts **two** such EVM challenge
/// flows, over different message strings:
///   - `/api/auth/nonce/:address` + `/api/auth/wallet-login`
///     (`wallet-auth-service.ts`): `"WRFCoin wallet login nonce: <nonce>"`;
///   - `/api/wallet-auth/nonce/:address` + `/api/wallet-auth/verify`
///     (`wallet-auth-routes.js`): `"Wrfcoin Authentication\nWallet: <addr>\nNonce: <nonce>"`.
///
/// The mobile app (`api_client.dart`) signs the first; the frontend
/// (`walletAuthService.ts::loginWithSigner`) **also signs the first** — it
/// requests the nonce, signs `buildWalletLoginMessage(nonce)` through the
/// connected wallet, and stores/uses the resulting JWT — another signer of the
/// existing challenge, not a third message format. **All** of these are
/// EVM/secp256k1 — a curve and key system disjoint from this crate's ed25519
/// `crypto::signature` — so none is signed by an ed25519 spending key and no
/// EIP-191 login signature can verify as any ed25519 `DomainSeparated` object
/// here. The cross-domain-replay surface this tag guards against therefore does
/// not currently exist in-Rust. The tag stays reserved (`Exception`, no type) so
/// a *future* native ed25519 wallet login must adopt this domain-separated shape
/// rather than pick a colliding tag; bringing the EVM login(s) into the audited
/// system is a separate backend/mobile/frontend migration. See
/// `docs/security/DOMAIN_TAGS.md`.
pub const WALLET_LOGIN_CHALLENGE_ISSUE: u32 = 2228;
/// Security issue: `GovernanceVotePayload`'s live signing message binds neither
/// chain id nor network id, so a testnet vote replays on mainnet.
pub const GOVERNANCE_VOTE_REPLAY_ISSUE: u32 = 2229;
/// Issue: `DomainSeparated` impls for signed types whose crates depend on
/// `shared-protocol`, so the impl must live in the owning crate.
pub const CROSS_CRATE_IMPL_ISSUE: u32 = 2249;

/// The signed-object inventory.
///
/// Adding a signed type to the protocol without adding a row here is caught by
/// [`tests::every_kind_is_registered`]; adding a row whose tag collides with
/// another is caught by [`tests::all_domain_tags_are_unique`].
pub const SIGNED_OBJECT_REGISTRY: &[SignedObjectEntry] = &[
    SignedObjectEntry {
        kind: SignedObjectKind::Transaction,
        tag: b"wrfcoin.transaction.v1",
        rust_type: "blockchain::transaction::Transaction",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::BlockHeader,
        tag: b"wrfcoin.block_header.v1",
        rust_type: "blockchain::block::Block",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::FinalityVote,
        // Review of PR #2231 caught this bound to `consensus::finality::
        // FinalitySignal`, which carries no signature field at all — #2226 even
        // records it as unsigned. The live signed finality vote is in
        // consensus-engines, which depends on shared-protocol, so its impl must
        // live there.
        tag: b"wrfcoin.finality_vote.v1",
        rust_type: "consensus_engines::selection::bft_finality::FinalityVote",
        coverage: DomainCoverage::CrossCrate {
            issue: CROSS_CRATE_IMPL_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::ValidatorSetChange,
        tag: b"wrfcoin.validator_set_change.v1",
        rust_type: "consensus::validator_set::ValidatorSetChange",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::ProducerSetChange,
        tag: b"wrfcoin.producer_set_change.v1",
        rust_type: "consensus::producer_set::ProducerSetChange",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::SlashingEvidence,
        tag: b"wrfcoin.slashing_report.v1",
        rust_type: "security::slashing::SlashingReportPayload",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::SnapshotPayload,
        tag: b"wrfcoin.snapshot_sync_payload.v2",
        rust_type: "blockchain::snapshot_sync::SnapshotSyncPayload",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::WeatherObservation,
        tag: b"wrfcoin.weather_observation.v1",
        rust_type: "weather::validation::WeatherReading",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::OracleAttestation,
        tag: b"wrfcoin.oracle_attestation.v1",
        rust_type: "blockchain::transaction::OracleAttestationPayload",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::AttestationEnvelope,
        tag: b"wrfcoin.calibration_attestation.v1",
        rust_type: "blockchain::transaction::CalibrationAttestationPayload",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::GovernanceProposal,
        tag: b"wrfcoin.governance_proposal.v1",
        rust_type: "governance::proposal::Proposal",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::GovernanceVote,
        tag: b"wrfcoin.governance_vote.v1",
        rust_type: "blockchain::transaction::GovernanceVotePayload",
        // #2229: the domain-separated path is wired but **version-gated** —
        // production `verify_vote_signature_at_height` path uses the domain
        // preimage only at V2 and the legacy
        // `governance-vote-v1|<id>|<approve>` string at V1 (to avoid a rolling
        // V1/V2 asymmetry). While the chain runs at V1 the live path is still
        // the legacy preimage, so this stays `MigrationDeferred` until the #2034
        // cut makes V2 the only active version.
        coverage: DomainCoverage::MigrationDeferred {
            issue: GOVERNANCE_VOTE_REPLAY_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::BridgeMessage,
        tag: b"wrfcoin.bridge_message.v1",
        rust_type: "cross_chain::types::BridgeMessage",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::WalletLoginChallenge,
        tag: b"wrfcoin.wallet_login_challenge.v1",
        rust_type: "(no type)",
        coverage: DomainCoverage::Exception {
            issue: WALLET_LOGIN_CHALLENGE_ISSUE,
        },
    },
    // --- omitted by the first submission; found in review of PR #2231 -------
    SignedObjectEntry {
        kind: SignedObjectKind::ComputeReceipt,
        tag: b"wrfcoin.compute_receipt.v1",
        rust_type: "blockchain::transaction::ComputeReceipt",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::ForecastVerificationReceipt,
        tag: b"wrfcoin.forecast_verification_receipt.v1",
        rust_type: "forecast_score::blockchain::ForecastVerificationReceipt",
        coverage: DomainCoverage::MigrationDeferred {
            issue: MIGRATION_TRACKING_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::DeviceAttestation,
        tag: b"wrfcoin.device_attestation.v1",
        rust_type: "sensor_attestation::DeviceAttestation",
        coverage: DomainCoverage::CrossCrate {
            issue: CROSS_CRATE_IMPL_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::StorageAttestation,
        tag: b"wrfcoin.storage_attestation.v1",
        rust_type: "keeper_proof::StorageAttestation",
        coverage: DomainCoverage::CrossCrate {
            issue: CROSS_CRATE_IMPL_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::OraclePacket,
        tag: b"wrfcoin.oracle_packet.v1",
        rust_type: "oracle_packet::OraclePacket",
        coverage: DomainCoverage::CrossCrate {
            issue: CROSS_CRATE_IMPL_ISSUE,
        },
    },
    SignedObjectEntry {
        kind: SignedObjectKind::UpgradeNotice,
        tag: b"wrfcoin.upgrade_notice.v1",
        rust_type: "governance::upgrade_notice::UpgradeNotice",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::FeatureScheduleExtension,
        tag: b"wrfcoin.feature_schedule_extension.v1",
        rust_type: "governance::schedule_extension::FeatureScheduleExtension",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::FaucetSinkPossession,
        tag: b"wrfcoin.faucet_sink_pop.v1",
        rust_type: "governance::faucet_sink_pop::FaucetSinkPossession",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NativeAccountSubject,
        tag: b"wrfcoin.native_smart_account.tx.v1",
        rust_type: "accounts::native_smart_accounts::NativeAccountSubject",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NativeSponsorSubject,
        tag: b"wrfcoin.native_smart_account.sponsor.v1",
        rust_type: "accounts::native_smart_accounts::NativeSponsorSubject",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NativeSessionPossession,
        tag: b"wrfcoin.native_smart_account.session_possession.v1",
        rust_type: "accounts::native_smart_accounts::NativeSessionPossession",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::SlashEvidenceReport,
        tag: b"wrfcoin.slash_evidence_report.v1",
        rust_type: "security::slash_evidence::SlashEvidenceReport",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeServiceReceipt,
        tag: b"wrfcoin.node_service_receipt.v1",
        rust_type: "consensus::node_service_receipt::NodeServiceReceipt",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeServiceTicket,
        tag: b"wrfcoin.node_service_ticket.v1",
        rust_type: "consensus::node_share_contracts::NodeServiceTicket",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeSettlementEndorsement,
        tag: b"wrfcoin.node_settlement_endorsement.v1",
        rust_type: "consensus::node_share_contracts::NodeSettlementEndorsement",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeBondAuthorization,
        tag: b"wrfcoin.node_bond_authorization.v1",
        rust_type: "consensus::node_share_contracts::NodeBondAuthorization",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeBondPossession,
        tag: b"wrfcoin.node_bond_possession.v1",
        rust_type: "consensus::node_share_contracts::NodeBondPossession",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeBondPayoutPasskeyApproval,
        tag: b"wrfcoin.node_bond_payout_passkey.v1",
        rust_type: "consensus::node_share_contracts::NodeBondPayoutPasskeyApproval",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::NodeReceiptSubmissionAuthorization,
        tag: b"wrfcoin.node_receipt_submission.v1",
        rust_type: "consensus::node_receipt_registry::NodeReceiptSubmissionAuthorization",
        coverage: DomainCoverage::Implemented,
    },
    SignedObjectEntry {
        kind: SignedObjectKind::RetrievalAuditReceipt,
        tag: b"wrfcoin.retrieval_audit_receipt.v1",
        rust_type: "keeper_proof::audit::RetrievalAuditReceipt",
        coverage: DomainCoverage::CrossCrate {
            issue: CROSS_CRATE_IMPL_ISSUE,
        },
    },
];

/// Every kind in [`SignedObjectKind`], for exhaustiveness checking.
///
/// Kept as an explicit list rather than derived, so adding a variant to the
/// enum without adding it here is a compile error at the `match` below.
pub const ALL_SIGNED_OBJECT_KINDS: &[SignedObjectKind] = &[
    SignedObjectKind::Transaction,
    SignedObjectKind::BlockHeader,
    SignedObjectKind::FinalityVote,
    SignedObjectKind::ValidatorSetChange,
    SignedObjectKind::ProducerSetChange,
    SignedObjectKind::SlashingEvidence,
    SignedObjectKind::SnapshotPayload,
    SignedObjectKind::WeatherObservation,
    SignedObjectKind::OracleAttestation,
    SignedObjectKind::AttestationEnvelope,
    SignedObjectKind::GovernanceProposal,
    SignedObjectKind::GovernanceVote,
    SignedObjectKind::BridgeMessage,
    SignedObjectKind::WalletLoginChallenge,
    SignedObjectKind::ComputeReceipt,
    SignedObjectKind::ForecastVerificationReceipt,
    SignedObjectKind::DeviceAttestation,
    SignedObjectKind::StorageAttestation,
    SignedObjectKind::OraclePacket,
    SignedObjectKind::UpgradeNotice,
    SignedObjectKind::FeatureScheduleExtension,
    SignedObjectKind::FaucetSinkPossession,
    SignedObjectKind::NativeAccountSubject,
    SignedObjectKind::NativeSponsorSubject,
    SignedObjectKind::NativeSessionPossession,
    SignedObjectKind::SlashEvidenceReport,
    SignedObjectKind::NodeServiceReceipt,
    SignedObjectKind::NodeServiceTicket,
    SignedObjectKind::NodeSettlementEndorsement,
    SignedObjectKind::NodeBondAuthorization,
    SignedObjectKind::NodeBondPossession,
    SignedObjectKind::NodeBondPayoutPasskeyApproval,
    SignedObjectKind::NodeReceiptSubmissionAuthorization,
    SignedObjectKind::RetrievalAuditReceipt,
];

/// Look up a registry row by kind.
pub fn entry(kind: SignedObjectKind) -> Option<&'static SignedObjectEntry> {
    SIGNED_OBJECT_REGISTRY.iter().find(|e| e.kind == kind)
}

/// Assert at runtime that a type's `DOMAIN_TAG` matches its registry row.
///
/// Used by the per-type tests so a tag renamed in code but not in the registry
/// (or vice versa) fails loudly rather than silently splitting the two.
pub fn tag_matches_registry<T: DomainSeparated>(kind: SignedObjectKind) -> bool {
    entry(kind).is_some_and(|e| e.tag == T::DOMAIN_TAG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The human-readable registry, embedded at compile time so it cannot drift.
    const REGISTRY_DOC: &str = include_str!("../../../docs/security/DOMAIN_TAGS.md");

    fn external_reserved_tags(document: &str) -> Vec<&str> {
        document
            .lines()
            .filter(|line| line.trim().starts_with('|'))
            .filter_map(|line| {
                let cells: Vec<&str> = line
                    .trim()
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                (cells.len() == 5 && cells[3] == "CrossCrateReserved")
                    .then(|| cells[1].trim_matches('`'))
            })
            .collect()
    }

    fn assert_domain_tags_unique(document: &str) {
        let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
        for e in SIGNED_OBJECT_REGISTRY {
            assert!(
                !e.tag.is_empty() && e.tag.len() < 256,
                "native tag violates the archive framing disjointness bound"
            );
            assert!(
                seen.insert(e.tag),
                "duplicate DOMAIN_TAG {:?} on kind {}",
                String::from_utf8_lossy(e.tag),
                e.kind.as_str()
            );
        }
        // External formats reserve names without inventing native Rust types.
        let external_tags = external_reserved_tags(document);
        for tag in &external_tags {
            assert!(
                seen.insert(tag.as_bytes()),
                "external reserved DOMAIN_TAG {tag} collides with another domain"
            );
        }
        assert_eq!(
            seen.len(),
            SIGNED_OBJECT_REGISTRY.len() + external_tags.len()
        );
    }

    #[test]
    fn all_domain_tags_are_unique() {
        assert_domain_tags_unique(REGISTRY_DOC);
        assert!(
            external_reserved_tags(REGISTRY_DOC).contains(&"wrfcoin.join_archive.v1"),
            "archive domain reservation absent"
        );
    }

    #[test]
    #[should_panic(expected = "external reserved DOMAIN_TAG")]
    fn external_archive_reservation_collision_is_refused() {
        // Keep the native registry intact; make the external fixture claim a
        // native tag. The original native-only checker misses this collision.
        let colliding_doc =
            REGISTRY_DOC.replace("`wrfcoin.join_archive.v1`", "`wrfcoin.transaction.v1`");
        assert_domain_tags_unique(&colliding_doc);
    }

    #[test]
    fn every_kind_is_registered_exactly_once() {
        for kind in ALL_SIGNED_OBJECT_KINDS {
            let matches = SIGNED_OBJECT_REGISTRY
                .iter()
                .filter(|e| e.kind == *kind)
                .count();
            assert_eq!(
                matches,
                1,
                "kind {} appears {matches} times in the registry, expected exactly 1",
                kind.as_str()
            );
        }
        assert_eq!(SIGNED_OBJECT_REGISTRY.len(), ALL_SIGNED_OBJECT_KINDS.len());
    }

    #[test]
    fn registry_is_injective_on_rust_type() {
        // Injectivity, the other half of tag uniqueness. Two distinct kinds
        // binding the SAME Rust type means one type answers to two tags, so a
        // signature made under one verifies under the other — the cross-protocol
        // replay hole from the opposite direction. Review of PR #2231 found the
        // adjacent defect (a kind bound to the wrong type), so bind-time
        // ambiguity is worth asserting explicitly.
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for e in SIGNED_OBJECT_REGISTRY {
            if e.rust_type == "(no type)" {
                continue; // reserved tags legitimately share the placeholder
            }
            assert!(
                seen.insert(e.rust_type),
                "rust_type {} is bound by more than one kind — one type must map \
                 to exactly one domain tag",
                e.rust_type
            );
        }
    }

    #[test]
    fn tags_follow_naming_convention() {
        for e in SIGNED_OBJECT_REGISTRY {
            let tag = std::str::from_utf8(e.tag).expect("tag must be UTF-8");
            assert!(
                tag.starts_with("wrfcoin."),
                "tag {tag} must be namespaced under `wrfcoin.`"
            );
            assert!(
                tag.rsplit('.').next().is_some_and(|v| v.starts_with('v')),
                "tag {tag} must end in a version segment"
            );
        }
    }

    #[test]
    fn every_kind_has_an_impl_or_a_filed_exception() {
        // The acceptance criterion for #2122: no kind may be silently
        // uncovered. Either the trait is implemented, or there is an issue.
        for e in SIGNED_OBJECT_REGISTRY {
            assert!(
                e.coverage.has_impl() || e.coverage.issue().is_some(),
                "kind {} has neither a DomainSeparated impl nor a filed exception issue",
                e.kind.as_str()
            );
            // Only `Exception` means "no type exists". `CrossCrate` names a
            // real type whose impl simply cannot live in this crate.
            if matches!(e.coverage, DomainCoverage::Exception { .. }) {
                assert_eq!(
                    e.rust_type,
                    "(no type)",
                    "kind {} is an Exception but names a concrete type",
                    e.kind.as_str()
                );
            }
            if matches!(e.coverage, DomainCoverage::CrossCrate { .. }) {
                assert_ne!(
                    e.rust_type,
                    "(no type)",
                    "kind {} is CrossCrate but names no type — CrossCrate exists \
                     precisely because the type is real but lives downstream",
                    e.kind.as_str()
                );
            }
        }
    }

    #[test]
    fn consensus_breaking_migrations_are_listed_not_applied() {
        // Guards the "listed, not silently applied" acceptance criterion: every
        // deferred migration must carry a tracking issue number.
        let deferred: Vec<_> = SIGNED_OBJECT_REGISTRY
            .iter()
            .filter(|e| matches!(e.coverage, DomainCoverage::MigrationDeferred { .. }))
            .collect();
        assert!(
            !deferred.is_empty(),
            "if every migration is live, delete this test along with the state"
        );
        for e in deferred {
            assert!(
                e.coverage.issue().is_some_and(|i| i > 0),
                "deferred migration for {} has no tracking issue",
                e.kind.as_str()
            );
        }
    }

    /// Parse the `| Kind | Tag | Type | Coverage | Issue |` rows out of the doc.
    ///
    /// Only rows whose first cell names a known [`SignedObjectKind`] count, so
    /// other tables in the document are ignored.
    fn doc_rows() -> Vec<Vec<String>> {
        REGISTRY_DOC
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with('|'))
            .map(|line| {
                line.trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().trim_matches('`').trim().to_string())
                    .collect::<Vec<String>>()
            })
            .filter(|cells| {
                cells.len() == 5
                    && ALL_SIGNED_OBJECT_KINDS
                        .iter()
                        .any(|k| k.as_str() == cells[0])
            })
            .collect()
    }

    #[test]
    fn registry_doc_matches_code() {
        // This is the CI check required by #2122: DOMAIN_TAGS.md and the
        // registry constant cannot drift apart, because the doc is compiled
        // into this test binary.
        let rows = doc_rows();
        assert_eq!(
            rows.len(),
            SIGNED_OBJECT_REGISTRY.len(),
            "DOMAIN_TAGS.md has {} object rows, registry has {}",
            rows.len(),
            SIGNED_OBJECT_REGISTRY.len()
        );

        for (e, row) in SIGNED_OBJECT_REGISTRY.iter().zip(rows.iter()) {
            let (kind, tag, rust_type, coverage, issue) =
                (&row[0], &row[1], &row[2], &row[3], &row[4]);
            assert_eq!(kind, e.kind.as_str(), "kind order/name mismatch");
            assert_eq!(
                tag.as_str(),
                String::from_utf8_lossy(e.tag).as_ref(),
                "tag mismatch for {kind}"
            );
            assert_eq!(rust_type, e.rust_type, "rust type mismatch for {kind}");
            assert_eq!(
                coverage,
                e.coverage.as_str(),
                "coverage state mismatch for {kind}"
            );
            let expected_issue = match e.coverage.issue() {
                Some(i) => format!("#{i}"),
                None => "—".to_string(),
            };
            assert_eq!(issue, &expected_issue, "issue mismatch for {kind}");
        }
    }
}
