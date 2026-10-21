//! Registration guard for Cargo integration targets (core4#2888, story 5).
//!
//! `cargo test --workspace --lib` never runs an integration target, so a target
//! executes in CI only if `scripts/integration_test_ratchet.json` lists it and
//! the ratchet runs it. Registration was a manual step that could be forgotten:
//! #2979 shipped a target the ratchet later caught as unregistered, at PR time,
//! in a different job from the one the author was watching.
//!
//! This module moves that failure to the point of addition. Every workspace
//! crate with integration targets carries a `#[cfg(test)]` unit test that
//! calls [`assert_registered`] with its own `CARGO_MANIFEST_DIR` and
//! `CARGO_PKG_NAME`. A developer who adds `tests/new_target.rs` and runs
//! `cargo test -p <crate>` — the command they were going to run anyway — gets
//! a failure naming the target and the manifest entry to add, before a commit
//! exists. The ratchet's own `--validate-only` remains the authority in CI;
//! this is the same rule, one step earlier, with no second inventory to drift.
//!
//! # Where the target list comes from
//!
//! `cargo metadata --no-deps`, the same source the ratchet reads. Not a
//! directory listing: `read_dir` is a construct the determinism gate rejects in
//! this crate (directory order is not deterministic — `scripts/check_determinism.py`,
//! `readdir-unsorted`), and a listing is also the weaker instrument. Cargo's
//! metadata already knows about explicit `[[test]]` entries, `autotests = false`,
//! and `tests/<dir>/main.rs`, so asking it is both gate-clean and exact.
//!
//! Only additions are checked here. A target deleted without removing its
//! manifest entry is caught by the ratchet's `manifest targets no longer
//! discovered` check, which belongs there.
//!
//! # Feature-gated on purpose
//!
//! This module and its one dependency (`serde_json`, for the metadata JSON)
//! exist only behind the `integration-guard` feature, which crates enable from
//! their `[dev-dependencies]`. The primitives crate's production dependency
//! surface — `serde` alone — is unchanged; the guard is test tooling and is
//! compiled only into test builds.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manifest path, relative to the workspace root.
pub const MANIFEST_RELATIVE: &str = "scripts/integration_test_ratchet.json";

/// Integration targets Cargo reports for `package` (`kind` contains `"test"`),
/// read from `cargo metadata --no-deps` against the crate's own manifest.
///
/// The cargo binary is the one that compiled this crate (`CARGO` at compile
/// time), falling back to `cargo` on `PATH`.
pub fn discovered_targets(crate_dir: &Path, package: &str) -> Result<BTreeSet<String>, String> {
    let cargo = option_env!("CARGO").unwrap_or("cargo");
    let manifest = crate_dir.join("Cargo.toml");
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--offline", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("could not run `{cargo} metadata` for {}: {e}", manifest.display()))?;
    if !output.status.success() {
        return Err(format!(
            "`{cargo} metadata --no-deps --manifest-path {}` failed ({}): {}",
            manifest.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("cargo metadata output is not JSON: {e}"))?;
    let mut found = BTreeSet::new();
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata output has no `packages` array")?;
    for pkg in packages {
        if pkg["name"].as_str() != Some(package) {
            continue;
        }
        for target in pkg["targets"].as_array().into_iter().flatten() {
            let is_test = target["kind"]
                .as_array()
                .map(|kinds| kinds.iter().any(|k| k.as_str() == Some("test")))
                .unwrap_or(false);
            if let (true, Some(name)) = (is_test, target["name"].as_str()) {
                found.insert(name.to_string());
            }
        }
    }
    Ok(found)
}

/// Target names the manifest's `inventory` array registers for `package`.
///
/// Scans the `"inventory": [ ... ]` region only, so an `external_coverage`,
/// `known_failures` or `docker_gated` entry cannot stand in for registration —
/// those all refer to targets that must ALSO be in the inventory. A string scan
/// rather than a parse, deliberately: the manifest's inventory has a fixed,
/// reviewed shape, and the scan is what keeps this usable without pulling the
/// manifest's full schema into the primitives crate.
pub fn registered_targets(manifest_text: &str, package: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let Some(start) = manifest_text.find("\"inventory\"") else {
        return found;
    };
    let region = &manifest_text[start..];
    // The inventory array closes at the first `\n  ]` (two-space indent, the
    // manifest's fixed formatting). Fall back to the whole remainder if the
    // formatting ever changes: that over-approximates and therefore can never
    // hide a missing registration.
    let region = match region.find("\n  ]") {
        Some(end) => &region[..end],
        None => region,
    };
    let mut rest = region;
    while let Some(pkg_at) = rest.find("\"package\"") {
        rest = &rest[pkg_at..];
        let Some(pkg) = quoted_value_after_key(rest, "\"package\"") else {
            break;
        };
        let Some(tgt_at) = rest.find("\"target\"") else {
            break;
        };
        let after_target = &rest[tgt_at..];
        let Some(tgt) = quoted_value_after_key(after_target, "\"target\"") else {
            break;
        };
        if pkg == package {
            found.insert(tgt.to_string());
        }
        rest = &after_target["\"target\"".len()..];
    }
    found
}

/// Given text starting at `key`, return the string value after `key":`.
fn quoted_value_after_key<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let after = text.strip_prefix(key)?;
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(&after[..end])
}

/// Walk up from `crate_dir` until the manifest is found.
pub fn find_manifest(crate_dir: &Path) -> Option<PathBuf> {
    let mut dir = Some(crate_dir);
    while let Some(current) = dir {
        let candidate = current.join(MANIFEST_RELATIVE);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// Targets Cargo reports for `package` that the manifest does not register.
/// `Err` when the manifest cannot be found or read, or cargo metadata fails —
/// a guard that cannot see its inputs must not pass.
pub fn unregistered_targets(crate_dir: &Path, package: &str) -> Result<Vec<String>, String> {
    let manifest = find_manifest(crate_dir).ok_or_else(|| {
        format!(
            "{MANIFEST_RELATIVE} not found walking up from {}",
            crate_dir.display()
        )
    })?;
    let text = fs::read_to_string(&manifest)
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    let registered = registered_targets(&text, package);
    Ok(discovered_targets(crate_dir, package)?
        .into_iter()
        .filter(|target| !registered.contains(target))
        .collect())
}

/// The guard: panics naming every unregistered target and the entry to add.
///
/// Call from a `#[cfg(test)]` unit test with `env!("CARGO_MANIFEST_DIR")` and
/// `env!("CARGO_PKG_NAME")`, so the failure lands in `cargo test -p <crate>`.
pub fn assert_registered(crate_dir: &str, package: &str) {
    let crate_dir = Path::new(crate_dir);
    match unregistered_targets(crate_dir, package) {
        Ok(missing) if missing.is_empty() => {}
        Ok(missing) => {
            let entries = missing
                .iter()
                .map(|target| {
                    format!(
                        "    {{ \"package\": \"{package}\", \"target\": \"{target}\" }}"
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            panic!(
                "\n\n{n} integration target(s) of {package} are not registered in \
                 {manifest}, so nothing in CI will ever run them (#2888):\n\n{list}\n\n\
                 Add to the \"inventory\" array (keep it sorted by package, then target):\n\
                 {entries}\n\n\
                 If the target asserts nothing by design, it ALSO needs a reviewed \
                 `zero_test_exceptions` entry; if it needs Docker, a `docker_gated` entry.\n",
                n = missing.len(),
                manifest = MANIFEST_RELATIVE,
                list = missing
                    .iter()
                    .map(|t| format!("  {t}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        Err(reason) => panic!(
            "integration target registration guard could not read its inputs: {reason}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
  "schema": 1,
  "inventory": [
    {
      "package": "alpha",
      "target": "one"
    },
    {
      "package": "alpha",
      "target": "two",
      "review": {
        "source_pr": 10,
        "owning_issue": 20,
        "covers": "The reviewed behavior, which mentions \"target\" in prose."
      }
    },
    {
      "package": "beta",
      "target": "three"
    }
  ],
  "external_coverage": [
    {
      "package": "alpha",
      "target": "not_in_inventory",
      "step": "step"
    }
  ]
}
"#;

    /// A throwaway single-package workspace under the OS temp dir, with the
    /// auto-discovery shapes Cargo recognises and one it must ignore.
    fn scratch_workspace(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wrf-primitives-inventory-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let crate_dir = root.join("crates").join("alpha");
        fs::create_dir_all(crate_dir.join("src")).unwrap();
        fs::create_dir_all(crate_dir.join("tests").join("common")).unwrap();
        fs::create_dir_all(crate_dir.join("tests").join("suite")).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        fs::write(crate_dir.join("src").join("lib.rs"), "").unwrap();
        fs::write(crate_dir.join("tests").join("one.rs"), "").unwrap();
        fs::write(crate_dir.join("tests").join("brand_new.rs"), "").unwrap();
        fs::write(crate_dir.join("tests").join("common").join("mod.rs"), "").unwrap();
        fs::write(crate_dir.join("tests").join("suite").join("main.rs"), "").unwrap();
        fs::write(crate_dir.join("tests").join("notes.md"), "").unwrap();
        root
    }

    #[test]
    fn registered_targets_reads_only_the_inventory_for_the_named_package() {
        let alpha = registered_targets(MANIFEST, "alpha");
        assert_eq!(alpha.into_iter().collect::<Vec<_>>(), vec!["one", "two"]);
        let beta = registered_targets(MANIFEST, "beta");
        assert_eq!(beta.into_iter().collect::<Vec<_>>(), vec!["three"]);
        assert!(registered_targets(MANIFEST, "gamma").is_empty());
        // An external_coverage entry is not a registration.
        assert!(!registered_targets(MANIFEST, "alpha").contains("not_in_inventory"));
    }

    #[test]
    fn discovered_targets_come_from_cargo_metadata_not_a_listing() {
        let root = scratch_workspace("discover");
        let crate_dir = root.join("crates").join("alpha");
        let found = discovered_targets(&crate_dir, "alpha").unwrap();
        fs::remove_dir_all(&root).unwrap();
        // `common/mod.rs` and `notes.md` are not targets; `suite/main.rs` is.
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            vec!["brand_new", "one", "suite"]
        );
    }

    #[test]
    fn discovered_targets_for_an_unknown_package_is_empty_not_an_error() {
        let root = scratch_workspace("unknown");
        let found = discovered_targets(&root.join("crates").join("alpha"), "omega").unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn unregistered_targets_names_exactly_the_missing_ones() {
        let root = scratch_workspace("unregistered");
        let crate_dir = root.join("crates").join("alpha");
        fs::create_dir_all(root.join("scripts")).unwrap();
        // `one` is registered; `brand_new` and `suite` are not.
        fs::write(root.join(MANIFEST_RELATIVE), MANIFEST).unwrap();
        let missing = unregistered_targets(&crate_dir, "alpha").unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(missing, vec!["brand_new", "suite"]);
    }

    #[test]
    fn missing_manifest_is_an_error_not_a_pass() {
        let root = scratch_workspace("no-manifest");
        let result = unregistered_targets(&root.join("crates").join("alpha"), "alpha");
        fs::remove_dir_all(&root).unwrap();
        assert!(result.is_err(), "a guard that cannot find its manifest must not pass");
    }

    #[test]
    fn this_workspace_reports_a_known_target_for_shared_protocol() {
        // The real tree, the real cargo: wycheproof has been an integration
        // target of shared-protocol since 2026-07-22 and is in the inventory.
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let shared = here.parent().unwrap().join("shared-protocol");
        let found = discovered_targets(&shared, "wrfcoin-shared-protocol").unwrap();
        assert!(found.contains("wycheproof"), "{found:?}");
    }
}
