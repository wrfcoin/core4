use crate::crypto::hash::hash_bytes;
use serde::Serialize;

/// Canonical protocol signing envelope version.
pub const SIGNING_SCHEME_VERSION: &str = "wrfcoin-sign-v1";
/// Canonical chain id used for protocol message domain separation. This is the
/// **fallback** value when no deployment override is configured; testnet and
/// other networks override it via [`CHAIN_ID_ENV`] (#2034).
pub const DEFAULT_CHAIN_ID: &str = "wrfcoin-main";
/// Canonical network id used for protocol message domain separation. Fallback
/// value; overridden per deployment via [`NETWORK_ID_ENV`] (#2034).
pub const DEFAULT_NETWORK_ID: &str = "mainnet";

/// Environment variable overriding the chain id folded into signing preimages
/// (#2034). Same variable the p2p chain-identity / #2035 boot assertion read, so
/// a node signs, verifies, and peers under one consistent identity.
pub const CHAIN_ID_ENV: &str = "WRFCOIN_CHAIN_ID";
/// Environment variable overriding the network id folded into signing preimages
/// (#2034).
pub const NETWORK_ID_ENV: &str = "WRFCOIN_NETWORK_ID";

/// Resolve the runtime chain id folded into signing preimages for cross-network
/// replay protection (#2034).
///
/// Precedence: a non-empty [`CHAIN_ID_ENV`] wins; otherwise the mainnet fallback
/// [`DEFAULT_CHAIN_ID`]. Read per call (cheap — env lives in process memory) so
/// that unit tests can vary it without a frozen cache; production sets it once
/// at process start. The signer (wallet/faucet/collector) and the verifier
/// (node) MUST resolve the same value or signatures fail to verify — this is the
/// intended cross-network replay barrier.
pub fn signing_chain_id() -> String {
    configured_chain_id().unwrap_or_else(|| DEFAULT_CHAIN_ID.to_string())
}

/// Resolve the runtime network id folded into signing preimages (#2034). See
/// [`signing_chain_id`] for precedence and cross-signer requirements.
pub fn signing_network_id() -> String {
    configured_network_id().unwrap_or_else(|| DEFAULT_NETWORK_ID.to_string())
}

/// The chain id the deployment **declared**, or `None` when it declared nothing.
///
/// This is [`signing_chain_id`]'s env-reading half, extracted **verbatim** so the
/// two cannot drift: `signing_chain_id()` is now exactly
/// `configured_chain_id().unwrap_or_else(|| DEFAULT_CHAIN_ID.to_string())`, which
/// is the same expression it has always evaluated. A caller that sets
/// [`CHAIN_ID_ENV`] therefore gets a byte-identical result before and after this
/// split — asserted in `chain_identity_fail_closed_2713.rs`.
///
/// It exists so a caller can tell **declared** from **defaulted**, which
/// [`signing_chain_id`] deliberately cannot: it returns a `String` either way, and
/// that indistinguishability is #2713.
///
/// Empty-after-trim counts as *undeclared*, matching the filter
/// `signing_chain_id` already applied. `WRFCOIN_CHAIN_ID=` is how "unset" is
/// written in a compose file or a systemd unit, and accepting it as a declaration
/// would reintroduce the silent default one layer down.
pub fn configured_chain_id() -> Option<String> {
    std::env::var(CHAIN_ID_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// The network id the deployment **declared**, or `None`. See
/// [`configured_chain_id`].
pub fn configured_network_id() -> Option<String> {
    std::env::var(NETWORK_ID_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// The chain identity a node is permitted to run under: **declared, or refused**.
///
/// # Why this is fail-CLOSED and not "default to testnet"
///
/// #2713: with both variables unset a node REPORTS `chain_id 777` /
/// `network_id unknown` (inline literals at `routes/network.rs:191-192`) while
/// every signature it produces is bound to `wrfcoin-main` / `mainnet`, because
/// [`signing_chain_id`] is what folds into the signing preimage. Nothing compares
/// the two, so the disagreement can persist indefinitely with no diagnostic.
///
/// Defaulting the unset case to *testnet* would be the inverse mistake #2523
/// deleted `GENESIS_ORACLE_SET_ENV` for: there, "unset" validated nothing and said
/// nothing, and one forgotten export on cut day would have let a node accept a
/// unanimous-but-wrong block 1 in silence. A plausible default is the thing that
/// makes a misconfiguration survivable long enough to matter.
///
/// **A node that will not start is recoverable in thirty seconds. A node that
/// starts and signs under the wrong domain is not** — and per #2056 a
/// wrong-commit node is invisible to the chain, so there is no reason to expect
/// anything downstream to surface it.
///
/// # The compounding this also closes (#2717)
///
/// `WRFCOIN_NETWORK_ID` is not only a signing input. It is one of the two ways the
/// #2035 genesis-identity check acquires a *committing expectation* — the other
/// being an explicit `WRFCOIN_GENESIS_HASH`. With it unset there is no
/// expectation, so that check is skipped and a node will start against a data
/// volume from another chain. One unset variable, two hazards: it makes the node
/// sign under the mainnet domain, **and** it disables the guard that would have
/// caught the resulting foreign volume. Refusing here removes both.
///
/// # What this does NOT change
///
/// Nothing, for a correctly configured node. When both variables are set and
/// non-empty this returns them unchanged, and every existing caller of
/// [`signing_chain_id`] / [`signing_network_id`] resolves exactly as before. This
/// is a startup precondition, not a validity predicate: it does not participate
/// in block validation, state transitions, or `state_root`, so it is not a
/// consensus change.
pub fn declared_chain_identity() -> Result<DeclaredChainIdentity, String> {
    match (configured_chain_id(), configured_network_id()) {
        (Some(chain_id), Some(network_id)) => Ok(DeclaredChainIdentity {
            chain_id,
            network_id,
        }),
        (chain, network) => {
            let mut missing = Vec::new();
            if chain.is_none() {
                missing.push(CHAIN_ID_ENV);
            }
            if network.is_none() {
                missing.push(NETWORK_ID_ENV);
            }
            Err(format!(
                "chain identity is not declared: {} unset or empty. This node would \
                 REPORT one identity and SIGN under another — signatures fold in \
                 `{DEFAULT_CHAIN_ID}`/`{DEFAULT_NETWORK_ID}` by fallback while \
                 /network/info reports the testnet-looking literals, and nothing \
                 compares them (#2713). An unset {NETWORK_ID_ENV} additionally \
                 leaves the #2035 genesis-identity check with no expectation to \
                 enforce, so a data volume from another chain would be accepted \
                 (#2717). Set both explicitly — there is deliberately no default, \
                 because a plausible one is what lets this survive to production.",
                missing.join(" and ")
            ))
        },
    }
}

/// A chain identity that was declared rather than defaulted.
///
/// Constructible only through [`declared_chain_identity`], so holding one is
/// evidence the deployment said what network it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredChainIdentity {
    /// The value of [`CHAIN_ID_ENV`], non-empty after trimming.
    pub chain_id: String,
    /// The value of [`NETWORK_ID_ENV`], non-empty after trimming.
    pub network_id: String,
}

/// Environment variable holding the allowlist of ed25519 public keys authorized
/// to sign gamification-reward payouts (mobile#215 / core4#2081 review).
///
/// Comma-separated lowercase hex, e.g. `aabb..,ccdd..`. This is the SAME
/// deployment-config trust model as [`CHAIN_ID_ENV`]: every node MUST be
/// configured with the same allowlist or they disagree on which gamification
/// payouts are valid — exactly as they must agree on the chain id. The set is
/// deliberately NOT in `state_root`; it is a validity predicate over deployment
/// config, like the chain identity a signature is bound to.
pub const GAMIFICATION_OPERATOR_PUBKEYS_ENV: &str = "WRFCOIN_GAMIFICATION_OPERATOR_PUBKEYS";

/// Resolve the authorized gamification-operator public keys (lowercase hex).
///
/// Read per call (cheap; env lives in process memory) so tests can vary it
/// without a frozen cache; production sets it once at process start. An empty or
/// unset value yields an empty allowlist — which means EVERY gamification payout
/// fails authorization (fail-closed): a node that has not been told who may
/// authorize payouts must not accept any.
pub fn gamification_operator_pubkeys() -> Vec<String> {
    std::env::var(GAMIFICATION_OPERATOR_PUBKEYS_ENV)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `pubkey_hex` is in the authorized gamification-operator allowlist.
///
/// Case-insensitive on the hex. Fail-closed: an empty allowlist authorizes no
/// one.
pub fn is_authorized_gamification_operator(pubkey_hex: &str) -> bool {
    let needle = pubkey_hex.trim().to_ascii_lowercase();
    !needle.is_empty() && gamification_operator_pubkeys().iter().any(|k| k == &needle)
}

/// Process-wide lock serializing tests that mutate
/// [`GAMIFICATION_OPERATOR_PUBKEYS_ENV`] — env is global, so two tests setting
/// it to different values in parallel would flake. Cross-module so every
/// gamification-auth test in this crate's test binary shares one lock. Poison is
/// recovered (a panicking test still leaves the env restored by its own guard).
#[cfg(test)]
pub(crate) static GAMIFICATION_ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Environment variable holding the allowlist of ed25519 public keys authorized
/// to sign on-chain oracle-registration transactions (#2055 Option A, oracle
/// gate #724).
///
/// Comma-separated lowercase hex. Same deployment-config trust model as
/// [`GAMIFICATION_OPERATOR_PUBKEYS_ENV`] / [`CHAIN_ID_ENV`]: every node MUST be
/// configured with the same allowlist or they disagree on which oracle
/// registrations are valid. Deliberately NOT in `state_root` — a validity
/// predicate over deployment config, like the chain identity a signature binds
/// to. Registering/rotating oracle operators on a LIVE chain flows through a
/// signed transaction (not the out-of-band admin route, which forks a node and
/// is height-0 only), so this key is the governance authority for that tx.
pub const ORACLE_GOVERNANCE_PUBKEYS_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

/// Resolve the authorized oracle-governance public keys (lowercase hex).
///
/// Read per call so tests can vary it without a frozen cache; production sets it
/// once at start. Empty/unset yields an empty allowlist — every oracle
/// registration then fails authorization (fail-closed): a node not told who may
/// authorize registrations must accept none.
pub fn oracle_governance_pubkeys() -> Vec<String> {
    std::env::var(ORACLE_GOVERNANCE_PUBKEYS_ENV)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ── REMOVED (#2523): `GENESIS_ORACLE_SET_ENV` / `ExpectedGenesisOracleSet` /
// `expected_genesis_oracle_set` ────────────────────────────────────────────
//
// These resolved the canonical genesis oracle set from THIS NODE'S environment,
// at validation time, and #2496 checked every block-1 `OracleRegistration`
// against the result. Bob refuted his own fix and the manager ruled on it: the
// `Ok(None)` arm — variable unset — validated nothing and said nothing. It was
// reachable only while validating a registration, i.e. after the ceremony, when
// the oracle keys demonstrably exist, so "unset" there was always operator error
// and never a pre-ceremony state. One forgotten export on cut day and that node
// accepted a unanimous-but-wrong block 1 in silence while its correctly
// configured peers refused it. A partition, at best.
//
// The replacement is not a stricter version of this. It is a different answer to
// "who decides": block 1 declares the oracle set in its own metadata, and every
// node — configured, misconfigured, or unconfigured — reads the declaration back
// out of the block. See `crypto::genesis_oracle` and
// `ProofOfPublishing::check_block_one_genesis_oracles`.
//
// The variable name survives as `genesis_oracle::CUT_ORACLE_SET_ENV`, read ONCE
// by the node that cuts block 1 and written straight into that block. Do not
// reintroduce an environment read on any validation path: node-local
// configuration is precisely what made the previous design a partition risk.

/// Whether `pubkey_hex` is in the authorized oracle-governance allowlist.
/// Case-insensitive; fail-closed on an empty allowlist.
pub fn is_authorized_oracle_governance(pubkey_hex: &str) -> bool {
    let needle = pubkey_hex.trim().to_ascii_lowercase();
    !needle.is_empty() && oracle_governance_pubkeys().iter().any(|k| k == &needle)
}

/// The number of oracle-governance keys the deployment **declares** it expects
/// (#2712).
///
/// Optional, and only load-bearing when the allowlist holds more than one entry —
/// see [`check_oracle_governance_allowlist_declaration`] for exactly when it is
/// required and why it is not always.
pub const ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT";

/// Startup check: is the oracle-governance allowlist **unambiguous**? (#2712)
///
/// Returns the resolved entry count, or a refusal.
///
/// # The defect
///
/// [`oracle_governance_pubkeys`] splits a comma list and authorizes against
/// whatever remains — no cardinality check, no expected-count assertion, nothing
/// that reports the resolved size. So a rotation performed as
/// `=<old>,<new>` (an APPEND) instead of `=<new>` (a REPLACE) leaves the retired
/// key fully authorized, indefinitely, and nothing says so. **Rotation is exactly
/// when a human appends**: the intent is "add the new key", and appending is the
/// literal expression of that intent, while the safe action is replace.
/// Demonstrated at the real gate in
/// `native-chain/tests/oracle_governance_rotation_2712.rs`.
///
/// # Why this refuses the AMBIGUOUS state rather than the multi-key state
///
/// A two-entry allowlist is syntactically legitimate, and refusing it outright
/// would break a deliberate two-signer deployment. But a two-entry allowlist is
/// also precisely what append-instead-of-replace produces, and from the value
/// alone **the two are indistinguishable**. That indistinguishability is the
/// defect, so that is what this refuses: not "more than one key", but "more than
/// one key that nobody declared".
///
/// | Entries | [`ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV`] | Result |
/// |---|---|---|
/// | 0 or 1 | absent | **Ok** — unambiguous; nothing a rotation could have silently added |
/// | 0 or 1 | present, matching | Ok |
/// | ≥ 2 | absent | **REFUSE** — the ambiguous state |
/// | any | present, mismatching | **REFUSE** — the declaration is the point |
/// | duplicates present | any | **REFUSE** — a list containing the same key twice is a list nobody read |
///
/// Requiring the declaration only above one entry is what keeps this from
/// changing behaviour for a correctly configured node: every setter in this
/// workspace configures exactly one governance key, and
/// `scripts/verify-oracle-governance-allowlist.sh` cannot even *express* a
/// multi-key expectation (`--expected-pubkey` is singular). There is no deliberate
/// multi-signer deployment for this to break.
///
/// # Why a startup check and not a resolver change
///
/// [`oracle_governance_pubkeys`] is on the per-transaction validation path:
/// `verify_oracle_registration_authorization` calls it for every registration, on
/// the producer's self-screen and on every peer's `verify_block`. Refusing THERE
/// would make a node reject registrations its peers accept — a consensus-visible
/// change, and a partition. This is a startup precondition instead: it takes no
/// part in block validation, state transitions, or `state_root`. A node either
/// runs or does not.
///
/// # Not a substitute for the out-of-band audit
///
/// `scripts/verify-oracle-governance-allowlist.sh` already checks a *running*
/// process against an exact expected key, and it would catch this mistake. It only
/// catches it when someone runs it, and it takes the expectation as a
/// command-line argument at audit time rather than from the deployment. This moves
/// the same question to boot, where it is asked every time and by nobody.
pub fn check_oracle_governance_allowlist_declaration() -> Result<usize, String> {
    let entries = oracle_governance_pubkeys();

    // Duplicates first: they make any count assertion below ambiguous, and a list
    // holding the same key twice is unambiguously a list that was edited blind.
    let mut seen = std::collections::BTreeSet::new();
    let duplicates = entries.iter().filter(|k| !seen.insert((*k).clone())).count();
    if duplicates > 0 {
        return Err(format!(
            "{ORACLE_GOVERNANCE_PUBKEYS_ENV} lists {} entries but only {} are distinct \
             ({duplicates} duplicate(s)). A list containing the same key twice is a list \
             nobody read — correct it rather than declaring the duplicate count (#2712).",
            entries.len(),
            seen.len(),
        ));
    }

    // determinism-ok: startup-only declaration check, read once before block processing begins to decide whether this node refuses to boot; the value never enters a signing preimage, a state transition, or the state root, so two nodes with different values still compute identical roots (#2712).
    let declared = std::env::var(ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty());

    match declared {
        Some(raw) => {
            let expected: usize = raw.trim().parse().map_err(|_| {
                format!(
                    "{ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV}={raw:?} is not a non-negative \
                     integer. Refusing rather than ignoring it: a declaration that cannot \
                     be parsed must not read as 'no declaration' (#2712)."
                )
            })?;
            if expected != entries.len() {
                return Err(format!(
                    "{ORACLE_GOVERNANCE_PUBKEYS_ENV} resolves to {} key(s) but \
                     {ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV} declares {expected}. If a rotation \
                     was just performed, check whether it APPENDED the new key instead of \
                     REPLACING the old one — an appended list leaves the retired key fully \
                     authorized (#2712). Refusing to start.",
                    entries.len()
                ));
            }
            Ok(entries.len())
        },
        None if entries.len() > 1 => Err(format!(
            "{ORACLE_GOVERNANCE_PUBKEYS_ENV} resolves to {} keys and \
             {ORACLE_GOVERNANCE_PUBKEY_COUNT_ENV} is not set, so this node cannot tell a \
             deliberate multi-signer allowlist from a rotation that APPENDED the new key \
             instead of REPLACING the retired one — the two are identical in the value, and \
             the second leaves a retired key authorized (#2712). Declare the expected count \
             explicitly, or reduce the allowlist to the single current key. There is \
             deliberately no default: picking one of the two readings is what lets the wrong \
             one survive to production.",
            entries.len()
        )),
        None => Ok(entries.len()),
    }
}

/// Test lock for [`ORACLE_GOVERNANCE_PUBKEYS_ENV`], see
/// [`GAMIFICATION_ENV_TEST_LOCK`].
#[cfg(test)]
pub(crate) static ORACLE_GOVERNANCE_ENV_TEST_LOCK: std::sync::Mutex<()> =
    std::sync::Mutex::new(());

/// Signed action domains. Each action maps to a unique domain string to prevent cross-action replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainSigningAction {
    OracleAttestation,
    CalibrationAttestation,
    BridgeAction,
    GovernanceAction,
    KeyRotation,
    SnapshotSync,
    ForecastVerification,
    ComputeReceipt,
    SelectiveDisclosure,
    WeatherDataGossip,
    /// Reporter signature over a `SlashingReportPayload` (#1393 round-4 P0).
    /// Domain separation prevents a signature harvested from any other
    /// action type being replayed as a slashing authorisation.
    SlashingReport,
    /// Governance signature over an `UpgradeNotice` (#3094): the activation
    /// height, minimum protocol version, features and release digests an
    /// operator installs before the height. Its own domain so a signature
    /// harvested from any other governance action cannot be replayed as an
    /// upgrade instruction.
    UpgradeNotice,
    /// Governance signature over a `FeatureScheduleExtension` (#3277): the
    /// future `(feature, height)` activations appended to a running chain's
    /// consensus schedule. Its own domain, distinct from `UpgradeNotice`: a
    /// notice is advisory and off-chain, an extension is consensus, so a
    /// signature over either must never verify as the other.
    FeatureScheduleExtension,
    /// Faucet-key signature over a `FaucetSinkPossession` claim (#3361 F3):
    /// proof that the key controlling a `LEGACY_FAUCET_BINDINGS` address
    /// consents to it being one chain instance's faucet sink. Off-chain; it
    /// gates the SIGNING of the governance extension. Its own domain, so no
    /// signature the faucet key makes elsewhere (a transfer authorization)
    /// and no governance signature can be replayed as one.
    FaucetSinkPossession,
}

impl DomainSigningAction {
    pub fn domain(self) -> &'static str {
        match self {
            Self::OracleAttestation => "wrfcoin.oracle_attestation.v1",
            Self::CalibrationAttestation => "wrfcoin.calibration_attestation.v1",
            Self::BridgeAction => "wrfcoin.bridge_action.v1",
            Self::GovernanceAction => "wrfcoin.governance_action.v1",
            Self::KeyRotation => "wrfcoin.key_rotation.v1",
            Self::SnapshotSync => "wrfcoin.snapshot_sync.v1",
            Self::ForecastVerification => "wrfcoin.forecast_verification.v1",
            Self::ComputeReceipt => "wrfcoin.compute_receipt.v1",
            Self::SelectiveDisclosure => "wrfcoin.selective_disclosure.v1",
            Self::WeatherDataGossip => "wrfcoin.weather_data_gossip.v1",
            Self::SlashingReport => "wrfcoin.slashing_report.v1",
            Self::UpgradeNotice => "wrfcoin.upgrade_notice.v1",
            Self::FeatureScheduleExtension => "wrfcoin.feature_schedule_extension.v1",
            Self::FaucetSinkPossession => "wrfcoin.faucet_sink_pop.v1",
        }
    }
}

/// Canonical signing context shared across action types.
#[derive(Clone, Debug)]
pub struct DomainSigningContext<'a> {
    pub action: DomainSigningAction,
    pub signer_id: &'a str,
    pub timestamp: i64,
    pub nonce: Option<&'a str>,
    pub chain_id: &'a str,
    pub network_id: &'a str,
}

/// Compute deterministic payload digest used by signing envelopes.
///
/// Panics if the payload cannot serialize — the only realistic cause is a
/// non-finite `f64` field (`serde_json` rejects NaN/Infinity). Callers that
/// verify signatures over remotely-supplied payloads MUST reject non-finite
/// floats *before* calling this (see `OracleAttestationPayload::is_signable`),
/// so the panic is only reachable from a local programming error, never from
/// network input.
pub fn payload_digest_hex<T: Serialize>(payload: &T) -> String {
    let bytes = serde_json::to_vec(payload)
        .expect("payload_digest_hex: protocol signing types must serialize cleanly — reject non-finite f64 fields before signing/verifying (see OracleAttestationPayload::is_signable)");
    hash_bytes(&bytes).to_string()
}

/// Build canonical protocol signing envelope.
pub fn canonical_signing_message(
    context: &DomainSigningContext<'_>,
    payload_digest: &str,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        SIGNING_SCHEME_VERSION,
        context.action.domain(),
        context.chain_id,
        context.network_id,
        context.signer_id,
        context.timestamp,
        context.nonce.unwrap_or(""),
        payload_digest
    )
}

// ---------------------------------------------------------------------------
// Typed domain separation, chain-and-version bound (#2122 + #2123)
// ---------------------------------------------------------------------------
//
// `DomainSigningAction` above is a *string envelope* keyed off an action enum,
// and it only covers the eleven actions whose call sites happened to adopt it.
// #2065 proved that leaves gaps: coverage was measured from call sites, so a
// signed type that never called the module was invisible.
//
// `DomainSeparated` inverts that. Coverage is a property of the **type**: a
// signed object either implements this trait — and therefore has a tag in
// `crate::crypto::domain_registry` — or it is a recorded, issue-tracked
// exception. There is no third state, and `domain_registry`'s tests fail if one
// appears.
//
// #2123 then moved the *framing* off the implementor. #2122 (PR #2231) made
// each impl build its own preimage via `domain_preimage(TAG, &body)`; an impl
// that forgot the chain binding would have compiled and signed happily. The
// required method is now `canonical_body` — the object's bytes alone — and tag,
// chain id, network id and version are applied by the provided methods below.
// "Every signed type is chain- and version-bound" is therefore structural
// rather than a convention each impl has to remember.
//
// This API is **additive**. It does not replace any existing production signing
// path; migrating those is consensus-breaking and is tracked per type in the
// registry. See `docs/security/DOMAIN_TAGS.md`.

/// Append a length-prefixed field to a preimage buffer.
///
/// The 8-byte little-endian length prefix is what makes concatenation
/// unambiguous: without it, `("ab", "c")` and `("a", "bc")` produce identical
/// bytes, and a signature over one is a valid signature over the other. Every
/// variable-length component of a domain preimage goes through here.
pub fn push_length_prefixed(buf: &mut Vec<u8>, field: &[u8]) {
    buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
    buf.extend_from_slice(field);
}

/// Unbound domain preimage: length-prefixed tag, then length-prefixed
/// canonical body.
///
/// Equivalent to [`crate::crypto::chain_binding::domain_preimage_bound`] at
/// `V1`, and kept as the primitive the V1 path is *defined* against — V1
/// byte-identity is asserted against this function, not against a copy of its
/// behaviour.
///
/// Implementors of [`DomainSeparated`] no longer call this directly (#2123);
/// they return an unframed [`DomainSeparated::canonical_body`] and the trait's
/// provided methods apply the framing. It stays public because the V1 identity
/// tests and any non-trait caller need one unambiguous framing.

pub fn domain_preimage(tag: &[u8], canonical_body: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16 + tag.len() + canonical_body.len());
    push_length_prefixed(&mut buf, tag);
    push_length_prefixed(&mut buf, canonical_body);
    buf
}

/// A protocol object that is signed under its own unique domain tag, bound to
/// one chain and one protocol version.
///
/// Implementing this trait is the *only* sanctioned way to add a new signed
/// object. `DOMAIN_TAG` must be unique across the whole binary — the
/// `domain_registry::tests::all_domain_tags_are_unique` test is a hard failure
/// on collision, because two objects sharing a tag means a signature over one
/// verifies as the other.
///
/// Implementors supply only [`DomainSeparated::canonical_body`]. Framing — tag,
/// chain id, network id, version — is applied by the provided methods, so no
/// impl can accidentally omit the binding that makes its signatures
/// network-specific. That is the structural fix for #2065.
pub trait DomainSeparated {
    /// Globally unique tag for this object type, by convention
    /// `wrfcoin.<snake_case_object>.v<n>`.
    const DOMAIN_TAG: &'static [u8];

    /// The object's own bytes, without any framing.
    ///
    /// Do **not** prepend `DOMAIN_TAG` or any chain/version material here —
    /// the provided methods below add all of it. An impl that framed its own
    /// bytes would double-frame, and (worse) an impl that framed with
    /// [`domain_preimage`] alone would silently drop the chain binding.
    ///
    /// Must be a deterministic function of the object's value alone — no
    /// wall-clock reads, no unordered-map iteration, no float arithmetic.
    fn canonical_body(&self) -> Vec<u8>;

    /// Preimage under an explicit chain identity and protocol version.
    ///
    /// Prefer this in tests and anywhere the identity is already known; it
    /// reads no ambient state.
    fn signing_preimage_for(
        &self,
        identity: &crate::crypto::chain_binding::ChainIdentity,
        version: crate::crypto::chain_binding::SigningProtocolVersion,
    ) -> Vec<u8> {
        crate::crypto::chain_binding::domain_preimage_bound(
            Self::DOMAIN_TAG,
            identity,
            version,
            &self.canonical_body(),
        )
    }

    /// Preimage under the process's active chain identity and protocol version.
    fn signing_preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(
            crate::crypto::chain_binding::ChainIdentity::active(),
            crate::crypto::chain_binding::SigningProtocolVersion::active(),
        )
    }
}

/// Sign an object under its domain tag, bound to an explicit chain and version.
pub fn sign_domain_for<T: DomainSeparated>(
    signer: &crate::crypto::signature::PrivateKey,
    obj: &T,
    identity: &crate::crypto::chain_binding::ChainIdentity,
    version: crate::crypto::chain_binding::SigningProtocolVersion,
) -> crate::crypto::signature::Signature {
    signer.sign(&obj.signing_preimage_for(identity, version))
}

/// Verify a signature produced by [`sign_domain_for`] under the same chain and
/// version.
pub fn verify_domain_for<T: DomainSeparated>(
    public_key: &crate::crypto::signature::PublicKey,
    obj: &T,
    signature: &crate::crypto::signature::Signature,
    identity: &crate::crypto::chain_binding::ChainIdentity,
    version: crate::crypto::chain_binding::SigningProtocolVersion,
) -> bool {
    public_key.verify(&obj.signing_preimage_for(identity, version), signature)
}

/// Sign an object under the process's active chain identity and version.
///
/// The plan in #2122 wrote this as `sign_domain(signer: &Signer, ..)`; this
/// codebase's signing type is [`PrivateKey`] (there is no `Signer` struct —
/// `crypto::signer::BlockSigner` is a trait over block signing specifically),
/// so the concrete type is used here.
pub fn sign_domain<T: DomainSeparated>(
    signer: &crate::crypto::signature::PrivateKey,
    obj: &T,
) -> crate::crypto::signature::Signature {
    signer.sign(&obj.signing_preimage())
}

/// Verify a signature produced by [`sign_domain`] over the same object type,
/// on the same chain, under the same protocol version.
///
/// A signature over a *different* `DomainSeparated` type cannot verify here:
/// the tag is inside the preimage, so the verified message differs. Since
/// #2123 the same is true across chains and across protocol versions.
pub fn verify_domain<T: DomainSeparated>(
    public_key: &crate::crypto::signature::PublicKey,
    obj: &T,
    signature: &crate::crypto::signature::Signature,
) -> bool {
    public_key.verify(&obj.signing_preimage(), signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[derive(Serialize)]
    struct TestPayload<'a> {
        id: &'a str,
        approved: bool,
    }

    struct AlphaObject(&'static str);
    struct BetaObject(&'static str);

    impl DomainSeparated for AlphaObject {
        const DOMAIN_TAG: &'static [u8] = b"wrfcoin.test_alpha.v1";
        fn canonical_body(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }
    }

    impl DomainSeparated for BetaObject {
        const DOMAIN_TAG: &'static [u8] = b"wrfcoin.test_beta.v1";
        fn canonical_body(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }
    }

    fn test_key() -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(2123);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        (sk, pk)
    }

    #[test]
    fn signature_does_not_carry_across_domains_with_identical_bodies() {
        let (sk, pk) = test_key();
        let identity = ChainIdentity::new("wrfcoin-testnet", "testnet");
        let alpha = AlphaObject("identical-body");
        let beta = BetaObject("identical-body");

        let sig = sign_domain_for(&sk, &alpha, &identity, SigningProtocolVersion::V2);

        assert!(verify_domain_for(
            &pk,
            &alpha,
            &sig,
            &identity,
            SigningProtocolVersion::V2
        ));
        assert!(
            !verify_domain_for(&pk, &beta, &sig, &identity, SigningProtocolVersion::V2),
            "an Alpha signature must not verify as a Beta object"
        );
    }

    #[test]
    fn signature_does_not_carry_across_chains_for_the_same_object() {
        let (sk, pk) = test_key();
        let testnet = ChainIdentity::new("wrfcoin-testnet", "testnet");
        let mainnet = ChainIdentity::new("wrfcoin-main", "mainnet");
        let obj = AlphaObject("body");

        let sig = sign_domain_for(&sk, &obj, &testnet, SigningProtocolVersion::V2);

        assert!(verify_domain_for(
            &pk,
            &obj,
            &sig,
            &testnet,
            SigningProtocolVersion::V2
        ));
        assert!(
            !verify_domain_for(&pk, &obj, &sig, &mainnet, SigningProtocolVersion::V2),
            "a testnet signature must not verify on mainnet"
        );
    }

    #[test]
    fn signature_does_not_carry_across_protocol_versions() {
        let (sk, pk) = test_key();
        let identity = ChainIdentity::new("wrfcoin-main", "mainnet");
        let obj = AlphaObject("body");

        let v2_sig = sign_domain_for(&sk, &obj, &identity, SigningProtocolVersion::V2);
        assert!(!verify_domain_for(
            &pk,
            &obj,
            &v2_sig,
            &identity,
            SigningProtocolVersion::V1
        ));
    }

    #[test]
    fn length_prefixing_makes_concatenation_unambiguous() {
        let mut left = Vec::new();
        push_length_prefixed(&mut left, b"ab");
        push_length_prefixed(&mut left, b"c");

        let mut right = Vec::new();
        push_length_prefixed(&mut right, b"a");
        push_length_prefixed(&mut right, b"bc");

        assert_ne!(left, right);
    }

    #[test]
    fn domain_separation_changes_message() {
        let payload = TestPayload {
            id: "abc",
            approved: true,
        };
        let digest = payload_digest_hex(&payload);

        let mut context = DomainSigningContext {
            action: DomainSigningAction::OracleAttestation,
            signer_id: "oracle-1",
            timestamp: 1_700_000_000,
            nonce: Some("n1"),
            chain_id: DEFAULT_CHAIN_ID,
            network_id: DEFAULT_NETWORK_ID,
        };
        let oracle_message = canonical_signing_message(&context, &digest);

        context.action = DomainSigningAction::BridgeAction;
        let bridge_message = canonical_signing_message(&context, &digest);

        assert_ne!(oracle_message, bridge_message);
    }

    // ---- typed domain separation (#2122), retained through the #2123 rebase ----
    //
    // #2231's fixtures (`AlphaObject`, `BetaObject`, `test_key`) and its
    // cross-domain and length-prefix tests were duplicated by the rebase, since
    // this branch had already added equivalents above. The duplicates are
    // dropped; the two tests below are #2231's and have no equivalent above, so
    // they are kept:
    //
    //   * `sign_domain_round_trips` is the only test that exercises the
    //     *ambient* path (`sign_domain`/`verify_domain` reading the active
    //     identity and version) rather than the explicit `_for` variants.
    //   * `preimage_starts_with_length_prefixed_tag` pins the framing layout.
    //     `domain_preimage_bound` pushes the tag first at every version, so this
    //     holds under V1 and V2 alike — if a future version ever moved the tag,
    //     this is the test that should fail.

    #[test]
    fn sign_domain_round_trips() {
        let (sk, pk) = test_key();
        let obj = AlphaObject("body");
        let sig = sign_domain(&sk, &obj);
        assert!(verify_domain(&pk, &obj, &sig));
    }

    #[test]
    fn preimage_starts_with_length_prefixed_tag() {
        let obj = AlphaObject("body");
        let preimage = obj.signing_preimage();
        let tag = AlphaObject::DOMAIN_TAG;

        assert_eq!(&preimage[0..8], &(tag.len() as u64).to_le_bytes());
        assert_eq!(&preimage[8..8 + tag.len()], tag);
    }
}
