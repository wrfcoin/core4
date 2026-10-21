use serde::Serialize;
use serde_json::{Value, json};
use std::cell::RefCell;

/// A single retry attempt recorded by the API client.
#[derive(Clone, Debug, Serialize)]
pub struct WhyAttempt {
    pub attempt: u32,
    pub error: String,
    pub delay_ms: u64,
}

/// A single route entry recorded by the API client.
#[derive(Clone, Debug, Serialize)]
pub struct WhyRoute {
    pub method: String,
    pub path: String,
}

/// Accumulated --why report for a single CLI invocation.
#[derive(Clone, Debug, Serialize)]
pub struct WhyReport {
    pub routes: Vec<WhyRoute>,
    pub base_url: String,
    pub retry_policy: WhyRetryPolicy,
    pub attempts: Vec<WhyAttempt>,
    pub exit_code: i32,
    pub exit_reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WhyRetryPolicy {
    pub max_attempts: u32,
    pub delay_ms: u64,
}

impl WhyReport {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            base_url: String::new(),
            retry_policy: WhyRetryPolicy {
                max_attempts: 0,
                delay_ms: 0,
            },
            attempts: Vec::new(),
            exit_code: 0,
            exit_reason: "success".to_string(),
        }
    }

    pub fn to_json(&self) -> Value {
        let routes: Vec<String> = self
            .routes
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect();
        json!({
            "routes": routes,
            "base_url": redact(&self.base_url),
            "retry_policy": {
                "max_attempts": self.retry_policy.max_attempts,
                "delay_ms": self.retry_policy.delay_ms,
            },
            "attempts": self.attempts.iter().map(|a| json!({
                "attempt": a.attempt,
                "error": redact(&a.error),
                "delay_ms": a.delay_ms,
            })).collect::<Vec<_>>(),
            "exit_code": self.exit_code,
            "exit_reason": &self.exit_reason,
        })
    }

    pub fn to_human_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for route in &self.routes {
            lines.push(format!(
                "WHY: route      = {} {}",
                route.method,
                redact(&route.path)
            ));
        }
        if !self.base_url.is_empty() {
            lines.push(format!("WHY: base_url   = {}", redact(&self.base_url)));
        }
        if self.attempts.is_empty() {
            lines.push(format!(
                "WHY: retries    = 0 of {} (no failures)",
                self.retry_policy.max_attempts.saturating_sub(1)
            ));
        } else {
            for attempt in &self.attempts {
                if attempt.delay_ms > 0 {
                    lines.push(format!(
                        "WHY: attempt {} = error: {}, waited {}ms",
                        attempt.attempt,
                        redact(&attempt.error),
                        attempt.delay_ms
                    ));
                } else {
                    lines.push(format!(
                        "WHY: attempt {} = error: {} (final)",
                        attempt.attempt,
                        redact(&attempt.error)
                    ));
                }
            }
        }
        lines.push(format!(
            "WHY: exit_code  = {} ({})",
            self.exit_code, self.exit_reason
        ));
        lines
    }
}

// ---------------------------------------------------------------------------
// Thread-local collector
// ---------------------------------------------------------------------------

thread_local! {
    static REPORT: RefCell<WhyReport> = RefCell::new(WhyReport::new());
}

/// Record a route selection (called by ApiClient when explain_mode is true).
pub fn record_route(method: &str, url: &str, base_url: &str, max_attempts: u32, delay_ms: u64) {
    REPORT.with(|r| {
        let mut report = r.borrow_mut();
        report.routes.push(WhyRoute {
            method: method.to_string(),
            path: url.to_string(),
        });
        report.base_url = base_url.to_string();
        report.retry_policy.max_attempts = max_attempts;
        report.retry_policy.delay_ms = delay_ms;
    });
}

/// Record a retry attempt (called by ApiClient when a request fails and will be retried).
pub fn record_attempt(attempt: u32, error: &str, delay_ms: u64) {
    REPORT.with(|r| {
        r.borrow_mut().attempts.push(WhyAttempt {
            attempt,
            error: error.to_string(),
            delay_ms,
        });
    });
}

/// Record the final failed attempt (no more retries).
pub fn record_final_failure(attempt: u32, error: &str) {
    REPORT.with(|r| {
        r.borrow_mut().attempts.push(WhyAttempt {
            attempt,
            error: error.to_string(),
            delay_ms: 0,
        });
    });
}

/// Set exit code and reason on the report.
pub fn set_exit(code: i32, reason: &str) {
    REPORT.with(|r| {
        let mut report = r.borrow_mut();
        report.exit_code = code;
        report.exit_reason = reason.to_string();
    });
}

/// Take the accumulated report and reset the thread-local to a fresh report.
pub fn take_report() -> WhyReport {
    REPORT.with(|r| {
        let report = r.borrow().clone();
        *r.borrow_mut() = WhyReport::new();
        report
    })
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// Redact sensitive values from a string.
///
/// Strips:
/// - Bearer tokens (`Bearer eyJ...`)
/// - JWT-like strings (three base64 segments separated by dots)
/// - Query parameters named key, token, secret, password, api_key, apikey, auth
/// - Hex strings that look like API keys (32+ hex chars)
pub fn redact(input: &str) -> String {
    use std::borrow::Cow;

    let mut result: Cow<'_, str> = Cow::Borrowed(input);

    // Redact Bearer tokens
    if let Some(idx) = result.find("Bearer ") {
        let start = idx + 7;
        let end = result[start..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map(|i| start + i)
            .unwrap_or(result.len());
        if end > start {
            let mut s = result.into_owned();
            s.replace_range(start..end, "[REDACTED]");
            result = Cow::Owned(s);
        }
    }

    // Redact JWT-like strings (eyJ...<dot>...<dot>...)
    let jwt_re_simple = |s: &str| -> Option<(usize, usize)> {
        if let Some(idx) = s.find("eyJ") {
            let token_start = idx;
            let token_end = s[idx..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .map(|i| idx + i)
                .unwrap_or(s.len());
            let candidate = &s[token_start..token_end];
            if candidate.matches('.').count() >= 2 && candidate.len() > 20 {
                return Some((token_start, token_end));
            }
        }
        None
    };
    if let Some((start, end)) = jwt_re_simple(&result) {
        let mut s = result.into_owned();
        s.replace_range(start..end, "[REDACTED]");
        result = Cow::Owned(s);
    }

    // Redact sensitive query parameters
    let sensitive_params = [
        "key=",
        "token=",
        "secret=",
        "password=",
        "api_key=",
        "apikey=",
        "auth=",
    ];
    for param in &sensitive_params {
        if let Some(idx) = result.to_lowercase().find(param) {
            let value_start = idx + param.len();
            let value_end = result[value_start..]
                .find(|c: char| c == '&' || c == ' ' || c == '"' || c == '\'')
                .map(|i| value_start + i)
                .unwrap_or(result.len());
            if value_end > value_start {
                let mut s = result.into_owned();
                s.replace_range(value_start..value_end, "[REDACTED]");
                result = Cow::Owned(s);
            }
        }
    }

    // Redact long hex strings that look like API keys/secrets (32+ chars)
    let mut s = result.into_owned();
    let mut i = 0;
    while i < s.len() {
        let hex_start = i;
        let mut hex_len = 0;
        for c in s[i..].chars() {
            if c.is_ascii_hexdigit() {
                hex_len += c.len_utf8();
            } else {
                break;
            }
        }
        if hex_len >= 32 {
            s.replace_range(hex_start..hex_start + hex_len, "[REDACTED]");
            i = hex_start + "[REDACTED]".len();
        } else {
            i += if hex_len > 0 { hex_len } else { 1 };
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig";
        let output = redact(input);
        assert!(
            !output.contains("eyJhbGciOiJIUzI1NiJ9"),
            "JWT should be redacted: {}",
            output
        );
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn redact_jwt_in_url() {
        let input = "https://api.example.com?token=eyJhbGciOiJIUzI1NiJ9.body.sig";
        let output = redact(input);
        assert!(
            !output.contains("eyJhbGciOiJIUzI1NiJ9"),
            "JWT should be redacted: {}",
            output
        );
    }

    #[test]
    fn redact_api_key_in_query() {
        let input = "https://api.example.com/path?api_key=supersecretkey123&other=value";
        let output = redact(input);
        assert!(
            !output.contains("supersecretkey123"),
            "API key should be redacted: {}",
            output
        );
        assert!(
            output.contains("other=value"),
            "Non-sensitive params preserved"
        );
    }

    #[test]
    fn redact_long_hex_secret() {
        let input = "key=a]b]0123456789abcdef0123456789abcdef rest";
        let output = redact(input);
        assert!(
            !output.contains("0123456789abcdef0123456789abcdef"),
            "Hex key should be redacted: {}",
            output
        );
    }

    #[test]
    fn redact_preserves_normal_text() {
        let input = "GET http://localhost:4001/api/network/info";
        let output = redact(input);
        assert_eq!(input, output);
    }

    #[test]
    fn redact_preserves_short_hex() {
        let input = "block hash abc123 is ok";
        let output = redact(input);
        assert_eq!(input, output);
    }

    #[test]
    fn report_to_json_success() {
        let mut report = WhyReport::new();
        report.routes.push(WhyRoute {
            method: "GET".to_string(),
            path: "http://localhost:4001/api/network/info".to_string(),
        });
        report.base_url = "http://localhost:4001/api".to_string();
        report.retry_policy = WhyRetryPolicy {
            max_attempts: 6,
            delay_ms: 2000,
        };
        let j = report.to_json();
        assert_eq!(j["exit_code"], 0);
        assert_eq!(j["exit_reason"], "success");
        assert!(j["routes"].as_array().unwrap().len() == 1);
        assert!(j["attempts"].as_array().unwrap().is_empty());
    }

    #[test]
    fn report_to_human_lines_no_failures() {
        let mut report = WhyReport::new();
        report.routes.push(WhyRoute {
            method: "GET".to_string(),
            path: "http://localhost:4001/api/network/info".to_string(),
        });
        report.base_url = "http://localhost:4001/api".to_string();
        report.retry_policy = WhyRetryPolicy {
            max_attempts: 6,
            delay_ms: 2000,
        };
        let lines = report.to_human_lines();
        assert!(lines.iter().any(|l| l.contains("route")));
        assert!(lines.iter().any(|l| l.contains("retries    = 0 of 5")));
        assert!(lines.iter().any(|l| l.contains("exit_code  = 0")));
    }

    #[test]
    fn report_to_human_lines_with_failures() {
        let mut report = WhyReport::new();
        report.routes.push(WhyRoute {
            method: "GET".to_string(),
            path: "http://localhost:4001/api/network/info".to_string(),
        });
        report.base_url = "http://localhost:4001/api".to_string();
        report.retry_policy = WhyRetryPolicy {
            max_attempts: 3,
            delay_ms: 2000,
        };
        report.attempts.push(WhyAttempt {
            attempt: 1,
            error: "connection refused".to_string(),
            delay_ms: 2000,
        });
        report.attempts.push(WhyAttempt {
            attempt: 2,
            error: "connection refused".to_string(),
            delay_ms: 0,
        });
        report.exit_code = 2;
        report.exit_reason = "network/connectivity error".to_string();

        let lines = report.to_human_lines();
        assert!(lines.iter().any(|l| l.contains("attempt 1")));
        assert!(lines.iter().any(|l| l.contains("waited 2000ms")));
        assert!(lines.iter().any(|l| l.contains("(final)")));
        assert!(lines.iter().any(|l| l.contains("exit_code  = 2")));
    }

    #[test]
    fn thread_local_take_report_resets() {
        record_route("GET", "/test", "http://localhost", 3, 1000);
        let report = take_report();
        assert_eq!(report.routes.len(), 1);

        let report2 = take_report();
        assert!(report2.routes.is_empty(), "second take should be empty");
    }
}
