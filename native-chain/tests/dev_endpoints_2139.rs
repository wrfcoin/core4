//! #2139 — dev endpoints are compiled OUT of release builds.
//!
//! Source-level pins for the shape the story requires. The binary-level proof — that a
//! built release `launch_api` contains no dev route — is `scripts/check_release_endpoints.py`,
//! which runs against the real artifact in `release-signing.yml` and in the
//! `release-endpoints` job of `deploy-gate.yml`. These tests make the source shape that
//! proof depends on impossible to regress silently:
//!
//! 1. the `dev-endpoints` feature exists and no `default` list enables it;
//! 2. every `/debug/`-style route registration in `routes/mod.rs` sits inside a
//!    `#[cfg(any(test, feature = "dev-endpoints"))]` block, and the handler module is
//!    declared under the same gate (a registration outside the gate is a handler in the
//!    release binary);
//! 3. no build profile that ships a binary names the feature;
//! 4. `RPC_SURFACE.md` classifies every dev endpoint `disabled-in-release` with the
//!    `(feature: dev-endpoints)` annotation the script keys on.

use std::fs;
use std::path::{Path, PathBuf};

const GATE: &str = "#[cfg(any(test, feature = \"dev-endpoints\"))]";
const DEV_PREFIXES: &[&str] = &["/debug/", "/trace/", "/profile/", "/profiling/", "/pprof", "/heap"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("native-chain sits under the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn is_dev_route(path: &str) -> bool {
    DEV_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

#[test]
fn dev_endpoints_feature_exists_and_is_off_by_default_2139() {
    let manifest = read("native-chain/Cargo.toml");
    assert!(
        manifest.lines().any(|l| l.trim_start().starts_with("dev-endpoints = [")),
        "native-chain/Cargo.toml must declare the `dev-endpoints` feature"
    );
    for line in manifest.lines() {
        let code = line.split('#').next().unwrap_or("");
        if code.trim_start().starts_with("default") {
            assert!(
                !code.contains("dev-endpoints"),
                "`default` must never enable dev-endpoints: {line}"
            );
        }
    }
}

#[test]
fn every_dev_route_registration_is_inside_the_cfg_gate_2139() {
    let source = read("native-chain/src/bin/launch_api/routes/mod.rs");
    assert!(
        source.contains(&format!("{GATE}\npub mod debug;")),
        "routes/mod.rs must declare `pub mod debug;` directly under {GATE}"
    );

    // Walk the file tracking brace depth. Accept either a `{ … }` block immediately
    // following the gate, or the single guarded `let scope = scope.route(...)` form used
    // to keep conditionally compiled routes inside the shared OriginGuard scope. Route
    // literals like "/nft/{id}" contain balanced braces, so counting is safe.
    let mut depth: i32 = 0;
    let mut gated_from: Option<i32> = None;
    let mut gated_statement = false;
    let mut pending_gate = false;
    let mut gated_dev_routes = 0usize;
    for (number, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == GATE {
            pending_gate = true;
            continue;
        }
        if pending_gate {
            if trimmed.starts_with('{') {
                gated_from = Some(depth);
            } else if trimmed.starts_with("let scope = scope.route(") {
                gated_statement = true;
            }
            pending_gate = false;
        }
        if let Some(start) = trimmed.find(".route(\"") {
            let rest = &trimmed[start + ".route(\"".len()..];
            let path = rest.split('"').next().unwrap_or("");
            if is_dev_route(path) {
                assert!(
                    gated_from.is_some() || gated_statement,
                    "routes/mod.rs:{}: dev route {path} is registered outside {GATE} — it would be in the release binary",
                    number + 1
                );
                gated_dev_routes += 1;
            }
        }
        let opens = trimmed.matches('{').count() as i32;
        let closes = trimmed.matches('}').count() as i32;
        depth += opens - closes;
        if let Some(start) = gated_from {
            if depth <= start {
                gated_from = None;
            }
        }
        if gated_statement && trimmed.ends_with(';') {
            gated_statement = false;
        }
    }
    assert!(gated_dev_routes >= 1, "no gated dev route found — the pin is vacuous");
}

#[test]
fn no_shipping_build_profile_enables_dev_endpoints_2139() {
    for profile in [
        "native-chain/Dockerfile",
        "native-chain/Dockerfile.producer-key-pop",
        ".github/workflows/release-signing.yml",
        "scripts/build-release.sh",
    ] {
        let text = read(profile);
        for (number, line) in text.lines().enumerate() {
            if line.contains("--features") {
                assert!(
                    !line.contains("dev-endpoints"),
                    "{profile}:{}: shipping build profile enables dev-endpoints: {line}",
                    number + 1
                );
            }
        }
    }
}

#[test]
fn surface_document_classifies_dev_endpoints_as_disabled_in_release_2139() {
    let doc = read("docs/security/RPC_SURFACE.md");
    let mut annotated = 0usize;
    for (number, line) in doc.lines().enumerate() {
        if !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // Inventory rows have seven columns (nine cells with the empty ends);
        // the shorter tables elsewhere in the document are not the registry.
        if cells.len() < 9 {
            continue;
        }
        let method = cells[1].trim_matches('`');
        let handler = cells[3];
        let classification = cells[4].trim_matches('`');
        let path = method.split(' ').nth(1).unwrap_or(method);
        let has_mark = handler.contains("(feature: dev-endpoints)");
        if is_dev_route(path) {
            assert!(
                has_mark,
                "RPC_SURFACE.md:{}: {method} is a dev route without the (feature: dev-endpoints) annotation",
                number + 1
            );
        }
        if has_mark {
            annotated += 1;
            assert_eq!(
                classification,
                "disabled-in-release",
                "RPC_SURFACE.md:{}: {method} is a dev endpoint but classified `{classification}`",
                number + 1
            );
        }
    }
    assert!(annotated >= 1, "RPC_SURFACE.md has no (feature: dev-endpoints) row");
}
