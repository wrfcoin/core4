//! The output contract between `zk-proving-report` and `docs/zk/PROVING_COSTS.md` (#2177, #2230).
//!
//! # Why this lives in the library rather than beside the renderer
//!
//! The contract and its checker were originally written inside
//! `src/bin/zk-proving-report.rs`, next to the renderer they guard. That put
//! the guard in a **binary**, and `cargo test --lib` does not build binaries —
//! so the two tests proving the guard worked never ran under the command this
//! crate is tested with, nor under CI, whose `rust-tests` job runs
//! `cargo test --workspace --lib --all-features`.
//!
//! The #2230 review asked for "an output-contract assertion so they cannot
//! disappear silently again". An assertion that no gate executes is the same
//! defect one level up: it exists, it passes, and nothing runs it. Moving the
//! field list and the checker here puts both under `--lib`, and therefore under
//! CI, alongside a test that validates the **committed artifacts** rather than
//! only synthetic input.
//!
//! # What is checked
//!
//! `docs/zk/PROVING_COSTS.md` states that setup, prove, verify and `process_vk`
//! each report a median "with min–max in the JSON". That claim was false for
//! `process_vk` for two revisions: the child emitted min/max/samples, the
//! aggregating struct had no home for them, absorption dropped them, and the
//! renderer never wrote them — so the document promised a reader could audit
//! the sampling while the artifact did not let them.
//!
//! It mattered because the conclusion resting on `process_vk` had already been
//! retracted **twice** for under-sampling. Publishing a figure with that
//! history, minus the fields that would let a reader check the sampling, is the
//! worst possible place to lose a field.

/// Every per-measurement field `docs/zk/PROVING_COSTS.md` promises the JSON
/// contains.
///
/// Adding a promised field to the document means adding it here; the tests
/// below then require every committed artifact to carry it.
pub const PROMISED_JSON_FIELDS: &[&str] = &[
    "constraints",
    "synth_micros",
    "setup_min_micros",
    "setup_median_micros",
    "setup_max_micros",
    "setup_samples",
    "setup_peak_rss_kib",
    "pk_compressed_bytes",
    "pk_uncompressed_bytes",
    "vk_compressed_bytes",
    "vk_uncompressed_bytes",
    "key_load_micros",
    "prove_min_micros",
    "prove_median_micros",
    "prove_max_micros",
    "prove_samples",
    "prove_peak_rss_kib",
    "proof_bytes",
    "envelope_bytes",
    "process_vk_micros",
    "process_vk_min_micros",
    "process_vk_max_micros",
    "process_vk_samples",
    "verify_min_micros",
    "verify_median_micros",
    "verify_max_micros",
    "verify_samples",
    "verify_peak_rss_kib",
    "public_inputs",
];

/// Fail if the rendered JSON is missing a field the document promises.
///
/// Checked against the *rendered text*, not against the measurement struct, on
/// purpose: the bug was in the renderer, so a check that read the struct would
/// have passed while the artifact stayed wrong. This asserts what a reader
/// actually receives.
///
/// `rows` is the number of measurement rows the text should contain, so a field
/// present on one row and missing on another still fails — a partially-broken
/// sweep must not pass.
pub fn assert_json_contract(json: &str, rows: usize) -> Result<(), String> {
    if rows == 0 {
        return Ok(());
    }
    let mut missing: Vec<&str> = PROMISED_JSON_FIELDS
        .iter()
        .copied()
        .filter(|f| json.matches(&format!(r#""{f}":"#)).count() < rows)
        .collect();
    missing.sort_unstable();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "rendered JSON is missing {} promised field(s) on at least one of {rows} rows: {}\n\
         docs/zk/PROVING_COSTS.md documents these as present; either emit them or correct the \
         document — do not ship an artifact that does not match its own methodology section.",
        missing.len(),
        missing.join(", "),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn measurements_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("docs/zk/measurements")
    }

    /// Every committed artifact satisfies the contract, on every row.
    ///
    /// This is the assertion that actually protects a reader: the previous
    /// revision shipped 15 rows in each of three files with three promised
    /// `process_vk` fields absent, and nothing failed.
    #[test]
    fn every_committed_artifact_satisfies_the_contract() {
        let dir = measurements_dir();
        let mut checked = 0;
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        entries.sort();

        for path in &entries {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let rows = text.matches(r#""constraints":"#).count();
            assert!(
                rows > 0,
                "{} contains no measurement rows — a vacuous pass looks identical \
                 to a working check",
                path.display()
            );
            assert_json_contract(&text, rows).unwrap_or_else(|e| {
                panic!("{} violates the published output contract:\n{e}", path.display())
            });
            checked += 1;
        }

        assert!(
            checked >= 3,
            "expected at least the A/B/D artifacts, found {checked} in {}",
            dir.display()
        );
    }

    /// The three fields the #2230 review found missing are named in the
    /// contract and present in every artifact — pinned by name, so a future
    /// edit that trims the list is a test failure rather than a silent loss.
    #[test]
    fn the_process_vk_distribution_is_promised_and_present() {
        let fields = [
            "process_vk_micros",
            "process_vk_min_micros",
            "process_vk_max_micros",
            "process_vk_samples",
        ];
        for f in fields {
            assert!(
                PROMISED_JSON_FIELDS.contains(&f),
                "{f} must be a promised field"
            );
        }

        let dir = measurements_dir();
        for name in [
            "A-workstation-default.json",
            "B-workstation-rayon4.json",
            "D-single-core-bound.json",
        ] {
            let path = dir.join(name);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let rows = text.matches(r#""constraints":"#).count();
            for f in fields {
                assert_eq!(
                    text.matches(&format!(r#""{f}":"#)).count(),
                    rows,
                    "{name}: {f} must appear on all {rows} rows"
                );
            }
        }
    }

    /// Probe the failure, not just the success: a check that only asserts the
    /// happy path would have stayed green through the original defect.
    #[test]
    fn a_dropped_field_fails_the_contract() {
        let row = |extra: &str| {
            let body: String = PROMISED_JSON_FIELDS
                .iter()
                .filter(|f| **f != extra)
                .map(|f| format!(r#""{f}":0,"#))
                .collect();
            format!("{{{body}\"id\":\"x\"}}")
        };

        let complete: String = PROMISED_JSON_FIELDS
            .iter()
            .map(|f| format!(r#""{f}":0,"#))
            .collect();
        assert!(
            assert_json_contract(&format!("{{{complete}}}"), 1).is_ok(),
            "a complete row must satisfy the contract"
        );

        for dropped in [
            "process_vk_min_micros",
            "process_vk_max_micros",
            "process_vk_samples",
        ] {
            let err = assert_json_contract(&row(dropped), 1)
                .expect_err("a dropped promised field must fail the contract");
            assert!(err.contains(dropped), "failure must name {dropped}: {err}");
        }
    }

    /// A field present on one row but missing on another must still fail.
    #[test]
    fn the_contract_is_per_row_not_per_file() {
        let complete: String = PROMISED_JSON_FIELDS
            .iter()
            .map(|f| format!(r#""{f}":0,"#))
            .collect();
        let partial: String = PROMISED_JSON_FIELDS
            .iter()
            .filter(|f| **f != "process_vk_samples")
            .map(|f| format!(r#""{f}":0,"#))
            .collect();

        let two_good = format!("[{{{complete}}},{{{complete}}}]");
        assert!(assert_json_contract(&two_good, 2).is_ok());

        let one_bad = format!("[{{{complete}}},{{{partial}}}]");
        assert!(
            assert_json_contract(&one_bad, 2).is_err(),
            "a field missing from ONE of two rows must fail"
        );
    }

    /// Zero rows is not a pass with contents — it is the "nothing to check"
    /// case, and it must not be reachable from the artifact test above.
    #[test]
    fn zero_rows_is_vacuous_by_construction() {
        assert!(assert_json_contract("", 0).is_ok());
        assert!(
            assert_json_contract("", 1).is_err(),
            "one expected row with no fields must fail"
        );
    }
}
