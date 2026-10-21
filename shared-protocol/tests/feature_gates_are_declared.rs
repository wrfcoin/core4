//! Every `#[cfg(feature = "…")]` in this crate must name a feature that
//! `Cargo.toml` actually declares. (core4#2975)
//!
//! WHY THIS EXISTS
//! ---------------
//! A `cfg` naming an undeclared feature is not an error, a warning, or anything
//! else Cargo tells you about. It silently evaluates false forever, so the code
//! it guards is never compiled, never type-checked and never tested — while
//! still reading, to anyone browsing the source, like a shipped capability
//! behind a flag.
//!
//! This has now happened twice in this one crate:
//!
//!   * `data_hub`  — one gate on a `reqwest` import (core4#2316)
//!   * `governance` — SIX gates covering two complete implementations,
//!     `QuadraticVoting` and `DelayedImplementation` (core4#2975)
//!
//! Fixing the second one is not the generalisation; this test is. It makes the
//! whole class impossible to reintroduce, because a new gate for an undeclared
//! feature now fails the suite instead of quietly doing nothing.
//!
//! SCOPE, stated honestly. This checks DECLARATION, not usefulness. It cannot
//! tell you a declared feature is ever built in CI, or that gated code is
//! tested — `governance` has zero tests exercising it even now. It closes
//! exactly one hole: a gate that can never be true.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Features Cargo understands without them appearing in `[features]`.
/// `default` is special-cased by Cargo; the rest are implicit.
const IMPLICIT: &[&str] = &["default"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Declared feature names from `[features]` in this crate's Cargo.toml.
///
/// Deliberately a hand-rolled scan of the one section rather than a TOML
/// dependency: this test must not be able to fail because a parser crate
/// changed, and `[features]` is a flat `name = [...]` table.
fn declared_features() -> BTreeSet<String> {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml"))
        .expect("shared-protocol/Cargo.toml must be readable");

    let mut out = BTreeSet::new();
    let mut in_features = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_features = t == "[features]";
            continue;
        }
        if !in_features || t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some((name, _)) = t.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                out.insert(name.to_string());
            }
        }
    }
    assert!(
        !out.is_empty(),
        "parsed zero features from [features] — the parser is broken, not the manifest. \
         A test that discovers nothing cannot fail, which is the bug it is here to prevent."
    );
    out
}

/// Strip line comments so that PROSE mentioning a gate is not mistaken for one.
///
/// This is load-bearing, not defensive tidiness. Both surviving mentions of
/// `data_hub` in this crate live in comments that DESCRIBE the historical
/// defect — one in Cargo.toml, one atop `src/data_hub/sources.rs`. A naive scan
/// flags those and fails on a hole that was already fixed, so the guard would
/// have to be disabled on its first run.
///
/// Cutting at `//` can in principle truncate a string literal containing those
/// characters. That would cause this test to MISS a gate, never to invent one —
/// it fails safe toward silence rather than toward a false alarm, which is the
/// right direction for a test whose alternative is being switched off.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every feature name appearing in a `feature = "…"` position.
///
/// Matches the predicate itself rather than the `#[cfg(...)]` wrapper, so it
/// covers `cfg`, `cfg_attr`, `not(...)`, `all(...)`, `any(...)` and multi-line
/// attributes without needing to model attribute syntax.
fn features_referenced_in(src: &str) -> BTreeSet<String> {
    let src = strip_line_comments(src);
    let mut out = BTreeSet::new();
    let bytes: Vec<char> = src.chars().collect();
    let needle: Vec<char> = "feature".chars().collect();

    let mut i = 0usize;
    while i + needle.len() < bytes.len() {
        if bytes[i..i + needle.len()] == needle[..] {
            let mut j = i + needle.len();
            while j < bytes.len() && bytes[j].is_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == '=' {
                j += 1;
                while j < bytes.len() && bytes[j].is_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == '"' {
                    j += 1;
                    let start = j;
                    while j < bytes.len() && bytes[j] != '"' {
                        j += 1;
                    }
                    if j < bytes.len() {
                        out.insert(bytes[start..j].iter().collect::<String>());
                    }
                }
            }
        }
        i += 1;
    }
    out
}

fn rust_sources(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_sources(&p, acc);
        } else if p.extension().is_some_and(|x| x == "rs") {
            acc.push(p);
        }
    }
}

#[test]
fn every_cfg_feature_is_declared_in_cargo_toml() {
    let declared = declared_features();
    let root = crate_root();

    let mut files = Vec::new();
    rust_sources(&root.join("src"), &mut files);
    rust_sources(&root.join("tests"), &mut files);
    assert!(
        files.len() > 10,
        "discovered only {} rust file(s) under src/ and tests/ — discovery is broken, \
         and a scan that reads nothing passes trivially",
        files.len()
    );

    let mut undeclared: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();

    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        for feat in features_referenced_in(&src) {
            seen.insert(feat.clone());
            if !declared.contains(&feat) && !IMPLICIT.contains(&feat.as_str()) {
                undeclared.push((
                    feat,
                    f.strip_prefix(&root).unwrap_or(f).display().to_string(),
                ));
            }
        }
    }

    assert!(
        !seen.is_empty(),
        "found no `feature = \"…\"` references at all across {} file(s) — the scanner is \
         broken. This crate is known to gate on `hsm` and `test-support` at minimum.",
        files.len()
    );

    if !undeclared.is_empty() {
        let mut msg = String::from(
            "cfg gate(s) name a feature that [features] does not declare.\n\n\
             Such a gate is ALWAYS FALSE: the code behind it is never compiled, never \
             type-checked and never tested, while still reading like a shipped capability. \
             Declare the feature in shared-protocol/Cargo.toml, or delete the gate.\n\n",
        );
        for (feat, file) in &undeclared {
            msg.push_str(&format!("  undeclared feature {feat:?} gated in {file}\n"));
        }
        msg.push_str(&format!("\ndeclared: {declared:?}\n"));
        panic!("{msg}");
    }
}
