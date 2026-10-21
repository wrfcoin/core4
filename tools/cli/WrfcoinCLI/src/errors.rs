use anyhow::Error;
use serde::Serialize;
use std::error::Error as StdError;
use std::fmt;

/// Machine token retained for compatibility with the #2880 diagnostic.
///
/// New errors should use [`CliError`] instead of embedding a token in prose.
/// It is recognized only as a reserved machine token by the compatibility
/// entry point; ordinary human-readable text never selects a specific hint.
pub const INTERNAL_ERROR_SENTINEL: &str = "WRFCOIN_CLI_INTERNAL_DEFECT";

/// An error with an explicit, stable operator-facing classification.
///
/// The message is display-only. Classification uses `code`, so changing the
/// wording, adding context, or wrapping this error cannot change its hint.
#[derive(Debug)]
pub struct CliError {
    code: CliErrorCode,
    message: String,
}

impl CliError {
    pub fn new(code: CliErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> CliErrorCode {
        self.code
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl StdError for CliError {}

/// Fail a recognized command honestly when no WRFCoin service implements it.
pub fn not_wired(command: &str, note: &str) -> Error {
    CliError::new(
        CliErrorCode::NotFound,
        format!("{command}: not wired — no WRFCoin service implements this ({note})"),
    )
    .into()
}

/// Structured error codes for agent-friendly CLI diagnostics (#282).
///
/// Each variant maps to a unique exit code, a machine-readable string code,
/// and a human-readable hint that agents can surface to users.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CliErrorCode {
    Success,
    UserError,
    ConnectionError,
    AuthError,
    RateLimitError,
    BlockchainError,
    ValidationError,
    NotFound,
    ConfigError,
    /// A defect in the CLI itself (#2880 review), carried by [`CliError`] or
    /// the legacy [`INTERNAL_ERROR_SENTINEL`] compatibility channel.
    InternalError,
}

impl CliErrorCode {
    /// Every code that can classify an error — `Success` is deliberately
    /// absent.
    ///
    /// Completeness is NOT enforced by the exhaustive match below (#2954):
    /// that match forces a newly added variant to be VISITED here, not to be
    /// ADDED to the returned array — measured: a 10th variant with the
    /// or-pattern extended and the array untouched compiles and passes every
    /// pre-#2954 test. The enforced guarantee lives in the
    /// `error_codes_contains_every_error_variant` test: each error variant
    /// must appear in this array EXACTLY ONCE (omission and duplication are
    /// both RED), and a newly added variant fails compilation inside that
    /// guard, adjacent to the driver list and the assertions that must be
    /// extended with it.
    pub const ERROR_CODES: [Self; 9] = Self::all_error_codes(Self::Success);

    const fn all_error_codes(code: Self) -> [Self; 9] {
        match code {
            Self::Success
            | Self::UserError
            | Self::ConnectionError
            | Self::AuthError
            | Self::RateLimitError
            | Self::BlockchainError
            | Self::ValidationError
            | Self::NotFound
            | Self::ConfigError
            | Self::InternalError => [
                Self::UserError,
                Self::ConnectionError,
                Self::AuthError,
                Self::RateLimitError,
                Self::BlockchainError,
                Self::ValidationError,
                Self::NotFound,
                Self::ConfigError,
                Self::InternalError,
            ],
        }
    }

    /// Process exit code (0-9).
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::UserError => 1,
            Self::ConnectionError => 2,
            Self::AuthError => 3,
            Self::RateLimitError => 4,
            Self::BlockchainError => 5,
            Self::ValidationError => 6,
            Self::NotFound => 7,
            Self::ConfigError => 8,
            Self::InternalError => 9,
        }
    }

    /// Machine-readable string code for JSON envelopes.
    pub fn code_str(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::UserError => "USER_ERROR",
            Self::ConnectionError => "CONNECTION_ERROR",
            Self::AuthError => "AUTH_ERROR",
            Self::RateLimitError => "RATE_LIMIT_ERROR",
            Self::BlockchainError => "BLOCKCHAIN_ERROR",
            Self::ValidationError => "VALIDATION_ERROR",
            Self::NotFound => "NOT_FOUND",
            Self::ConfigError => "CONFIG_ERROR",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Actionable hint for the operator/agent.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Success => "",
            Self::UserError => "Check command arguments and input",
            Self::ConnectionError => "Is the API server running? Check with: wrfcoin node status",
            Self::AuthError => "Check wallet configuration and API key",
            Self::RateLimitError => "Too many requests; retry after a delay",
            Self::BlockchainError => "Check node sync status",
            Self::ValidationError => "Check input data format and required fields",
            Self::NotFound => "Resource does not exist or has been removed",
            Self::ConfigError => "Run: wrfcoin setup init",
            Self::InternalError => {
                "This is a defect in the CLI itself, not your configuration or keys — \
                 nothing you re-derive or re-run will fix it. Report it with the full \
                 command and output."
            },
        }
    }

    /// Classify an `anyhow` error by its explicit typed identity.
    ///
    /// Unclassified errors intentionally fall back to `UserError`. Their
    /// human-readable text is not an identity and must not select an
    /// operator remediation hint.
    pub fn from_error(error: &Error) -> Self {
        error
            .downcast_ref::<CliError>()
            .map(CliError::code)
            .unwrap_or_else(|| Self::from_error_msg(&error.to_string()))
    }

    /// Compatibility entry point for callers that only have prose.
    ///
    /// It recognizes only the reserved machine token used by the already
    /// released #2880 diagnostic. All other prose is deliberately
    /// unclassified and receives the generic user-error disposition.
    pub fn from_error_msg(msg: &str) -> Self {
        if msg.contains(INTERNAL_ERROR_SENTINEL) {
            Self::InternalError
        } else {
            Self::UserError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed_error(code: CliErrorCode, message: &str) -> Error {
        CliError::new(code, message).into()
    }

    #[test]
    fn exit_codes_are_unique_and_ordered() {
        let codes = [
            CliErrorCode::Success,
            CliErrorCode::UserError,
            CliErrorCode::ConnectionError,
            CliErrorCode::AuthError,
            CliErrorCode::RateLimitError,
            CliErrorCode::BlockchainError,
            CliErrorCode::ValidationError,
            CliErrorCode::NotFound,
            CliErrorCode::ConfigError,
            CliErrorCode::InternalError,
        ];
        for (i, code) in codes.iter().enumerate() {
            assert_eq!(code.exit_code(), i as i32);
        }
        // #2954: this list shares ERROR_CODES' visit-not-completion property.
        // Tie it to ERROR_CODES (whose own completeness the
        // error_codes_contains_every_error_variant guard pins), so an
        // omission here cannot silently shrink the exit-code coverage.
        assert_eq!(codes[0], CliErrorCode::Success);
        assert_eq!(codes.len(), CliErrorCode::ERROR_CODES.len() + 1);
        for code in CliErrorCode::ERROR_CODES {
            assert!(
                codes.contains(&code),
                "exit-code order list must cover {code:?}"
            );
        }
    }

    /// The legacy #2880 token remains a stable compatibility channel, while
    /// descriptive words around it remain irrelevant to classification.
    #[test]
    fn legacy_internal_sentinel_is_the_only_prose_channel() {
        let adversarial = format!(
            "{INTERNAL_ERROR_SENTINEL}: invalid unauthorized configuration \
             network timeout blockchain profile not found"
        );
        assert_eq!(
            CliErrorCode::from_error_msg(&adversarial),
            CliErrorCode::InternalError
        );
        assert_eq!(
            CliErrorCode::from_error_msg(
                "invalid unauthorized configuration network timeout blockchain profile not found"
            ),
            CliErrorCode::UserError
        );
    }

    #[test]
    fn unclassified_prose_never_selects_a_specific_hint() {
        for prose in [
            "HTTP 404: resource not found",
            "401 Unauthorized",
            "429 rate limit exceeded",
            "validation failed: missing field",
            "config file missing",
            "connection refused",
            "blockchain sync failed",
        ] {
            assert_eq!(CliErrorCode::from_error_msg(prose), CliErrorCode::UserError);
        }
    }

    #[test]
    fn network_id_prose_does_not_select_connection_hint() {
        let error: Error = anyhow::anyhow!(
            "self-verification failed while checking network_id; no API server is involved"
        );
        assert_eq!(CliErrorCode::from_error(&error), CliErrorCode::UserError);
    }

    #[test]
    fn typed_classifier_arms_are_reachable_and_distinct() {
        let cases = [
            (CliErrorCode::UserError, "network_id appears in this prose"),
            (CliErrorCode::ConnectionError, "configuration and invalid network"),
            (CliErrorCode::AuthError, "not found and blockchain"),
            (CliErrorCode::RateLimitError, "invalid configuration"),
            (CliErrorCode::BlockchainError, "timeout and unauthorized"),
            (CliErrorCode::ValidationError, "consensus chain"),
            (CliErrorCode::NotFound, "profile connection"),
            (CliErrorCode::ConfigError, "invalid configuration"),
            (CliErrorCode::InternalError, "not a configuration defect"),
        ];

        // #2954: this list shares ERROR_CODES' visit-not-completion property.
        // Tie its key set to ERROR_CODES exactly-once, so a reachability case
        // cannot silently drop out while the suite stays green.
        assert_eq!(cases.len(), CliErrorCode::ERROR_CODES.len());
        for code in CliErrorCode::ERROR_CODES {
            assert_eq!(
                cases.iter().filter(|(c, _)| *c == code).count(),
                1,
                "reachability cases must cover {code:?} exactly once"
            );
        }

        for (expected, prose) in cases {
            let error = typed_error(expected, prose);
            assert_eq!(CliErrorCode::from_error(&error), expected);
        }

        for (index, first) in CliErrorCode::ERROR_CODES.iter().enumerate() {
            for second in CliErrorCode::ERROR_CODES.iter().skip(index + 1) {
                let first_error = typed_error(*first, "same prose: invalid network timeout");
                let second_error = typed_error(*second, "same prose: invalid network timeout");
                assert_ne!(
                    CliErrorCode::from_error(&first_error),
                    CliErrorCode::from_error(&second_error),
                    "distinct typed errors must retain distinct classifications"
                );
            }
        }
    }

    #[test]
    fn typed_identity_wins_when_prose_would_have_selected_another_arm() {
        let error = typed_error(
            CliErrorCode::ConfigError,
            "validation failed while loading configuration",
        );
        assert_eq!(CliErrorCode::from_error(&error), CliErrorCode::ConfigError);
    }

    #[test]
    fn code_str_and_hint_are_non_empty_for_errors() {
        for code in &CliErrorCode::ERROR_CODES {
            assert!(!code.code_str().is_empty());
            assert!(!code.hint().is_empty());
        }
    }

    /// #2954: ERROR_CODES completeness guard — the enforcement the doc
    /// comment on ERROR_CODES points at.
    ///
    /// The production match forces a VISIT, not COMPLETION: an incomplete or
    /// wrongly shrunk array compiles and passed every pre-#2954 test. This
    /// guard pins each variant BY IDENTITY, exactly once — one arm per
    /// variant, never or-patterns (the or-pattern is how the production match
    /// was satisfied without the array in the #2941 review experiment).
    ///
    /// When a variant is added, compilation fails HERE; extend the driver
    /// list `all` below and the array together. (Measured limit, #2954: the
    /// compiler forces only the new ARM, not the driver entry — the arm you
    /// are forced to write sits ten lines from the driver it needs, which is
    /// as far as stable Rust can push this. See the issue for the
    /// measurement.)
    #[test]
    fn error_codes_contains_every_error_variant() {
        use CliErrorCode as C;

        fn occurrences(code: C) -> usize {
            C::ERROR_CODES.iter().filter(|c| **c == code).count()
        }

        fn assert_pinned(code: C) {
            match code {
                C::Success => assert_eq!(
                    occurrences(C::Success),
                    0,
                    "Success is not an error code and must stay out"
                ),
                C::UserError => assert_eq!(occurrences(C::UserError), 1),
                C::ConnectionError => assert_eq!(occurrences(C::ConnectionError), 1),
                C::AuthError => assert_eq!(occurrences(C::AuthError), 1),
                C::RateLimitError => assert_eq!(occurrences(C::RateLimitError), 1),
                C::BlockchainError => assert_eq!(occurrences(C::BlockchainError), 1),
                C::ValidationError => assert_eq!(occurrences(C::ValidationError), 1),
                C::NotFound => assert_eq!(occurrences(C::NotFound), 1),
                C::ConfigError => assert_eq!(occurrences(C::ConfigError), 1),
                C::InternalError => assert_eq!(occurrences(C::InternalError), 1),
            }
        }

        let all = [
            C::Success,
            C::UserError,
            C::ConnectionError,
            C::AuthError,
            C::RateLimitError,
            C::BlockchainError,
            C::ValidationError,
            C::NotFound,
            C::ConfigError,
            C::InternalError,
        ];
        for code in all {
            assert_pinned(code);
        }
        // Exactly-once over every variant above plus the Success exclusion
        // means the array can contain nothing else at this length.
        assert_eq!(all.len(), C::ERROR_CODES.len() + 1);
    }
}
