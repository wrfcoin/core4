//! #2700 — a declared ceiling that no code consumes is documentation, not a ceiling.
//!
//! `MAX_BLOCK_SIZE`, `MAX_TRANSACTION_DATA_SIZE` and `MEMPOOL_CAPACITY` are declared in
//! `shared-protocol/src/common/constants.rs`, and `docs/security/PROTOCOL_SPEC.md` and
//! `docs/security/THREAT_MODEL.md` describe them as protocol bounds and mitigations. On
//! `main` @ 67d07852 each is referenced by **zero** lines of code outside its own
//! declaration, and the workspace's single block-size comparison
//! (`consensus-engines/src/security/attack_prevention.rs:279`) sits in
//! `AttackPrevention::validate_block`, which `native-chain` never calls.
//!
//! ## Why this test is written as a source scan
//!
//! There is no "validate this block" entry point in `shared-protocol` to call with an
//! oversized block — `blockchain/block.rs` exposes only hashing helpers. The defect is
//! precisely that **no consuming code path exists**, so the honest demonstration is that
//! the constants have no consumers, not a behavioural probe of an API that is absent.
//!
//! ## Where the RED lives (updated after #2704 / #2714 / crate-execution-gates)
//!
//! `every_declared_ceiling_is_consumed_by_code` must fail before it can be trusted, and it
//! still fails on `main` (re-verified @ b14ae8ce7, 2026-09-01). But an expected-red inside
//! an unfiltered suite would paint crate-execution-gates' "shared-protocol full suite" step
//! red for every shared-protocol PR, hiding real regressions. So the observation test is
//! `#[ignore]`: default suite runs execute only the two anti-vacuity controls (which must
//! pass everywhere), and the RED is surfaced by
//! `.github/workflows/declared-ceiling-enforcement.yml` (#2711), which runs the ignored
//! test explicitly. When enforcement is wired, each constant gains a consumer, that
//! workflow's run flips GREEN, and both the `#[ignore]` and the workflow retire together.
//! Do not "fix" this by deleting the assertions: a ceiling nobody reads is the bug.
//!
//! Docs status: #2704 struck the threat model's MAX_BLOCK_SIZE mitigation claim and
//! #2714's docs/security/RESOURCE_CEILINGS.md now ledgers what actually bounds each
//! resource — the documentation is honest; the constants remain dead.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Constants whose whole purpose is to bound remote input. Each must be consumed by at
/// least one non-declaration, non-test line of Rust.
const CEILINGS: &[&str] = &[
    "MAX_BLOCK_SIZE",
    "MAX_TRANSACTION_DATA_SIZE",
    "MEMPOOL_CAPACITY",
];

/// Positive control. `MAX_BLOCK_EMISSION` **is** enforced today
/// (`native-chain/src/bin/launch_api/routes/rewards.rs:173`). If the scanner reports zero
/// consumers for this one, the scanner itself is broken and every result above is vacuous.
const ENFORCED_CONTROL: &str = "MAX_BLOCK_EMISSION";

/// The file that declares the ceilings; matches here are declarations, not consumption.
const DECLARATION_FILE: &str = "shared-protocol/src/common/constants.rs";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/shared-protocol`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("shared-protocol must have a parent workspace directory")
        .to_path_buf()
}

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // Skip build output and vendored code; neither is our source of truth.
            if matches!(name.as_ref(), "target" | ".git" | "node_modules" | "vendor") {
                continue;
            }
            collect_rust_sources(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Count lines that *consume* `symbol`: not its declaration file, not a comment, and not
/// inside a `#[cfg(test)]`-only file under `tests/`.
fn consuming_references(sources: &[PathBuf], root: &Path, symbol: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for path in sources {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if rel == DECLARATION_FILE {
            continue; // the declaration itself is not consumption
        }
        if rel.contains("/tests/") || rel.starts_with("tests/") {
            continue; // a test asserting the constant is not enforcement
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") || trimmed.starts_with("/*") {
                continue; // a mention in prose is not enforcement
            }
            if line.contains(symbol) {
                hits.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    hits
}

#[test]
fn scanner_examined_a_real_source_tree() {
    // Anti-vacuity: if the walk is broken or the path is wrong, every assertion below
    // would trivially "pass" with zero consumers. Fail loudly here instead.
    let root = workspace_root();
    let mut sources = Vec::new();
    collect_rust_sources(&root, &mut sources);

    assert!(
        sources.len() > 200,
        "scanner found only {} .rs files under {} — the walk is broken, so every \
         ceiling result would be vacuous",
        sources.len(),
        root.display()
    );

    let declaration = root.join(DECLARATION_FILE);
    assert!(
        declaration.is_file(),
        "declaration file {} not found — the ceilings may have moved; update this test \
         rather than deleting it",
        declaration.display()
    );
    let declared = fs::read_to_string(&declaration).expect("constants.rs must be readable");
    for symbol in CEILINGS {
        assert!(
            declared.contains(symbol),
            "{symbol} is no longer declared in {DECLARATION_FILE}; if it was removed \
             deliberately, drop it from CEILINGS in this test"
        );
    }
}

#[test]
fn positive_control_enforced_constant_has_consumers() {
    // Proves the scanner can see consumption when it exists. If this fails, the
    // zero-consumer findings below mean nothing.
    let root = workspace_root();
    let mut sources = Vec::new();
    collect_rust_sources(&root, &mut sources);

    let hits = consuming_references(&sources, &root, ENFORCED_CONTROL);
    assert!(
        !hits.is_empty(),
        "positive control {ENFORCED_CONTROL} reported zero consumers — the scanner is \
         broken and no conclusion can be drawn from the ceiling assertions"
    );
}

#[test]
#[ignore = "#2700: deliberately RED until ceiling enforcement lands. Ignored so that \
            unfiltered suite runs (crate-execution-gates' shared-protocol full suite, \
            cargo test -p wrfcoin-shared-protocol) stay green; the RED is surfaced by \
            .github/workflows/declared-ceiling-enforcement.yml, which opts in with \
            `-- --ignored --exact`. Remove this attribute together with that workflow \
            when enforcement exists."]
fn every_declared_ceiling_is_consumed_by_code() {
    let root = workspace_root();
    let mut sources = Vec::new();
    collect_rust_sources(&root, &mut sources);

    let mut unconsumed: BTreeMap<&str, usize> = BTreeMap::new();
    for symbol in CEILINGS {
        let hits = consuming_references(&sources, &root, symbol);
        if hits.is_empty() {
            unconsumed.insert(symbol, 0);
        }
    }

    assert!(
        unconsumed.is_empty(),
        "#2700 — these declared ceilings are consumed by NO code outside their \
         declaration, so nothing enforces them:\n  {}\n\n\
         The docs no longer overclaim (#2704 struck the threat-model mitigation; #2714's \
         docs/security/RESOURCE_CEILINGS.md ledgers what actually bounds each resource), \
         but the constants are still declared and still dead. Either wire them into the \
         block/transaction/mempool admission paths, or remove them and update that \
         ledger — a declared ceiling nobody reads is the bug.",
        unconsumed.keys().copied().collect::<Vec<_>>().join("\n  ")
    );
}
