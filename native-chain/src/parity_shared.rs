use crate as native;
use rand::{SeedableRng, rngs::StdRng};
use rustc_hash::FxHashSet;
use std::fmt;
use wrfcoin_shared_protocol as shared;

pub(crate) const PARITY_CRITICAL_MODULES: [&str; 5] =
    ["accounts", "identity", "staking", "weather", "invariants"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BehaviorClass {
    Serialization,
    Semantics,
    ErrorBehavior,
}

impl BehaviorClass {
    fn label(self) -> &'static str {
        match self {
            Self::Serialization => "serialization",
            Self::Semantics => "semantics",
            Self::ErrorBehavior => "error_behavior",
        }
    }
}

const BEHAVIOR_CLASSES: [BehaviorClass; 3] = [
    BehaviorClass::Serialization,
    BehaviorClass::Semantics,
    BehaviorClass::ErrorBehavior,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BaselinePolicy {
    LegacyEquivalent,
    CanonicalOnly,
}

impl BaselinePolicy {
    fn label(self) -> &'static str {
        match self {
            Self::LegacyEquivalent => "legacy_equivalent",
            Self::CanonicalOnly => "canonical_only",
        }
    }
}

const MODULE_BASELINE_POLICIES: [(&str, BaselinePolicy); 5] = [
    ("accounts", BaselinePolicy::LegacyEquivalent),
    ("identity", BaselinePolicy::LegacyEquivalent),
    ("staking", BaselinePolicy::LegacyEquivalent),
    ("weather", BaselinePolicy::CanonicalOnly),
    ("invariants", BaselinePolicy::CanonicalOnly),
];

const INVARIANTS_BASELINE_INVARIANT_NAMES: [&str; 7] = [
    "INV-BAL-1: Balance Conservation",
    "INV-BAL-2: Balance Accounting Identity",
    "INV-BAL-3: Stake Lock/Release Conservation",
    "INV-SLASH-1: Slashing Bound",
    "INV-POP-1: Useful-Work Activation Non-Regression",
    "INV-RWD-1: Reward Monotonicity",
    "INV-RWD-2: Emission Cap",
];

const INVARIANTS_SEMANTICS_BASELINE: [(&str, bool); 3] = [
    ("INV-BAL-2: Balance Accounting Identity", true),
    ("INV-RWD-1: Reward Monotonicity", true),
    ("INV-RWD-2: Emission Cap", true),
];

const INVARIANTS_ERROR_BASELINE_NAME: &str = "INV-RWD-2: Emission Cap";
const INVARIANTS_ERROR_BASELINE_PASSED: bool = false;
const INVARIANTS_ERROR_BASELINE_DETAIL: &str = "Reward amount 5000 exceeds emission cap 200";

#[derive(Debug, Clone, PartialEq, Eq)]
enum InvariantsBaselineDriftError {
    RegistryCount {
        expected: usize,
        actual: usize,
    },
    RegistryEntry {
        index: usize,
        expected: &'static str,
        actual: String,
    },
    SemanticsCount {
        expected: usize,
        actual: usize,
    },
    SemanticsEntry {
        index: usize,
        expected_name: &'static str,
        expected_passed: bool,
        actual_name: String,
        actual_passed: bool,
    },
    ErrorName {
        expected: &'static str,
        actual: String,
    },
    ErrorPassed {
        expected: bool,
        actual: bool,
    },
    ErrorDetail {
        expected: &'static str,
        actual: String,
    },
}

impl fmt::Display for InvariantsBaselineDriftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegistryCount { expected, actual } => {
                write!(
                    f,
                    "invariants registry baseline count drifted: expected={expected} actual={actual}"
                )
            },
            Self::RegistryEntry {
                index,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "invariants registry baseline drifted at index={index}: expected='{expected}' actual='{actual}'"
                )
            },
            Self::SemanticsCount { expected, actual } => {
                write!(
                    f,
                    "invariants semantics baseline count drifted: expected={expected} actual={actual}"
                )
            },
            Self::SemanticsEntry {
                index,
                expected_name,
                expected_passed,
                actual_name,
                actual_passed,
            } => {
                write!(
                    f,
                    "invariants semantics baseline drifted at index={index}: expected=('{expected_name}', {expected_passed}) actual=('{actual_name}', {actual_passed})"
                )
            },
            Self::ErrorName { expected, actual } => {
                write!(
                    f,
                    "invariants error baseline name drifted: expected='{expected}' actual='{actual}'"
                )
            },
            Self::ErrorPassed { expected, actual } => {
                write!(
                    f,
                    "invariants error baseline pass/fail drifted: expected={expected} actual={actual}"
                )
            },
            Self::ErrorDetail { expected, actual } => {
                write!(
                    f,
                    "invariants error baseline detail drifted: expected='{expected}' actual='{actual}'"
                )
            },
        }
    }
}

#[derive(Clone, Copy)]
struct ParityCase {
    module: &'static str,
    class: BehaviorClass,
    baseline: BaselinePolicy,
    run: fn() -> Result<(), String>,
}

fn parity_cases() -> Vec<ParityCase> {
    vec![
        ParityCase {
            module: "accounts",
            class: BehaviorClass::Serialization,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: accounts_serialization_parity,
        },
        ParityCase {
            module: "accounts",
            class: BehaviorClass::Semantics,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: accounts_semantics_parity,
        },
        ParityCase {
            module: "accounts",
            class: BehaviorClass::ErrorBehavior,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: accounts_error_parity,
        },
        ParityCase {
            module: "identity",
            class: BehaviorClass::Serialization,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: identity_serialization_parity,
        },
        ParityCase {
            module: "identity",
            class: BehaviorClass::Semantics,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: identity_semantics_parity,
        },
        ParityCase {
            module: "identity",
            class: BehaviorClass::ErrorBehavior,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: identity_error_parity,
        },
        ParityCase {
            module: "staking",
            class: BehaviorClass::Serialization,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: staking_serialization_parity,
        },
        ParityCase {
            module: "staking",
            class: BehaviorClass::Semantics,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: staking_semantics_parity,
        },
        ParityCase {
            module: "staking",
            class: BehaviorClass::ErrorBehavior,
            baseline: BaselinePolicy::LegacyEquivalent,
            run: staking_error_parity,
        },
        ParityCase {
            module: "weather",
            class: BehaviorClass::Serialization,
            baseline: BaselinePolicy::CanonicalOnly,
            run: weather_serialization_parity,
        },
        ParityCase {
            module: "weather",
            class: BehaviorClass::Semantics,
            baseline: BaselinePolicy::CanonicalOnly,
            run: weather_semantics_parity,
        },
        ParityCase {
            module: "weather",
            class: BehaviorClass::ErrorBehavior,
            baseline: BaselinePolicy::CanonicalOnly,
            run: weather_error_parity,
        },
        ParityCase {
            module: "invariants",
            class: BehaviorClass::Serialization,
            baseline: BaselinePolicy::CanonicalOnly,
            run: invariants_serialization_parity,
        },
        ParityCase {
            module: "invariants",
            class: BehaviorClass::Semantics,
            baseline: BaselinePolicy::CanonicalOnly,
            run: invariants_semantics_parity,
        },
        ParityCase {
            module: "invariants",
            class: BehaviorClass::ErrorBehavior,
            baseline: BaselinePolicy::CanonicalOnly,
            run: invariants_error_parity,
        },
    ]
}

fn module_baseline_policy(module: &str) -> Option<BaselinePolicy> {
    MODULE_BASELINE_POLICIES
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, policy)| *policy)
}

fn ensure_matrix_coverage(cases: &[ParityCase]) -> Result<(), String> {
    let mut seen = FxHashSet::default();

    for case in cases {
        if !PARITY_CRITICAL_MODULES.contains(&case.module) {
            return Err(format!(
                "unexpected module '{}' in parity matrix",
                case.module
            ));
        }

        let expected_baseline = module_baseline_policy(case.module).ok_or_else(|| {
            format!(
                "missing explicit baseline policy for module '{}'",
                case.module
            )
        })?;
        if case.baseline != expected_baseline {
            return Err(format!(
                "baseline policy mismatch for module={} class={}: expected={} actual={}",
                case.module,
                case.class.label(),
                expected_baseline.label(),
                case.baseline.label()
            ));
        }

        let key = (case.module, case.class);
        if !seen.insert(key) {
            return Err(format!(
                "duplicate parity case for module={} class={}",
                case.module,
                case.class.label()
            ));
        }
    }

    for module in PARITY_CRITICAL_MODULES {
        for class in BEHAVIOR_CLASSES {
            if !seen.contains(&(module, class)) {
                return Err(format!(
                    "missing parity case for module={} class={}",
                    module,
                    class.label()
                ));
            }
        }
    }

    for module in PARITY_CRITICAL_MODULES {
        if module_baseline_policy(module).is_none() {
            return Err(format!(
                "parity critical module '{}' is missing an explicit baseline policy",
                module
            ));
        }
    }

    Ok(())
}

fn run_cases_for_class(class: BehaviorClass) {
    let cases = parity_cases();
    ensure_matrix_coverage(&cases).expect("parity case matrix must stay complete");

    for case in cases.iter().filter(|case| case.class == class) {
        (case.run)().unwrap_or_else(|err| {
            panic!(
                "parity case failed [module={}, class={}]: {}",
                case.module,
                class.label(),
                err
            )
        });
    }
}

#[test]
fn parity_matrix_has_full_module_and_class_coverage() {
    ensure_matrix_coverage(&parity_cases()).expect("parity matrix must be fully populated");
}

#[test]
fn baseline_policy_labels_are_stable() {
    assert_eq!(
        BaselinePolicy::LegacyEquivalent.label(),
        "legacy_equivalent"
    );
    assert_eq!(BaselinePolicy::CanonicalOnly.label(), "canonical_only");
}

#[test]
fn parity_matrix_has_explicit_baseline_policy_for_each_case() {
    let cases = parity_cases();
    ensure_matrix_coverage(&cases).expect("baseline policy must be explicitly encoded");

    for case in cases {
        let expected = module_baseline_policy(case.module)
            .expect("all parity-critical modules should have a baseline policy");
        assert_eq!(
            case.baseline,
            expected,
            "baseline policy drift for module={} class={}",
            case.module,
            case.class.label()
        );
    }
}

#[test]
fn invariants_baseline_policy_is_canonical_only() {
    let cases = parity_cases();
    for class in BEHAVIOR_CLASSES {
        let case = cases
            .iter()
            .find(|case| case.module == "invariants" && case.class == class)
            .expect("invariants module must define a parity case for every class");
        assert_eq!(
            case.baseline,
            BaselinePolicy::CanonicalOnly,
            "invariants baseline policy must remain canonical-only for class={}",
            class.label()
        );
    }
}

#[test]
fn accounts_baseline_policy_is_legacy_equivalent() {
    let cases = parity_cases();
    for class in BEHAVIOR_CLASSES {
        let case = cases
            .iter()
            .find(|case| case.module == "accounts" && case.class == class)
            .expect("accounts module must define a parity case for every class");
        assert_eq!(
            case.baseline,
            BaselinePolicy::LegacyEquivalent,
            "accounts baseline policy must remain legacy-equivalent for class={}",
            class.label()
        );
    }
}

#[test]
fn identity_baseline_policy_is_legacy_equivalent() {
    let cases = parity_cases();
    for class in BEHAVIOR_CLASSES {
        let case = cases
            .iter()
            .find(|case| case.module == "identity" && case.class == class)
            .expect("identity module must define a parity case for every class");
        assert_eq!(
            case.baseline,
            BaselinePolicy::LegacyEquivalent,
            "identity baseline policy must remain legacy-equivalent for class={}",
            class.label()
        );
    }
}

#[test]
fn staking_baseline_policy_is_legacy_equivalent() {
    let cases = parity_cases();
    for class in BEHAVIOR_CLASSES {
        let case = cases
            .iter()
            .find(|case| case.module == "staking" && case.class == class)
            .expect("staking module must define a parity case for every class");
        assert_eq!(
            case.baseline,
            BaselinePolicy::LegacyEquivalent,
            "staking baseline policy must remain legacy-equivalent for class={}",
            class.label()
        );
    }
}

#[test]
fn staking_parity_boundary_serialization_case() {
    staking_serialization_parity().expect("staking serialization parity should stay green");
}

#[test]
fn staking_parity_boundary_semantics_case() {
    staking_semantics_parity().expect("staking semantics parity should stay green");
}

#[test]
fn staking_parity_boundary_error_case() {
    staking_error_parity().expect("staking error parity should stay green");
}

#[test]
fn staking_parity_boundary_unbonding_rule_is_explicit() {
    let fixture_with_unbonding = LegacyStakingAdapterFixture::exchange_rate_sensitive();
    let fixture_without_unbonding = fixture_with_unbonding.with_legacy_unbonding_epochs(None);

    let native_with_unbonding =
        run_native_staking_flow_from_legacy_fixture(&fixture_with_unbonding)
            .expect("native flow with legacy unbonding fixture should succeed");
    let native_without_unbonding =
        run_native_staking_flow_from_legacy_fixture(&fixture_without_unbonding)
            .expect("native flow without legacy unbonding fixture should succeed");
    let shared_with_unbonding =
        run_shared_staking_flow_from_legacy_fixture(&fixture_with_unbonding)
            .expect("shared flow with legacy unbonding fixture should succeed");
    let shared_without_unbonding =
        run_shared_staking_flow_from_legacy_fixture(&fixture_without_unbonding)
            .expect("shared flow without legacy unbonding fixture should succeed");

    if native_with_unbonding.with_legacy_unbonding_boundary
        == native_without_unbonding.with_legacy_unbonding_boundary
    {
        panic!("legacy unbonding boundary marker must differ across fixtures");
    }
    if shared_with_unbonding.with_legacy_unbonding_boundary
        == shared_without_unbonding.with_legacy_unbonding_boundary
    {
        panic!("shared legacy unbonding boundary marker must differ across fixtures");
    }

    let mut native_normalized = native_with_unbonding.clone();
    native_normalized.with_legacy_unbonding_boundary = false;
    let mut shared_normalized = shared_with_unbonding.clone();
    shared_normalized.with_legacy_unbonding_boundary = false;

    assert_eq!(
        native_normalized, native_without_unbonding,
        "native canonical liquid staking flow should be invariant to legacy unbonding metadata (boundary: {LEGACY_STAKING_UNBONDING_BOUNDARY_RULE})"
    );
    assert_eq!(
        shared_normalized, shared_without_unbonding,
        "shared canonical liquid staking flow should be invariant to legacy unbonding metadata (boundary: {LEGACY_STAKING_UNBONDING_BOUNDARY_RULE})"
    );
}

#[test]
fn parity_matrix_serialization_cases() {
    run_cases_for_class(BehaviorClass::Serialization);
}

#[test]
fn parity_matrix_semantics_cases() {
    run_cases_for_class(BehaviorClass::Semantics);
}

#[test]
fn parity_matrix_error_cases() {
    run_cases_for_class(BehaviorClass::ErrorBehavior);
}

#[test]
fn accounts_parity_boundary_serialization_cases() {
    accounts_serialization_parity()
        .unwrap_or_else(|err| panic!("accounts serialization parity boundary failed: {err}"));
}

#[test]
fn accounts_parity_boundary_semantics_cases() {
    accounts_semantics_parity()
        .unwrap_or_else(|err| panic!("accounts semantics parity boundary failed: {err}"));
}

#[test]
fn accounts_parity_boundary_error_cases() {
    accounts_error_parity()
        .unwrap_or_else(|err| panic!("accounts error parity boundary failed: {err}"));
}

#[test]
fn identity_parity_boundary_serialization_case() {
    identity_serialization_parity().expect("identity serialization parity should stay green");
}

#[test]
fn identity_parity_boundary_semantics_case() {
    identity_semantics_parity().expect("identity semantics parity should stay green");
}

#[test]
fn identity_parity_boundary_error_case() {
    identity_error_parity().expect("identity error parity should stay green");
}

#[test]
fn identity_parity_boundary_excluded_capabilities_rule_is_explicit() {
    let fixture_with_legacy_capabilities = LegacyIdentityAdapterFixture::station_upgrade_sample();
    let fixture_without_legacy_capabilities = fixture_with_legacy_capabilities
        .clone()
        .with_legacy_excluded_capabilities(None, vec![]);

    let native_with_legacy_capabilities =
        run_native_identity_flow_from_legacy_fixture(&fixture_with_legacy_capabilities)
            .expect("native flow with legacy identity capabilities should succeed");
    let native_without_legacy_capabilities =
        run_native_identity_flow_from_legacy_fixture(&fixture_without_legacy_capabilities)
            .expect("native flow without legacy identity capabilities should succeed");
    let shared_with_legacy_capabilities =
        run_shared_identity_flow_from_legacy_fixture(&fixture_with_legacy_capabilities)
            .expect("shared flow with legacy identity capabilities should succeed");
    let shared_without_legacy_capabilities =
        run_shared_identity_flow_from_legacy_fixture(&fixture_without_legacy_capabilities)
            .expect("shared flow without legacy identity capabilities should succeed");

    if native_with_legacy_capabilities.with_legacy_capability_boundary
        == native_without_legacy_capabilities.with_legacy_capability_boundary
    {
        panic!("legacy identity boundary marker must differ across fixtures");
    }
    if shared_with_legacy_capabilities.with_legacy_capability_boundary
        == shared_without_legacy_capabilities.with_legacy_capability_boundary
    {
        panic!("shared legacy identity boundary marker must differ across fixtures");
    }

    let mut native_normalized = native_with_legacy_capabilities.clone();
    native_normalized.with_legacy_capability_boundary = false;
    let mut shared_normalized = shared_with_legacy_capabilities.clone();
    shared_normalized.with_legacy_capability_boundary = false;

    assert_eq!(
        native_normalized, native_without_legacy_capabilities,
        "native canonical identity flow should be invariant to excluded legacy identity capabilities (boundary: {LEGACY_IDENTITY_EXCLUDED_BOUNDARY_RULE})"
    );
    assert_eq!(
        shared_normalized, shared_without_legacy_capabilities,
        "shared canonical identity flow should be invariant to excluded legacy identity capabilities (boundary: {LEGACY_IDENTITY_EXCLUDED_BOUNDARY_RULE})"
    );
}

fn native_addr(byte: u8) -> native::common::types::Address {
    native::common::types::Address([byte; 20])
}

fn shared_addr(byte: u8) -> shared::common::types::Address {
    shared::common::types::Address([byte; 20])
}

fn native_validation_context() -> native::accounts::abstraction::ValidationContext {
    native::accounts::abstraction::ValidationContext {
        chain_id: native::common::types::ChainId::new(77),
        block_number: native::common::types::BlockNumber::new(100),
        timestamp: native::common::types::Timestamp::new(15),
    }
}

fn shared_validation_context() -> shared::accounts::abstraction::ValidationContext {
    shared::accounts::abstraction::ValidationContext {
        chain_id: shared::common::types::ChainId::new(77),
        block_number: shared::common::types::BlockNumber::new(100),
        timestamp: shared::common::types::Timestamp::new(15),
    }
}

const LEGACY_ACCOUNTS_BASELINE_SOURCE: &str =
    "blockchain-core/src/core/account_abstraction.rs (archived in #365)";

#[derive(Debug, Clone)]
struct LegacyAccountAdapterFixture {
    sender_byte: u8,
    nonce: u64,
    call_data: Vec<u8>,
    max_fee_per_gas: u128,
    valid_after: Option<u64>,
    valid_until: Option<u64>,
}

impl LegacyAccountAdapterFixture {
    fn sample(sender_byte: u8, nonce: u64, max_fee_per_gas: u128) -> Self {
        Self {
            sender_byte,
            nonce,
            call_data: vec![0xAA, sender_byte, 0xBB, nonce as u8],
            max_fee_per_gas,
            valid_after: Some(10),
            valid_until: Some(20),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LegacyAdapterSignatureMode {
    Valid,
    Empty,
    Corrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountsValidationErrorCategory {
    InvalidTimeWindow,
    InvalidSignature,
    ReplayDetected,
    NonceMismatch,
    SponsorshipNotAllowed,
    SponsoredFeeBelowOperationFee,
    SponsoredFeeAbovePolicyLimit,
}

impl AccountsValidationErrorCategory {
    fn message_fragment(self) -> &'static str {
        match self {
            Self::InvalidTimeWindow => "operation is outside validity window",
            Self::InvalidSignature => "invalid operation signature",
            Self::ReplayDetected => "replay detected for operation hash",
            Self::NonceMismatch => "nonce mismatch for account",
            Self::SponsorshipNotAllowed => "gas sponsorship is not allowed for account",
            Self::SponsoredFeeBelowOperationFee => {
                "sponsored fee is below operation fee requirement"
            },
            Self::SponsoredFeeAbovePolicyLimit => "sponsored fee exceeds policy limit",
        }
    }
}

fn classify_accounts_validation_error(error: &str) -> Option<AccountsValidationErrorCategory> {
    let categories = [
        AccountsValidationErrorCategory::InvalidTimeWindow,
        AccountsValidationErrorCategory::InvalidSignature,
        AccountsValidationErrorCategory::ReplayDetected,
        AccountsValidationErrorCategory::NonceMismatch,
        AccountsValidationErrorCategory::SponsorshipNotAllowed,
        AccountsValidationErrorCategory::SponsoredFeeBelowOperationFee,
        AccountsValidationErrorCategory::SponsoredFeeAbovePolicyLimit,
    ];
    categories
        .into_iter()
        .find(|category| error.contains(category.message_fragment()))
}

fn assert_accounts_error_contract(
    case_name: &str,
    native_err: &str,
    shared_err: &str,
    expected_category: AccountsValidationErrorCategory,
) -> Result<(), String> {
    let native_category = classify_accounts_validation_error(native_err).ok_or_else(|| {
        format!("{case_name}: native error category not classifiable: {native_err}")
    })?;
    let shared_category = classify_accounts_validation_error(shared_err).ok_or_else(|| {
        format!("{case_name}: shared error category not classifiable: {shared_err}")
    })?;
    if native_category != shared_category {
        return Err(format!(
            "{case_name}: error category mismatch native={native_category:?} shared={shared_category:?}"
        ));
    }
    if native_category != expected_category {
        return Err(format!(
            "{case_name}: error category drift expected={expected_category:?} actual={native_category:?}"
        ));
    }

    let expected_fragment = expected_category.message_fragment();
    if !native_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: native error message contract mismatch expected fragment='{expected_fragment}' actual='{native_err}'"
        ));
    }
    if !shared_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: shared error message contract mismatch expected fragment='{expected_fragment}' actual='{shared_err}'"
        ));
    }

    Ok(())
}

fn native_user_operation_projection(
    op: &native::accounts::abstraction::UserOperation,
) -> serde_json::Value {
    serde_json::json!({
        "sender": op.sender.0,
        "nonce": op.nonce,
        "call_data_hash": op.call_data_hash,
        "max_fee_per_gas": op.max_fee_per_gas.to_string(),
        "signer_public_key": op.signer_public_key,
        "signature": op.signature,
        "valid_after": op.valid_after,
        "valid_until": op.valid_until,
    })
}

fn shared_user_operation_projection(
    op: &shared::accounts::abstraction::UserOperation,
) -> serde_json::Value {
    serde_json::json!({
        "sender": op.sender.0,
        "nonce": op.nonce,
        "call_data_hash": op.call_data_hash,
        "max_fee_per_gas": op.max_fee_per_gas.to_string(),
        "signer_public_key": op.signer_public_key,
        "signature": op.signature,
        "valid_after": op.valid_after,
        "valid_until": op.valid_until,
    })
}

fn legacy_fixture_to_native_user_operation(
    fixture: &LegacyAccountAdapterFixture,
    signature_mode: LegacyAdapterSignatureMode,
    context: native::accounts::abstraction::ValidationContext,
) -> native::accounts::abstraction::UserOperation {
    let call_data_hash: [u8; 32] = native::crypto::hash::hash_bytes(&fixture.call_data).into();
    let mut rng = StdRng::seed_from_u64(10_000 + fixture.sender_byte as u64);
    let private_key = native::crypto::signature::PrivateKey::generate(&mut rng);
    let public_key = native::crypto::signature::PublicKey::from_private_key(&private_key);

    let mut op = native::accounts::abstraction::UserOperation {
        sender: native_addr(fixture.sender_byte),
        nonce: native::common::types::Nonce::new(fixture.nonce),
        call_data_hash,
        max_fee_per_gas: fixture.max_fee_per_gas,
        signer_public_key: public_key.as_bytes().to_vec(),
        signature: vec![],
        valid_after: fixture
            .valid_after
            .map(native::common::types::Timestamp::new),
        valid_until: fixture
            .valid_until
            .map(native::common::types::Timestamp::new),
        validation_gas_limit: 1_000_000,
        call_gas_limit: 0,
        paymaster: None,
    };
    let message = native::accounts::abstraction::user_operation_hash(&op, context);
    op.signature = private_key.sign(&message).as_bytes().to_vec();

    match signature_mode {
        LegacyAdapterSignatureMode::Valid => {},
        LegacyAdapterSignatureMode::Empty => op.signature.clear(),
        LegacyAdapterSignatureMode::Corrupted => {
            if let Some(first) = op.signature.first_mut() {
                *first ^= 0xFF;
            }
        },
    }

    op
}

fn legacy_fixture_to_shared_user_operation(
    fixture: &LegacyAccountAdapterFixture,
    signature_mode: LegacyAdapterSignatureMode,
    context: shared::accounts::abstraction::ValidationContext,
) -> shared::accounts::abstraction::UserOperation {
    let call_data_hash: [u8; 32] = shared::crypto::hash::hash_bytes(&fixture.call_data).into();
    let mut rng = StdRng::seed_from_u64(10_000 + fixture.sender_byte as u64);
    let private_key = shared::crypto::signature::PrivateKey::generate(&mut rng);
    let public_key = shared::crypto::signature::PublicKey::from_private_key(&private_key);

    let mut op = shared::accounts::abstraction::UserOperation {
        sender: shared_addr(fixture.sender_byte),
        nonce: shared::common::types::Nonce::new(fixture.nonce),
        call_data_hash,
        max_fee_per_gas: fixture.max_fee_per_gas,
        signer_public_key: public_key.as_bytes().to_vec(),
        signature: vec![],
        valid_after: fixture
            .valid_after
            .map(shared::common::types::Timestamp::new),
        valid_until: fixture
            .valid_until
            .map(shared::common::types::Timestamp::new),
        validation_gas_limit: 1_000_000,
        call_gas_limit: 0,
        paymaster: None,
    };
    let message = shared::accounts::abstraction::user_operation_hash(&op, context);
    op.signature = private_key.sign(&message).as_bytes().to_vec();

    match signature_mode {
        LegacyAdapterSignatureMode::Valid => {},
        LegacyAdapterSignatureMode::Empty => op.signature.clear(),
        LegacyAdapterSignatureMode::Corrupted => {
            if let Some(first) = op.signature.first_mut() {
                *first ^= 0xFF;
            }
        },
    }

    op
}

fn native_sponsorship_request(
    max_fee_per_gas: u128,
) -> native::accounts::abstraction::SponsorshipRequest {
    native::accounts::abstraction::SponsorshipRequest {
        sponsor: native_addr(99),
        max_fee_per_gas,
    }
}

fn shared_sponsorship_request(
    max_fee_per_gas: u128,
) -> shared::accounts::abstraction::SponsorshipRequest {
    shared::accounts::abstraction::SponsorshipRequest {
        sponsor: shared_addr(99),
        max_fee_per_gas,
    }
}

fn accounts_serialization_parity() -> Result<(), String> {
    let context_native = native_validation_context();
    let context_shared = shared_validation_context();

    let legacy_cases = [
        LegacyAccountAdapterFixture::sample(9, 0, 5),
        LegacyAccountAdapterFixture {
            sender_byte: 12,
            nonce: 4,
            call_data: vec![0x01, 0x02, 0xF0, 0x0D],
            max_fee_per_gas: 17,
            valid_after: Some(0),
            valid_until: None,
        },
    ];

    for fixture in legacy_cases {
        let native_op = legacy_fixture_to_native_user_operation(
            &fixture,
            LegacyAdapterSignatureMode::Valid,
            context_native,
        );
        let shared_op = legacy_fixture_to_shared_user_operation(
            &fixture,
            LegacyAdapterSignatureMode::Valid,
            context_shared,
        );

        let native_projection = native_user_operation_projection(&native_op);
        let shared_projection = shared_user_operation_projection(&shared_op);
        if native_projection != shared_projection {
            return Err(format!(
                "legacy adapter projection mismatch against shared canonical shape (source: {LEGACY_ACCOUNTS_BASELINE_SOURCE}): native={native_projection} shared={shared_projection}"
            ));
        }

        let native_hash =
            native::accounts::abstraction::user_operation_hash(&native_op, context_native);
        let shared_hash =
            shared::accounts::abstraction::user_operation_hash(&shared_op, context_shared);
        if native_hash != shared_hash {
            return Err(format!(
                "legacy adapter user operation hash mismatch against shared canonical hash (source: {LEGACY_ACCOUNTS_BASELINE_SOURCE})"
            ));
        }

        let expected_call_data_hash: [u8; 32] =
            native::crypto::hash::hash_bytes(&fixture.call_data).into();
        if native_op.call_data_hash != expected_call_data_hash {
            return Err(format!(
                "legacy adapter call_data hash projection drift for sender_byte={} nonce={}",
                fixture.sender_byte, fixture.nonce
            ));
        }

        let mut chain_context_native = context_native;
        chain_context_native.chain_id = chain_context_native
            .chain_id
            .saturating_add(native::common::types::ChainId::new(1));
        let mut chain_context_shared = context_shared;
        chain_context_shared.chain_id = chain_context_shared
            .chain_id
            .saturating_add(shared::common::types::ChainId::new(1));
        let native_alt_hash =
            native::accounts::abstraction::user_operation_hash(&native_op, chain_context_native);
        let shared_alt_hash =
            shared::accounts::abstraction::user_operation_hash(&shared_op, chain_context_shared);
        if native_alt_hash != shared_alt_hash {
            return Err("adapter hash chain-id parity mismatch".to_string());
        }
        if native_hash == native_alt_hash {
            return Err("adapter hash should vary with chain id".to_string());
        }
    }

    Ok(())
}

fn accounts_semantics_parity() -> Result<(), String> {
    let sender_native = native_addr(10);
    let sender_shared = shared_addr(10);
    let context_native = native_validation_context();
    let context_shared = shared_validation_context();

    let mut native_pipeline = native::accounts::abstraction::ValidationPipeline::default();
    native_pipeline.set_policy(
        sender_native,
        native::accounts::abstraction::SmartAccountPolicy {
            allow_gas_sponsorship: true,
            max_sponsored_fee_per_op: 10,
        },
    );
    let mut shared_pipeline = shared::accounts::abstraction::ValidationPipeline::default();
    shared_pipeline.set_policy(
        sender_shared,
        shared::accounts::abstraction::SmartAccountPolicy {
            allow_gas_sponsorship: true,
            max_sponsored_fee_per_op: 10,
        },
    );

    let op0_fixture = LegacyAccountAdapterFixture::sample(10, 0, 5);
    let native_op0 = legacy_fixture_to_native_user_operation(
        &op0_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_native,
    );
    let shared_op0 = legacy_fixture_to_shared_user_operation(
        &op0_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_shared,
    );

    let native_outcome = native_pipeline
        .validate_and_apply(
            &native_op0,
            context_native,
            Some(native_sponsorship_request(8)),
        )
        .map_err(|err| format!("native acceptance failed: {err}"))?;
    let shared_outcome = shared_pipeline
        .validate_and_apply(
            &shared_op0,
            context_shared,
            Some(shared_sponsorship_request(8)),
        )
        .map_err(|err| format!("shared acceptance failed: {err}"))?;
    if native_outcome.accepted != shared_outcome.accepted {
        return Err("acceptance mismatch between native and shared".to_string());
    }

    let native_nonce = native_pipeline.expected_nonce(&sender_native);
    let shared_nonce = shared_pipeline.expected_nonce(&sender_shared);
    if native_nonce != 1 || shared_nonce != 1 {
        return Err(format!(
            "nonce progression mismatch after first adapted op: native={native_nonce} shared={shared_nonce}"
        ));
    }

    let native_replay_err = native_pipeline
        .validate_and_apply(
            &native_op0,
            context_native,
            Some(native_sponsorship_request(8)),
        )
        .expect_err("native replay should fail")
        .to_string();
    let shared_replay_err = shared_pipeline
        .validate_and_apply(
            &shared_op0,
            context_shared,
            Some(shared_sponsorship_request(8)),
        )
        .expect_err("shared replay should fail")
        .to_string();
    assert_accounts_error_contract(
        "accounts semantics parity replay rejection",
        &native_replay_err,
        &shared_replay_err,
        AccountsValidationErrorCategory::ReplayDetected,
    )?;

    let op1_fixture = LegacyAccountAdapterFixture::sample(10, 1, 5);
    let native_op1 = legacy_fixture_to_native_user_operation(
        &op1_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_native,
    );
    let shared_op1 = legacy_fixture_to_shared_user_operation(
        &op1_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_shared,
    );
    native_pipeline
        .validate_and_apply(&native_op1, context_native, None)
        .map_err(|err| format!("native follow-up op should succeed: {err}"))?;
    shared_pipeline
        .validate_and_apply(&shared_op1, context_shared, None)
        .map_err(|err| format!("shared follow-up op should succeed: {err}"))?;
    if native_pipeline.expected_nonce(&sender_native) != 2
        || shared_pipeline.expected_nonce(&sender_shared) != 2
    {
        return Err("nonce progression drift after second adapted op".to_string());
    }

    let op2_fixture = LegacyAccountAdapterFixture::sample(10, 2, 5);
    let native_bad_sig = legacy_fixture_to_native_user_operation(
        &op2_fixture,
        LegacyAdapterSignatureMode::Corrupted,
        context_native,
    );
    let shared_bad_sig = legacy_fixture_to_shared_user_operation(
        &op2_fixture,
        LegacyAdapterSignatureMode::Corrupted,
        context_shared,
    );
    let native_bad_sig_err = native_pipeline
        .validate_and_apply(&native_bad_sig, context_native, None)
        .expect_err("native should reject corrupted signature")
        .to_string();
    let shared_bad_sig_err = shared_pipeline
        .validate_and_apply(&shared_bad_sig, context_shared, None)
        .expect_err("shared should reject corrupted signature")
        .to_string();
    assert_accounts_error_contract(
        "accounts semantics parity signature verification flow",
        &native_bad_sig_err,
        &shared_bad_sig_err,
        AccountsValidationErrorCategory::InvalidSignature,
    )?;
    if native_pipeline.expected_nonce(&sender_native) != 2
        || shared_pipeline.expected_nonce(&sender_shared) != 2
    {
        return Err("nonce should not advance after signature rejection".to_string());
    }

    Ok(())
}

fn accounts_error_parity() -> Result<(), String> {
    let context_native = native_validation_context();
    let context_shared = shared_validation_context();

    let invalid_time_fixture = LegacyAccountAdapterFixture {
        valid_after: Some(25),
        ..LegacyAccountAdapterFixture::sample(21, 0, 5)
    };
    let native_invalid_time = legacy_fixture_to_native_user_operation(
        &invalid_time_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_native,
    );
    let shared_invalid_time = legacy_fixture_to_shared_user_operation(
        &invalid_time_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_shared,
    );
    let native_invalid_time_err = native::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&native_invalid_time, context_native, None)
        .expect_err("native should reject invalid time window")
        .to_string();
    let shared_invalid_time_err = shared::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&shared_invalid_time, context_shared, None)
        .expect_err("shared should reject invalid time window")
        .to_string();
    assert_accounts_error_contract(
        "accounts error parity invalid time window",
        &native_invalid_time_err,
        &shared_invalid_time_err,
        AccountsValidationErrorCategory::InvalidTimeWindow,
    )?;

    let nonce_mismatch_fixture = LegacyAccountAdapterFixture::sample(22, 4, 5);
    let native_nonce_mismatch = legacy_fixture_to_native_user_operation(
        &nonce_mismatch_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_native,
    );
    let shared_nonce_mismatch = legacy_fixture_to_shared_user_operation(
        &nonce_mismatch_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_shared,
    );
    let native_nonce_err = native::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&native_nonce_mismatch, context_native, None)
        .expect_err("native should reject nonce mismatch")
        .to_string();
    let shared_nonce_err = shared::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&shared_nonce_mismatch, context_shared, None)
        .expect_err("shared should reject nonce mismatch")
        .to_string();
    assert_accounts_error_contract(
        "accounts error parity nonce mismatch",
        &native_nonce_err,
        &shared_nonce_err,
        AccountsValidationErrorCategory::NonceMismatch,
    )?;

    let sponsorship_fixture = LegacyAccountAdapterFixture::sample(23, 0, 5);
    let native_sponsorship = legacy_fixture_to_native_user_operation(
        &sponsorship_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_native,
    );
    let shared_sponsorship = legacy_fixture_to_shared_user_operation(
        &sponsorship_fixture,
        LegacyAdapterSignatureMode::Valid,
        context_shared,
    );

    let native_sponsorship_not_allowed_err =
        native::accounts::abstraction::ValidationPipeline::default()
            .validate_and_apply(
                &native_sponsorship,
                context_native,
                Some(native_sponsorship_request(5)),
            )
            .expect_err("native should reject sponsorship when policy disallows")
            .to_string();
    let shared_sponsorship_not_allowed_err =
        shared::accounts::abstraction::ValidationPipeline::default()
            .validate_and_apply(
                &shared_sponsorship,
                context_shared,
                Some(shared_sponsorship_request(5)),
            )
            .expect_err("shared should reject sponsorship when policy disallows")
            .to_string();
    assert_accounts_error_contract(
        "accounts error parity sponsorship not allowed",
        &native_sponsorship_not_allowed_err,
        &shared_sponsorship_not_allowed_err,
        AccountsValidationErrorCategory::SponsorshipNotAllowed,
    )?;

    let sender_native = native_addr(sponsorship_fixture.sender_byte);
    let sender_shared = shared_addr(sponsorship_fixture.sender_byte);
    let mut native_pipeline_fee_bounds =
        native::accounts::abstraction::ValidationPipeline::default();
    native_pipeline_fee_bounds.set_policy(
        sender_native,
        native::accounts::abstraction::SmartAccountPolicy {
            allow_gas_sponsorship: true,
            max_sponsored_fee_per_op: 6,
        },
    );
    let mut shared_pipeline_fee_bounds =
        shared::accounts::abstraction::ValidationPipeline::default();
    shared_pipeline_fee_bounds.set_policy(
        sender_shared,
        shared::accounts::abstraction::SmartAccountPolicy {
            allow_gas_sponsorship: true,
            max_sponsored_fee_per_op: 6,
        },
    );
    let native_below_err = native_pipeline_fee_bounds
        .validate_and_apply(
            &native_sponsorship,
            context_native,
            Some(native_sponsorship_request(4)),
        )
        .expect_err("native should reject sponsored fee below op requirement")
        .to_string();
    let shared_below_err = shared_pipeline_fee_bounds
        .validate_and_apply(
            &shared_sponsorship,
            context_shared,
            Some(shared_sponsorship_request(4)),
        )
        .expect_err("shared should reject sponsored fee below op requirement")
        .to_string();
    assert_accounts_error_contract(
        "accounts error parity sponsored fee below operation fee",
        &native_below_err,
        &shared_below_err,
        AccountsValidationErrorCategory::SponsoredFeeBelowOperationFee,
    )?;

    let native_above_err = native_pipeline_fee_bounds
        .validate_and_apply(
            &native_sponsorship,
            context_native,
            Some(native_sponsorship_request(7)),
        )
        .expect_err("native should reject sponsored fee above policy limit")
        .to_string();
    let shared_above_err = shared_pipeline_fee_bounds
        .validate_and_apply(
            &shared_sponsorship,
            context_shared,
            Some(shared_sponsorship_request(7)),
        )
        .expect_err("shared should reject sponsored fee above policy limit")
        .to_string();
    assert_accounts_error_contract(
        "accounts error parity sponsored fee above policy",
        &native_above_err,
        &shared_above_err,
        AccountsValidationErrorCategory::SponsoredFeeAbovePolicyLimit,
    )?;

    let invalid_signature_fixture = LegacyAccountAdapterFixture::sample(24, 0, 5);
    let native_invalid_sig = legacy_fixture_to_native_user_operation(
        &invalid_signature_fixture,
        LegacyAdapterSignatureMode::Empty,
        context_native,
    );
    let shared_invalid_sig = legacy_fixture_to_shared_user_operation(
        &invalid_signature_fixture,
        LegacyAdapterSignatureMode::Empty,
        context_shared,
    );
    let native_invalid_sig_err = native::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&native_invalid_sig, context_native, None)
        .expect_err("native should reject empty signature")
        .to_string();
    let shared_invalid_sig_err = shared::accounts::abstraction::ValidationPipeline::default()
        .validate_and_apply(&shared_invalid_sig, context_shared, None)
        .expect_err("shared should reject empty signature")
        .to_string();
    assert_accounts_error_contract(
        "accounts error parity invalid signature",
        &native_invalid_sig_err,
        &shared_invalid_sig_err,
        AccountsValidationErrorCategory::InvalidSignature,
    )?;

    Ok(())
}

fn native_verification_level(level: native::identity::registry::VerificationLevel) -> &'static str {
    match level {
        native::identity::registry::VerificationLevel::Basic => "basic",
        native::identity::registry::VerificationLevel::KycVerified => "kyc_verified",
        native::identity::registry::VerificationLevel::StationVerified => "station_verified",
        native::identity::registry::VerificationLevel::Institutional => "institutional",
    }
}

fn shared_verification_level(level: shared::identity::registry::VerificationLevel) -> &'static str {
    match level {
        shared::identity::registry::VerificationLevel::Basic => "basic",
        shared::identity::registry::VerificationLevel::KycVerified => "kyc_verified",
        shared::identity::registry::VerificationLevel::StationVerified => "station_verified",
        shared::identity::registry::VerificationLevel::Institutional => "institutional",
    }
}

const LEGACY_IDENTITY_BASELINE_SOURCE: &str = "blockchain-core/src/core/identity/{did,naming_service,verifiable_credentials}.rs (archived fixture shape in parity_shared.rs; source files not present in-repo)";
const LEGACY_IDENTITY_EXCLUDED_BOUNDARY_RULE: &str = "legacy DID document resolution, naming ownership proofs, and verifiable-credential revocation semantics are excluded from canonical identity-registry parity scope";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyIdentityCredentialTier {
    Basic,
    Kyc,
    Station,
    Institutional,
}

impl LegacyIdentityCredentialTier {
    fn label(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Kyc => "kyc_verified",
            Self::Station => "station_verified",
            Self::Institutional => "institutional",
        }
    }
}

#[derive(Debug, Clone)]
struct LegacyIdentityAdapterFixture {
    address_byte: u8,
    did_uri: String,
    naming_handle: Option<String>,
    credential_tier: LegacyIdentityCredentialTier,
    updated_at: u64,
    transition_tier: LegacyIdentityCredentialTier,
    transition_updated_at: u64,
    legacy_revoked_credential_epoch: Option<u64>,
    legacy_alias_records: Vec<String>,
}

impl LegacyIdentityAdapterFixture {
    fn station_upgrade_sample() -> Self {
        Self {
            address_byte: 6,
            did_uri: "did:wrf:station-6".to_string(),
            naming_handle: Some("station-6".to_string()),
            credential_tier: LegacyIdentityCredentialTier::Kyc,
            updated_at: 100,
            transition_tier: LegacyIdentityCredentialTier::Station,
            transition_updated_at: 200,
            legacy_revoked_credential_epoch: Some(333),
            legacy_alias_records: vec![
                "station-6.legacy.wrf".to_string(),
                "did:key:zStation6".to_string(),
            ],
        }
    }

    fn with_legacy_excluded_capabilities(
        mut self,
        legacy_revoked_credential_epoch: Option<u64>,
        legacy_alias_records: Vec<String>,
    ) -> Self {
        self.legacy_revoked_credential_epoch = legacy_revoked_credential_epoch;
        self.legacy_alias_records = legacy_alias_records;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentityFlowOutcome {
    contributor_id: String,
    verification_level: String,
    updated_at: u64,
    station_verified: bool,
    with_legacy_capability_boundary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityErrorCategory {
    AlreadyRegistered,
    NotFound,
}

impl IdentityErrorCategory {
    fn message_fragment(self) -> &'static str {
        match self {
            Self::AlreadyRegistered => "identity already exists for address",
            Self::NotFound => "identity not found for address",
        }
    }
}

fn classify_identity_error(error: &str) -> Option<IdentityErrorCategory> {
    let categories = [
        IdentityErrorCategory::AlreadyRegistered,
        IdentityErrorCategory::NotFound,
    ];
    categories
        .into_iter()
        .find(|category| error.contains(category.message_fragment()))
}

fn assert_identity_error_contract(
    case_name: &str,
    native_err: &str,
    shared_err: &str,
    expected_category: IdentityErrorCategory,
) -> Result<(), String> {
    let native_category = classify_identity_error(native_err).ok_or_else(|| {
        format!("{case_name}: native error category not classifiable: {native_err}")
    })?;
    let shared_category = classify_identity_error(shared_err).ok_or_else(|| {
        format!("{case_name}: shared error category not classifiable: {shared_err}")
    })?;

    if native_category != shared_category {
        return Err(format!(
            "{case_name}: error category mismatch native={native_category:?} shared={shared_category:?}"
        ));
    }
    if native_category != expected_category {
        return Err(format!(
            "{case_name}: error category drift expected={expected_category:?} actual={native_category:?}"
        ));
    }

    let expected_fragment = expected_category.message_fragment();
    if !native_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: native error message contract mismatch expected fragment='{expected_fragment}' actual='{native_err}'"
        ));
    }
    if !shared_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: shared error message contract mismatch expected fragment='{expected_fragment}' actual='{shared_err}'"
        ));
    }

    Ok(())
}

fn legacy_identity_tier_to_native(
    tier: LegacyIdentityCredentialTier,
) -> native::identity::registry::VerificationLevel {
    match tier {
        LegacyIdentityCredentialTier::Basic => native::identity::registry::VerificationLevel::Basic,
        LegacyIdentityCredentialTier::Kyc => {
            native::identity::registry::VerificationLevel::KycVerified
        },
        LegacyIdentityCredentialTier::Station => {
            native::identity::registry::VerificationLevel::StationVerified
        },
        LegacyIdentityCredentialTier::Institutional => {
            native::identity::registry::VerificationLevel::Institutional
        },
    }
}

fn legacy_identity_tier_to_shared(
    tier: LegacyIdentityCredentialTier,
) -> shared::identity::registry::VerificationLevel {
    match tier {
        LegacyIdentityCredentialTier::Basic => shared::identity::registry::VerificationLevel::Basic,
        LegacyIdentityCredentialTier::Kyc => {
            shared::identity::registry::VerificationLevel::KycVerified
        },
        LegacyIdentityCredentialTier::Station => {
            shared::identity::registry::VerificationLevel::StationVerified
        },
        LegacyIdentityCredentialTier::Institutional => {
            shared::identity::registry::VerificationLevel::Institutional
        },
    }
}

fn legacy_identity_contributor_id(fixture: &LegacyIdentityAdapterFixture) -> String {
    fixture
        .naming_handle
        .clone()
        .unwrap_or_else(|| fixture.did_uri.clone())
}

fn legacy_identity_has_excluded_capabilities(fixture: &LegacyIdentityAdapterFixture) -> bool {
    fixture.legacy_revoked_credential_epoch.is_some() || !fixture.legacy_alias_records.is_empty()
}

fn legacy_fixture_to_native_identity(
    fixture: &LegacyIdentityAdapterFixture,
) -> native::identity::registry::ContributorIdentity {
    native::identity::registry::ContributorIdentity {
        contributor_id: legacy_identity_contributor_id(fixture),
        address: native_addr(fixture.address_byte),
        verification_level: legacy_identity_tier_to_native(fixture.credential_tier),
        updated_at: native::common::types::Timestamp::new(fixture.updated_at),
    }
}

fn legacy_fixture_to_shared_identity(
    fixture: &LegacyIdentityAdapterFixture,
) -> shared::identity::registry::ContributorIdentity {
    shared::identity::registry::ContributorIdentity {
        contributor_id: legacy_identity_contributor_id(fixture),
        address: shared_addr(fixture.address_byte),
        verification_level: legacy_identity_tier_to_shared(fixture.credential_tier),
        updated_at: shared::common::types::Timestamp::new(fixture.updated_at),
    }
}

fn native_identity_projection(
    identity: &native::identity::registry::ContributorIdentity,
) -> serde_json::Value {
    serde_json::json!({
        "contributor_id": identity.contributor_id,
        "address": identity.address.0,
        "verification_level": native_verification_level(identity.verification_level),
        "updated_at": identity.updated_at,
    })
}

fn shared_identity_projection(
    identity: &shared::identity::registry::ContributorIdentity,
) -> serde_json::Value {
    serde_json::json!({
        "contributor_id": identity.contributor_id,
        "address": identity.address.0,
        "verification_level": shared_verification_level(identity.verification_level),
        "updated_at": identity.updated_at,
    })
}

fn identity_projection_repr(snapshot: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(snapshot)
        .map_err(|err| format!("identity projection encode failed: {err}"))
}

fn run_native_identity_flow_from_legacy_fixture(
    fixture: &LegacyIdentityAdapterFixture,
) -> Result<IdentityFlowOutcome, String> {
    let mut registry = native::identity::registry::IdentityRegistry::default();
    let initial_identity = legacy_fixture_to_native_identity(fixture);
    let address = initial_identity.address;
    registry
        .register(initial_identity)
        .map_err(|err| format!("native adapted register failed: {err}"))?;
    registry
        .update_verification_level(
            address,
            legacy_identity_tier_to_native(fixture.transition_tier),
            native::common::types::Timestamp::new(fixture.transition_updated_at),
        )
        .map_err(|err| format!("native adapted verification update failed: {err}"))?;
    let final_identity = registry
        .get(&address)
        .ok_or_else(|| "native identity missing after adapted transition".to_string())?;

    Ok(IdentityFlowOutcome {
        contributor_id: final_identity.contributor_id.clone(),
        verification_level: native_verification_level(final_identity.verification_level)
            .to_string(),
        updated_at: final_identity.updated_at.into_inner(),
        station_verified: registry.is_station_verified(&address),
        with_legacy_capability_boundary: legacy_identity_has_excluded_capabilities(fixture),
    })
}

fn run_shared_identity_flow_from_legacy_fixture(
    fixture: &LegacyIdentityAdapterFixture,
) -> Result<IdentityFlowOutcome, String> {
    let mut registry = shared::identity::registry::IdentityRegistry::default();
    let initial_identity = legacy_fixture_to_shared_identity(fixture);
    let address = initial_identity.address;
    registry
        .register(initial_identity)
        .map_err(|err| format!("shared adapted register failed: {err}"))?;
    registry
        .update_verification_level(
            address,
            legacy_identity_tier_to_shared(fixture.transition_tier),
            shared::common::types::Timestamp::new(fixture.transition_updated_at),
        )
        .map_err(|err| format!("shared adapted verification update failed: {err}"))?;
    let final_identity = registry
        .get(&address)
        .ok_or_else(|| "shared identity missing after adapted transition".to_string())?;

    Ok(IdentityFlowOutcome {
        contributor_id: final_identity.contributor_id.clone(),
        verification_level: shared_verification_level(final_identity.verification_level)
            .to_string(),
        updated_at: final_identity.updated_at.into_inner(),
        station_verified: registry.is_station_verified(&address),
        with_legacy_capability_boundary: legacy_identity_has_excluded_capabilities(fixture),
    })
}

fn identity_serialization_parity() -> Result<(), String> {
    let fixtures = [
        LegacyIdentityAdapterFixture::station_upgrade_sample(),
        LegacyIdentityAdapterFixture {
            address_byte: 15,
            did_uri: "did:wrf:institution-15".to_string(),
            naming_handle: None,
            credential_tier: LegacyIdentityCredentialTier::Basic,
            updated_at: 1_700_000_222,
            transition_tier: LegacyIdentityCredentialTier::Institutional,
            transition_updated_at: 1_700_000_333,
            legacy_revoked_credential_epoch: None,
            legacy_alias_records: vec![],
        },
    ];

    for fixture in fixtures {
        let native_identity = legacy_fixture_to_native_identity(&fixture);
        let shared_identity = legacy_fixture_to_shared_identity(&fixture);

        let native_snapshot = native_identity_projection(&native_identity);
        let shared_snapshot = shared_identity_projection(&shared_identity);
        if native_snapshot != shared_snapshot {
            return Err(format!(
                "legacy identity adapter snapshot mismatch (source: {LEGACY_IDENTITY_BASELINE_SOURCE}): native={native_snapshot} shared={shared_snapshot}"
            ));
        }

        let native_repr = identity_projection_repr(&native_snapshot)?;
        let shared_repr = identity_projection_repr(&shared_snapshot)?;
        if native_repr != shared_repr {
            return Err(format!(
                "legacy identity deterministic projection mismatch (source: {LEGACY_IDENTITY_BASELINE_SOURCE}): native='{native_repr}' shared='{shared_repr}'"
            ));
        }

        let expected_contributor_id = legacy_identity_contributor_id(&fixture);
        if native_identity.contributor_id != expected_contributor_id
            || shared_identity.contributor_id != expected_contributor_id
        {
            return Err(format!(
                "legacy identity contributor_id mapping drift (source: {LEGACY_IDENTITY_BASELINE_SOURCE})"
            ));
        }

        let expected_verification_label = fixture.credential_tier.label();
        if native_verification_level(native_identity.verification_level)
            != expected_verification_label
            || shared_verification_level(shared_identity.verification_level)
                != expected_verification_label
        {
            return Err(format!(
                "legacy identity verification-level mapping drift (source: {LEGACY_IDENTITY_BASELINE_SOURCE}): expected={expected_verification_label} native={} shared={}",
                native_verification_level(native_identity.verification_level),
                shared_verification_level(shared_identity.verification_level),
            ));
        }
    }

    Ok(())
}

fn identity_semantics_parity() -> Result<(), String> {
    let fixture = LegacyIdentityAdapterFixture::station_upgrade_sample();
    let native_outcome = run_native_identity_flow_from_legacy_fixture(&fixture)?;
    let shared_outcome = run_shared_identity_flow_from_legacy_fixture(&fixture)?;
    if native_outcome != shared_outcome {
        return Err(format!(
            "legacy identity adapter semantics mismatch (source: {LEGACY_IDENTITY_BASELINE_SOURCE}): native={native_outcome:?} shared={shared_outcome:?}"
        ));
    }

    let expected = IdentityFlowOutcome {
        contributor_id: "station-6".to_string(),
        verification_level: "station_verified".to_string(),
        updated_at: 200,
        station_verified: true,
        with_legacy_capability_boundary: true,
    };
    if native_outcome != expected {
        return Err(format!(
            "identity semantics drift from expected registration/verification transition: expected={expected:?} actual={native_outcome:?}"
        ));
    }

    let fixture_without_legacy_capabilities =
        fixture.with_legacy_excluded_capabilities(None, vec![]);
    let native_without_legacy_capabilities =
        run_native_identity_flow_from_legacy_fixture(&fixture_without_legacy_capabilities)?;
    let shared_without_legacy_capabilities =
        run_shared_identity_flow_from_legacy_fixture(&fixture_without_legacy_capabilities)?;
    let mut expected_without_legacy_capabilities = expected.clone();
    expected_without_legacy_capabilities.with_legacy_capability_boundary = false;

    if native_without_legacy_capabilities != expected_without_legacy_capabilities
        || shared_without_legacy_capabilities != expected_without_legacy_capabilities
    {
        return Err(format!(
            "legacy identity excluded-boundary drifted (boundary: {LEGACY_IDENTITY_EXCLUDED_BOUNDARY_RULE})"
        ));
    }

    Ok(())
}

fn identity_error_parity() -> Result<(), String> {
    let fixture = LegacyIdentityAdapterFixture::station_upgrade_sample();
    let mut native_registry = native::identity::registry::IdentityRegistry::default();
    let mut shared_registry = shared::identity::registry::IdentityRegistry::default();

    let native_identity = legacy_fixture_to_native_identity(&fixture);
    let shared_identity = legacy_fixture_to_shared_identity(&fixture);
    native_registry
        .register(native_identity.clone())
        .map_err(|err| format!("native initial register failed: {err}"))?;
    shared_registry
        .register(shared_identity.clone())
        .map_err(|err| format!("shared initial register failed: {err}"))?;

    let conflicting_fixture = LegacyIdentityAdapterFixture {
        did_uri: "did:wrf:station-6-conflict".to_string(),
        naming_handle: Some("station-6-conflict".to_string()),
        credential_tier: LegacyIdentityCredentialTier::Institutional,
        updated_at: 150,
        transition_tier: LegacyIdentityCredentialTier::Institutional,
        transition_updated_at: 250,
        ..fixture.clone()
    };
    let native_conflicting_err = native_registry
        .register(legacy_fixture_to_native_identity(&conflicting_fixture))
        .expect_err("native conflicting registration should fail")
        .to_string();
    let shared_conflicting_err = shared_registry
        .register(legacy_fixture_to_shared_identity(&conflicting_fixture))
        .expect_err("shared conflicting registration should fail")
        .to_string();
    assert_identity_error_contract(
        "identity error parity conflicting registration",
        &native_conflicting_err,
        &shared_conflicting_err,
        IdentityErrorCategory::AlreadyRegistered,
    )?;

    let missing_native_address = native_addr(fixture.address_byte.saturating_add(1));
    let missing_shared_address = shared_addr(fixture.address_byte.saturating_add(1));
    let native_missing_err = native_registry
        .update_verification_level(
            missing_native_address,
            legacy_identity_tier_to_native(LegacyIdentityCredentialTier::Station),
            native::common::types::Timestamp::new(300),
        )
        .expect_err("native missing-address update should fail")
        .to_string();
    let shared_missing_err = shared_registry
        .update_verification_level(
            missing_shared_address,
            legacy_identity_tier_to_shared(LegacyIdentityCredentialTier::Station),
            shared::common::types::Timestamp::new(300),
        )
        .expect_err("shared missing-address update should fail")
        .to_string();
    assert_identity_error_contract(
        "identity error parity invalid transition path",
        &native_missing_err,
        &shared_missing_err,
        IdentityErrorCategory::NotFound,
    )?;

    Ok(())
}

const LEGACY_STAKING_BASELINE_SOURCE: &str = "blockchain-core/src/core/liquid_staking.rs (archived fixture shape in parity_shared.rs; source file not present in-repo)";
const LEGACY_STAKING_UNBONDING_BOUNDARY_RULE: &str = "legacy unbonding queue/epoch-delay semantics are excluded from native/shared liquid staking parity scope";

#[derive(Debug, Clone, Copy)]
struct LegacyStakingAdapterFixture {
    staker_byte: u8,
    pool_wrf_locked: u128,
    pool_stwrf_supply: u128,
    wrf_to_stake: u128,
    stwrf_to_redeem: u128,
    role_weight_ppm: u32,
    legacy_unbonding_epochs: Option<u16>,
}

impl LegacyStakingAdapterFixture {
    fn exchange_rate_sensitive() -> Self {
        Self {
            staker_byte: 13,
            pool_wrf_locked: 3_000,
            pool_stwrf_supply: 2_000,
            wrf_to_stake: 750,
            stwrf_to_redeem: 400,
            role_weight_ppm: 735_000,
            legacy_unbonding_epochs: Some(7),
        }
    }

    fn with_legacy_unbonding_epochs(mut self, legacy_unbonding_epochs: Option<u16>) -> Self {
        self.legacy_unbonding_epochs = legacy_unbonding_epochs;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StakingFlowOutcome {
    stwrf_minted: u128,
    wrf_out: u128,
    total_wrf_staked: u128,
    total_stwrf_supply: u128,
    exchange_rate_num: u128,
    exchange_rate_den: u128,
    role_weight_bps: u16,
    with_legacy_unbonding_boundary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StakingErrorCategory {
    ZeroAmount,
    InsufficientLiquidity,
    MathOverflow,
}

impl StakingErrorCategory {
    fn message_fragment(self) -> &'static str {
        match self {
            Self::ZeroAmount => "amount must be greater than zero",
            Self::InsufficientLiquidity => "insufficient pool liquidity",
            Self::MathOverflow => "math overflow during staking operation",
        }
    }
}

fn classify_staking_error(error: &str) -> Option<StakingErrorCategory> {
    let categories = [
        StakingErrorCategory::ZeroAmount,
        StakingErrorCategory::InsufficientLiquidity,
        StakingErrorCategory::MathOverflow,
    ];
    categories
        .into_iter()
        .find(|category| error.contains(category.message_fragment()))
}

fn assert_staking_error_contract(
    case_name: &str,
    native_err: &str,
    shared_err: &str,
    expected_category: StakingErrorCategory,
) -> Result<(), String> {
    let native_category = classify_staking_error(native_err).ok_or_else(|| {
        format!("{case_name}: native error category not classifiable: {native_err}")
    })?;
    let shared_category = classify_staking_error(shared_err).ok_or_else(|| {
        format!("{case_name}: shared error category not classifiable: {shared_err}")
    })?;

    if native_category != shared_category {
        return Err(format!(
            "{case_name}: error category mismatch native={native_category:?} shared={shared_category:?}"
        ));
    }
    if native_category != expected_category {
        return Err(format!(
            "{case_name}: error category drift expected={expected_category:?} actual={native_category:?}"
        ));
    }

    let expected_fragment = expected_category.message_fragment();
    if !native_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: native error message contract mismatch expected fragment='{expected_fragment}' actual='{native_err}'"
        ));
    }
    if !shared_err.contains(expected_fragment) {
        return Err(format!(
            "{case_name}: shared error message contract mismatch expected fragment='{expected_fragment}' actual='{shared_err}'"
        ));
    }

    Ok(())
}

fn legacy_role_weight_ppm_to_bps(role_weight_ppm: u32) -> u16 {
    (role_weight_ppm / 100).min(10_000) as u16
}

fn legacy_fixture_to_native_pool(
    fixture: &LegacyStakingAdapterFixture,
) -> native::staking::liquid_staking::LiquidStakingPool {
    native::staking::liquid_staking::LiquidStakingPool {
        total_wrf_staked: native::common::types::Balance::new(fixture.pool_wrf_locked),
        total_stwrf_supply: native::common::types::Balance::new(fixture.pool_stwrf_supply),
    }
}

fn legacy_fixture_to_shared_pool(
    fixture: &LegacyStakingAdapterFixture,
) -> shared::staking::liquid_staking::LiquidStakingPool {
    shared::staking::liquid_staking::LiquidStakingPool {
        total_wrf_staked: shared::common::types::Balance::new(fixture.pool_wrf_locked),
        total_stwrf_supply: shared::common::types::Balance::new(fixture.pool_stwrf_supply),
    }
}

fn legacy_fixture_to_native_deposit_args(
    fixture: &LegacyStakingAdapterFixture,
) -> (
    native::common::types::Address,
    native::common::types::Balance,
    u16,
) {
    (
        native_addr(fixture.staker_byte),
        native::common::types::Balance::new(fixture.wrf_to_stake),
        legacy_role_weight_ppm_to_bps(fixture.role_weight_ppm),
    )
}

fn legacy_fixture_to_shared_deposit_args(
    fixture: &LegacyStakingAdapterFixture,
) -> (
    shared::common::types::Address,
    shared::common::types::Balance,
    u16,
) {
    (
        shared_addr(fixture.staker_byte),
        shared::common::types::Balance::new(fixture.wrf_to_stake),
        legacy_role_weight_ppm_to_bps(fixture.role_weight_ppm),
    )
}

fn native_staking_pool_projection(
    pool: &native::staking::liquid_staking::LiquidStakingPool,
) -> serde_json::Value {
    let exchange_rate = pool.exchange_rate();
    serde_json::json!({
        "total_wrf_staked": pool.total_wrf_staked.to_string(),
        "total_stwrf_supply": pool.total_stwrf_supply.to_string(),
        "exchange_rate_num": exchange_rate.0.to_string(),
        "exchange_rate_den": exchange_rate.1.to_string(),
    })
}

fn shared_staking_pool_projection(
    pool: &shared::staking::liquid_staking::LiquidStakingPool,
) -> serde_json::Value {
    let exchange_rate = pool.exchange_rate();
    serde_json::json!({
        "total_wrf_staked": pool.total_wrf_staked.to_string(),
        "total_stwrf_supply": pool.total_stwrf_supply.to_string(),
        "exchange_rate_num": exchange_rate.0.to_string(),
        "exchange_rate_den": exchange_rate.1.to_string(),
    })
}

fn native_staking_position_projection(
    position: &native::staking::liquid_staking::StakingPosition,
) -> serde_json::Value {
    serde_json::json!({
        "owner": position.owner.0,
        "wrf_staked": position.wrf_staked.to_string(),
        "stwrf_minted": position.stwrf_minted.to_string(),
        "role_weight_bps": position.role_weight_bps,
    })
}

fn shared_staking_position_projection(
    position: &shared::staking::liquid_staking::StakingPosition,
) -> serde_json::Value {
    serde_json::json!({
        "owner": position.owner.0,
        "wrf_staked": position.wrf_staked.to_string(),
        "stwrf_minted": position.stwrf_minted.to_string(),
        "role_weight_bps": position.role_weight_bps,
    })
}

fn staking_projection_repr(snapshot: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(snapshot)
        .map_err(|err| format!("staking projection encode failed: {err}"))
}

fn run_native_staking_flow_from_legacy_fixture(
    fixture: &LegacyStakingAdapterFixture,
) -> Result<StakingFlowOutcome, String> {
    let mut pool = legacy_fixture_to_native_pool(fixture);
    let (owner, wrf_to_stake, role_weight_bps) = legacy_fixture_to_native_deposit_args(fixture);
    let position = pool
        .deposit(owner, wrf_to_stake, role_weight_bps)
        .map_err(|err| format!("native adapted deposit failed: {err}"))?;
    let wrf_out = pool
        .redeem(native::common::types::Balance::new(fixture.stwrf_to_redeem))
        .map_err(|err| format!("native adapted redeem failed: {err}"))?;
    let exchange_rate = pool.exchange_rate();

    Ok(StakingFlowOutcome {
        stwrf_minted: position.stwrf_minted.into_inner(),
        wrf_out: wrf_out.into_inner(),
        total_wrf_staked: pool.total_wrf_staked.into_inner(),
        total_stwrf_supply: pool.total_stwrf_supply.into_inner(),
        exchange_rate_num: exchange_rate.0.into_inner(),
        exchange_rate_den: exchange_rate.1.into_inner(),
        role_weight_bps: position.role_weight_bps,
        with_legacy_unbonding_boundary: fixture.legacy_unbonding_epochs.is_some(),
    })
}

fn run_shared_staking_flow_from_legacy_fixture(
    fixture: &LegacyStakingAdapterFixture,
) -> Result<StakingFlowOutcome, String> {
    let mut pool = legacy_fixture_to_shared_pool(fixture);
    let (owner, wrf_to_stake, role_weight_bps) = legacy_fixture_to_shared_deposit_args(fixture);
    let position = pool
        .deposit(owner, wrf_to_stake, role_weight_bps)
        .map_err(|err| format!("shared adapted deposit failed: {err}"))?;
    let wrf_out = pool
        .redeem(shared::common::types::Balance::new(fixture.stwrf_to_redeem))
        .map_err(|err| format!("shared adapted redeem failed: {err}"))?;
    let exchange_rate = pool.exchange_rate();

    Ok(StakingFlowOutcome {
        stwrf_minted: position.stwrf_minted.into_inner(),
        wrf_out: wrf_out.into_inner(),
        total_wrf_staked: pool.total_wrf_staked.into_inner(),
        total_stwrf_supply: pool.total_stwrf_supply.into_inner(),
        exchange_rate_num: exchange_rate.0.into_inner(),
        exchange_rate_den: exchange_rate.1.into_inner(),
        role_weight_bps: position.role_weight_bps,
        with_legacy_unbonding_boundary: fixture.legacy_unbonding_epochs.is_some(),
    })
}

fn staking_serialization_parity() -> Result<(), String> {
    let fixture = LegacyStakingAdapterFixture::exchange_rate_sensitive();
    let native_seeded_pool = legacy_fixture_to_native_pool(&fixture);
    let shared_seeded_pool = legacy_fixture_to_shared_pool(&fixture);

    let native_seeded_snapshot = native_staking_pool_projection(&native_seeded_pool);
    let shared_seeded_snapshot = shared_staking_pool_projection(&shared_seeded_pool);
    if native_seeded_snapshot != shared_seeded_snapshot {
        return Err(format!(
            "legacy staking adapter seeded state projection mismatch (source: {LEGACY_STAKING_BASELINE_SOURCE}): native={native_seeded_snapshot} shared={shared_seeded_snapshot}"
        ));
    }

    let native_seeded_repr = staking_projection_repr(&native_seeded_snapshot)?;
    let shared_seeded_repr = staking_projection_repr(&shared_seeded_snapshot)?;
    if native_seeded_repr != shared_seeded_repr {
        return Err(format!(
            "legacy staking adapter deterministic representation mismatch (source: {LEGACY_STAKING_BASELINE_SOURCE}): native='{native_seeded_repr}' shared='{shared_seeded_repr}'"
        ));
    }

    let (native_owner, native_wrf, native_role_weight_bps) =
        legacy_fixture_to_native_deposit_args(&fixture);
    let (shared_owner, shared_wrf, shared_role_weight_bps) =
        legacy_fixture_to_shared_deposit_args(&fixture);
    let native_adapter_projection = serde_json::json!({
        "owner": native_owner.0,
        "wrf_to_stake": native_wrf.to_string(),
        "role_weight_bps": native_role_weight_bps,
        "legacy_unbonding_boundary": fixture.legacy_unbonding_epochs.is_some(),
    });
    let shared_adapter_projection = serde_json::json!({
        "owner": shared_owner.0,
        "wrf_to_stake": shared_wrf.to_string(),
        "role_weight_bps": shared_role_weight_bps,
        "legacy_unbonding_boundary": fixture.legacy_unbonding_epochs.is_some(),
    });
    if native_adapter_projection != shared_adapter_projection {
        return Err(format!(
            "legacy staking adapter request projection mismatch (source: {LEGACY_STAKING_BASELINE_SOURCE}): native={native_adapter_projection} shared={shared_adapter_projection}"
        ));
    }

    let mut native_pool = legacy_fixture_to_native_pool(&fixture);
    let native_position = native_pool
        .deposit(native_owner, native_wrf, native_role_weight_bps)
        .map_err(|err| format!("native adapted deposit failed: {err}"))?;
    let mut shared_pool = legacy_fixture_to_shared_pool(&fixture);
    let shared_position = shared_pool
        .deposit(shared_owner, shared_wrf, shared_role_weight_bps)
        .map_err(|err| format!("shared adapted deposit failed: {err}"))?;

    let native_position_projection = native_staking_position_projection(&native_position);
    let shared_position_projection = shared_staking_position_projection(&shared_position);
    if native_position_projection != shared_position_projection {
        return Err(format!(
            "legacy staking adapter position projection mismatch (source: {LEGACY_STAKING_BASELINE_SOURCE}): native={native_position_projection} shared={shared_position_projection}"
        ));
    }

    let expected_role_weight_bps = legacy_role_weight_ppm_to_bps(fixture.role_weight_ppm);
    if native_position.role_weight_bps != expected_role_weight_bps
        || shared_position.role_weight_bps != expected_role_weight_bps
    {
        return Err(format!(
            "legacy role weight projection drift (source: {LEGACY_STAKING_BASELINE_SOURCE}): expected_bps={expected_role_weight_bps} native={} shared={}",
            native_position.role_weight_bps, shared_position.role_weight_bps
        ));
    }

    let native_post_snapshot = native_staking_pool_projection(&native_pool);
    let shared_post_snapshot = shared_staking_pool_projection(&shared_pool);
    if native_post_snapshot != shared_post_snapshot {
        return Err("staking post-deposit pool snapshot mismatch".to_string());
    }

    let native_post_repr = staking_projection_repr(&native_post_snapshot)?;
    let shared_post_repr = staking_projection_repr(&shared_post_snapshot)?;
    if native_post_repr != shared_post_repr {
        return Err(format!(
            "staking post-deposit deterministic representation mismatch native='{native_post_repr}' shared='{shared_post_repr}'"
        ));
    }

    Ok(())
}

fn staking_semantics_parity() -> Result<(), String> {
    let fixture = LegacyStakingAdapterFixture::exchange_rate_sensitive();
    let native_outcome = run_native_staking_flow_from_legacy_fixture(&fixture)?;
    let shared_outcome = run_shared_staking_flow_from_legacy_fixture(&fixture)?;
    if native_outcome != shared_outcome {
        return Err(format!(
            "legacy staking adapter semantics mismatch (source: {LEGACY_STAKING_BASELINE_SOURCE}): native={native_outcome:?} shared={shared_outcome:?}"
        ));
    }

    let expected = StakingFlowOutcome {
        stwrf_minted: 500,
        wrf_out: 600,
        total_wrf_staked: 3_150,
        total_stwrf_supply: 2_100,
        exchange_rate_num: 3_150,
        exchange_rate_den: 2_100,
        role_weight_bps: 7_350,
        with_legacy_unbonding_boundary: true,
    };
    if native_outcome != expected {
        return Err(format!(
            "staking semantics drift from expected exchange-rate-sensitive transition: expected={expected:?} actual={native_outcome:?}"
        ));
    }

    let fixture_without_unbonding = fixture.with_legacy_unbonding_epochs(None);
    let native_without_unbonding =
        run_native_staking_flow_from_legacy_fixture(&fixture_without_unbonding)?;
    let shared_without_unbonding =
        run_shared_staking_flow_from_legacy_fixture(&fixture_without_unbonding)?;
    let mut expected_without_unbonding = expected.clone();
    expected_without_unbonding.with_legacy_unbonding_boundary = false;

    if native_without_unbonding != expected_without_unbonding
        || shared_without_unbonding != expected_without_unbonding
    {
        return Err(format!(
            "legacy unbonding boundary drifted (boundary: {LEGACY_STAKING_UNBONDING_BOUNDARY_RULE})"
        ));
    }

    Ok(())
}

fn staking_error_parity() -> Result<(), String> {
    let fixture = LegacyStakingAdapterFixture::exchange_rate_sensitive();
    let mut native_pool = legacy_fixture_to_native_pool(&fixture);
    let mut shared_pool = legacy_fixture_to_shared_pool(&fixture);

    let native_zero_mint = native_pool
        .preview_mint(native::common::types::Balance::new(0))
        .expect_err("native preview_mint(0) should fail")
        .to_string();
    let shared_zero_mint = shared_pool
        .preview_mint(shared::common::types::Balance::new(0))
        .expect_err("shared preview_mint(0) should fail")
        .to_string();
    assert_staking_error_contract(
        "staking zero mint amount parity",
        &native_zero_mint,
        &shared_zero_mint,
        StakingErrorCategory::ZeroAmount,
    )?;

    let native_zero_redeem = native_pool
        .redeem(native::common::types::Balance::new(0))
        .expect_err("native redeem(0) should fail")
        .to_string();
    let shared_zero_redeem = shared_pool
        .redeem(shared::common::types::Balance::new(0))
        .expect_err("shared redeem(0) should fail")
        .to_string();
    assert_staking_error_contract(
        "staking zero redeem amount parity",
        &native_zero_redeem,
        &shared_zero_redeem,
        StakingErrorCategory::ZeroAmount,
    )?;

    let over_redeem = fixture.pool_stwrf_supply.saturating_add(1);
    let native_insufficient = native_pool
        .redeem(native::common::types::Balance::new(over_redeem))
        .expect_err("native redeem above supply should fail")
        .to_string();
    let shared_insufficient = shared_pool
        .redeem(shared::common::types::Balance::new(over_redeem))
        .expect_err("shared redeem above supply should fail")
        .to_string();
    assert_staking_error_contract(
        "staking insufficient liquidity parity",
        &native_insufficient,
        &shared_insufficient,
        StakingErrorCategory::InsufficientLiquidity,
    )?;

    let native_overflow_pool = native::staking::liquid_staking::LiquidStakingPool {
        total_wrf_staked: native::common::types::Balance::new(u128::MAX),
        total_stwrf_supply: native::common::types::Balance::new(2),
    };
    let shared_overflow_pool = shared::staking::liquid_staking::LiquidStakingPool {
        total_wrf_staked: shared::common::types::Balance::new(u128::MAX),
        total_stwrf_supply: shared::common::types::Balance::new(2),
    };
    let native_overflow = native_overflow_pool
        .preview_redeem(native::common::types::Balance::new(2))
        .expect_err("native overflow preview_redeem should fail")
        .to_string();
    let shared_overflow = shared_overflow_pool
        .preview_redeem(shared::common::types::Balance::new(2))
        .expect_err("shared overflow preview_redeem should fail")
        .to_string();
    assert_staking_error_contract(
        "staking math overflow parity",
        &native_overflow,
        &shared_overflow,
        StakingErrorCategory::MathOverflow,
    )?;

    Ok(())
}

fn weather_payload_v1() -> serde_json::Value {
    serde_json::json!({
        "schema_version": native::weather::schema::WEATHER_SCHEMA_V1,
        "station_id": "station-11",
        "observed_at_unix": 1_700_200_000i64,
        "observation": {
            "temperature_c": 24.1,
            "humidity": 63.0,
            "pressure_hpa": 1007.3
        }
    })
}

fn weather_serialization_parity() -> Result<(), String> {
    let payload = weather_payload_v1();
    let native_parsed =
        native::weather::schema::parse_weather_payload(&payload).map_err(|err| err.to_string())?;
    let shared_parsed =
        shared::weather::schema::parse_weather_payload(&payload).map_err(|err| err.to_string())?;

    if native_parsed != shared_parsed {
        return Err("weather payload parsing mismatch".to_string());
    }

    Ok(())
}

fn weather_semantics_parity() -> Result<(), String> {
    let native_previous = native::weather::validation::WeatherObservationPoint {
        observed_at_unix: 1_700_100_000,
        observed_at_block: 100,
        observation: native::weather::validation::WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(55.0),
            pressure_hpa: Some(1008.0),
            wind_speed_ms: Some(5.0),
            precipitation_mm: Some(0.5),
            dewpoint_c: Some(12.0),
            radio_metadata: None,
        },
    };
    let native_current = native::weather::validation::WeatherObservation {
        temperature_c: Some(20.5),
        humidity_percent: Some(56.0),
        pressure_hpa: Some(1008.2),
        wind_speed_ms: Some(5.2),
        precipitation_mm: Some(0.6),
        dewpoint_c: Some(12.1),
        radio_metadata: None,
    };
    let native_result =
        native::weather::validation::validate_weather_observation_with_temporal_context(
            &native_current,
            1_700_100_030,
            101,
            Some(&native_previous),
            &native::weather::validation::WeatherValidationPolicy::default(),
        );

    let shared_previous = shared::weather::validation::WeatherObservationPoint {
        observed_at_unix: 1_700_100_000,
        observed_at_block: 100,
        observation: shared::weather::validation::WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(55.0),
            pressure_hpa: Some(1008.0),
            wind_speed_ms: Some(5.0),
            precipitation_mm: Some(0.5),
            dewpoint_c: Some(12.0),
            radio_metadata: None,
        },
    };
    let shared_current = shared::weather::validation::WeatherObservation {
        temperature_c: Some(20.5),
        humidity_percent: Some(56.0),
        pressure_hpa: Some(1008.2),
        wind_speed_ms: Some(5.2),
        precipitation_mm: Some(0.6),
        dewpoint_c: Some(12.1),
        radio_metadata: None,
    };
    let shared_result =
        shared::weather::validation::validate_weather_observation_with_temporal_context(
            &shared_current,
            1_700_100_030,
            101,
            Some(&shared_previous),
            &shared::weather::validation::WeatherValidationPolicy::default(),
        );

    if native_result != shared_result {
        return Err("weather temporal validation mismatch".to_string());
    }

    Ok(())
}

fn weather_error_parity() -> Result<(), String> {
    let payload = serde_json::json!({
        "schema_version": "wrf.weather.observation.v999",
        "observation": {
            "temperature_c": 20.0
        }
    });

    let native_err = native::weather::schema::parse_weather_payload(&payload)
        .expect_err("native should reject unknown schema version");
    let shared_err = shared::weather::schema::parse_weather_payload(&payload)
        .expect_err("shared should reject unknown schema version");

    if native_err != shared_err {
        return Err(format!(
            "weather error mismatch native='{native_err}' shared='{shared_err}'"
        ));
    }

    Ok(())
}

fn assert_invariants_registry_baseline(
    actual_names: &[String],
) -> Result<(), InvariantsBaselineDriftError> {
    if actual_names.len() != INVARIANTS_BASELINE_INVARIANT_NAMES.len() {
        return Err(InvariantsBaselineDriftError::RegistryCount {
            expected: INVARIANTS_BASELINE_INVARIANT_NAMES.len(),
            actual: actual_names.len(),
        });
    }

    for (idx, (actual, expected)) in actual_names
        .iter()
        .zip(INVARIANTS_BASELINE_INVARIANT_NAMES.iter())
        .enumerate()
    {
        if actual != expected {
            return Err(InvariantsBaselineDriftError::RegistryEntry {
                index: idx,
                expected,
                actual: actual.clone(),
            });
        }
    }

    Ok(())
}

fn assert_invariants_semantics_baseline(
    actual_projection: &[(String, bool)],
) -> Result<(), InvariantsBaselineDriftError> {
    if actual_projection.len() != INVARIANTS_SEMANTICS_BASELINE.len() {
        return Err(InvariantsBaselineDriftError::SemanticsCount {
            expected: INVARIANTS_SEMANTICS_BASELINE.len(),
            actual: actual_projection.len(),
        });
    }

    for (idx, ((actual_name, actual_passed), (expected_name, expected_passed))) in actual_projection
        .iter()
        .zip(INVARIANTS_SEMANTICS_BASELINE.iter())
        .enumerate()
    {
        if actual_name != expected_name || actual_passed != expected_passed {
            return Err(InvariantsBaselineDriftError::SemanticsEntry {
                index: idx,
                expected_name,
                expected_passed: *expected_passed,
                actual_name: actual_name.clone(),
                actual_passed: *actual_passed,
            });
        }
    }

    Ok(())
}

fn assert_invariants_error_baseline(
    name: &str,
    passed: bool,
    detail: &str,
) -> Result<(), InvariantsBaselineDriftError> {
    if name != INVARIANTS_ERROR_BASELINE_NAME {
        return Err(InvariantsBaselineDriftError::ErrorName {
            expected: INVARIANTS_ERROR_BASELINE_NAME,
            actual: name.to_string(),
        });
    }
    if passed != INVARIANTS_ERROR_BASELINE_PASSED {
        return Err(InvariantsBaselineDriftError::ErrorPassed {
            expected: INVARIANTS_ERROR_BASELINE_PASSED,
            actual: passed,
        });
    }
    if detail != INVARIANTS_ERROR_BASELINE_DETAIL {
        return Err(InvariantsBaselineDriftError::ErrorDetail {
            expected: INVARIANTS_ERROR_BASELINE_DETAIL,
            actual: detail.to_string(),
        });
    }

    Ok(())
}

fn invariants_serialization_parity() -> Result<(), String> {
    let native_invariant_names: Vec<String> = native::invariants::all_invariants()
        .iter()
        .map(|invariant| invariant.name().to_string())
        .collect();
    let shared_invariant_names: Vec<String> = shared::invariants::all_invariants()
        .iter()
        .map(|invariant| invariant.name().to_string())
        .collect();

    if native_invariant_names != shared_invariant_names {
        return Err("invariant registry name projection mismatch".to_string());
    }
    assert_invariants_registry_baseline(&native_invariant_names)
        .map_err(|err| format!("native invariants baseline drift: {err}"))?;
    assert_invariants_registry_baseline(&shared_invariant_names)
        .map_err(|err| format!("shared invariants baseline drift: {err}"))?;

    let native_config = native::invariants::model_checker::ModelCheckConfig {
        num_accounts: 2,
        initial_balances: vec![30, 20],
        amounts: vec![1, 5],
        max_depth: 2,
        stake_required: vec![0, 0],
    };
    let shared_config = shared::invariants::model_checker::ModelCheckConfig {
        num_accounts: 2,
        initial_balances: vec![30, 20],
        amounts: vec![1, 5],
        max_depth: 2,
        stake_required: vec![0, 0],
    };

    let native_report = native::invariants::model_checker::run(
        &native_config,
        &native::invariants::all_invariants(),
    );
    let shared_report = shared::invariants::model_checker::run(
        &shared_config,
        &shared::invariants::all_invariants(),
    );

    let native_json = serde_json::to_value(&native_report)
        .map_err(|err| format!("native serialize failed: {err}"))?;
    let shared_json = serde_json::to_value(&shared_report)
        .map_err(|err| format!("shared serialize failed: {err}"))?;

    if native_json != shared_json {
        return Err("invariant model-check report mismatch".to_string());
    }
    if shared_report.max_depth_reached != 1 {
        return Err(format!(
            "invariant model-check depth baseline drifted: expected=1 actual={}",
            shared_report.max_depth_reached
        ));
    }
    if !shared_report.all_passed || !shared_report.violations.is_empty() {
        return Err("invariant model-check pass/fail baseline drifted".to_string());
    }

    Ok(())
}

fn invariants_semantics_parity() -> Result<(), String> {
    let mut native_pre = native::blockchain::state::State::new();
    native_pre.create_account(native_addr(12), native::common::types::Balance::new(100));
    let native_transition = native::blockchain::state::StateTransition::ApplyReward {
        address: native_addr(12),
        amount: native::common::types::Balance::new(50),
        reason: "parity".to_string(),
    };
    let mut native_post = native_pre.clone();
    native_post
        .apply_transition(native_transition.clone())
        .map_err(|err| format!("native apply transition failed: {err}"))?;

    let mut shared_pre = shared::blockchain::state::State::new();
    shared_pre.create_account(shared_addr(12), shared::common::types::Balance::new(100));
    let shared_transition = shared::blockchain::state::StateTransition::ApplyReward {
        address: shared_addr(12),
        amount: shared::common::types::Balance::new(50),
        reason: "parity".to_string(),
    };
    let mut shared_post = shared_pre.clone();
    shared_post
        .apply_transition(shared_transition.clone())
        .map_err(|err| format!("shared apply transition failed: {err}"))?;

    let native_results = native::invariants::check_all_invariants(
        &native::invariants::all_invariants(),
        &native_pre,
        &native_post,
        &native_transition,
    );
    let shared_results = shared::invariants::check_all_invariants(
        &shared::invariants::all_invariants(),
        &shared_pre,
        &shared_post,
        &shared_transition,
    );

    let native_projection: Vec<_> = native_results
        .iter()
        .map(|result| (result.invariant_name.to_string(), result.passed))
        .collect();
    let shared_projection: Vec<_> = shared_results
        .iter()
        .map(|result| (result.invariant_name.to_string(), result.passed))
        .collect();

    if native_projection != shared_projection {
        return Err("invariant result projection mismatch".to_string());
    }
    assert_invariants_semantics_baseline(&native_projection)
        .map_err(|err| format!("native invariants semantics baseline drift: {err}"))?;
    assert_invariants_semantics_baseline(&shared_projection)
        .map_err(|err| format!("shared invariants semantics baseline drift: {err}"))?;

    Ok(())
}

fn invariants_error_parity() -> Result<(), String> {
    let native_pre = native::blockchain::state::State::new();
    let native_post = native::blockchain::state::State::new();
    let native_transition = native::blockchain::state::StateTransition::ApplyReward {
        address: native_addr(13),
        amount: native::common::types::Balance::new(5_000),
        reason: "parity".to_string(),
    };
    let native_cap = native::invariants::rewards::EmissionCap::new(200);
    let native_result = {
        use native::invariants::ConsensusInvariant;
        native_cap
            .check(&native_pre, &native_post, &native_transition)
            .ok_or_else(|| "native emission cap returned None".to_string())?
    };

    let shared_pre = shared::blockchain::state::State::new();
    let shared_post = shared::blockchain::state::State::new();
    let shared_transition = shared::blockchain::state::StateTransition::ApplyReward {
        address: shared_addr(13),
        amount: shared::common::types::Balance::new(5_000),
        reason: "parity".to_string(),
    };
    let shared_cap = shared::invariants::rewards::EmissionCap::new(200);
    let shared_result = {
        use shared::invariants::ConsensusInvariant;
        shared_cap
            .check(&shared_pre, &shared_post, &shared_transition)
            .ok_or_else(|| "shared emission cap returned None".to_string())?
    };

    if native_result.passed != shared_result.passed
        || native_result.invariant_name != shared_result.invariant_name
        || native_result.detail != shared_result.detail
    {
        return Err("invariant emission-cap failure mismatch".to_string());
    }
    assert_invariants_error_baseline(
        native_result.invariant_name,
        native_result.passed,
        &native_result.detail,
    )
    .map_err(|err| format!("native invariants error baseline drift: {err}"))?;
    assert_invariants_error_baseline(
        shared_result.invariant_name,
        shared_result.passed,
        &shared_result.detail,
    )
    .map_err(|err| format!("shared invariants error baseline drift: {err}"))?;

    Ok(())
}

#[test]
fn invariants_registry_baseline_guard_rejects_drifted_names() {
    let mut drifted: Vec<String> = INVARIANTS_BASELINE_INVARIANT_NAMES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    drifted[2] = "INV-BAL-3: Drifted Name".to_string();

    let err = assert_invariants_registry_baseline(&drifted)
        .expect_err("registry baseline guard should reject name drift");
    assert!(matches!(
        err,
        InvariantsBaselineDriftError::RegistryEntry { index: 2, .. }
    ));
}

#[test]
fn invariants_semantics_baseline_guard_rejects_drifted_projection() {
    let mut drifted: Vec<(String, bool)> = INVARIANTS_SEMANTICS_BASELINE
        .iter()
        .map(|(name, passed)| ((*name).to_string(), *passed))
        .collect();
    drifted[1] = ("INV-RWD-1: Reward Monotonicity".to_string(), false);

    let err = assert_invariants_semantics_baseline(&drifted)
        .expect_err("semantics baseline guard should reject drift");
    assert!(matches!(
        err,
        InvariantsBaselineDriftError::SemanticsEntry { index: 1, .. }
    ));
}

#[test]
fn invariants_error_baseline_guard_rejects_drifted_detail() {
    let err = assert_invariants_error_baseline(
        INVARIANTS_ERROR_BASELINE_NAME,
        INVARIANTS_ERROR_BASELINE_PASSED,
        "Reward amount 5001 exceeds emission cap 200",
    )
    .expect_err("error baseline guard should reject detail drift");
    assert!(matches!(
        err,
        InvariantsBaselineDriftError::ErrorDetail { .. }
    ));
}
