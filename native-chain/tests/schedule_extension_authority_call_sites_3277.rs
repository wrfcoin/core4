//! #3277 S5, review-s2 condition F1: the schedule-extension authority is
//! consensus input, so it may be built only from block 1 and the compiled-in
//! pins, through ONE production call site.
//!
//! A source scan over every production Rust file of the crates that run
//! consensus (shared-protocol, native-chain, the CLI). Test code is cut at
//! the first `#[cfg(test)]` test module of each file; files under `tests/`
//! are skipped.
//!
//! - `install_schedule_extension_authority_from_block1(` is called from
//!   exactly one place: `native-chain/src/upgrade.rs`
//!   (`install_schedule_extension_authority_for_block1`).
//! - The test-only constructor is referenced by no production code except its
//!   own `#[cfg(any(test, feature = "test-support"))]` definition.
//! - The old unrestricted installer no longer exists.

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "tests" && name != "target" && name != "benches" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// `(file relative to the workspace, production text)` for every scanned file.
fn production_sources() -> Vec<(String, String)> {
    let root = workspace();
    let mut files = Vec::new();
    for dir in ["shared-protocol/src", "native-chain/src", "tools/cli/WrfcoinCLI/src"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "vacuity: the scan must see the crates ({} files)", files.len());
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            // Cut at the first test MODULE: some files carry #[cfg(test)]
            // items earlier, and cutting there would hide production code.
            let end = text.find("#[cfg(test)]\nmod ").unwrap_or(text.len());
            let production = text[..end].to_string();
            let rel = path.strip_prefix(&root).unwrap().to_string_lossy().to_string();
            (rel, production)
        })
        .collect()
}

fn code_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.trim_start().starts_with("//"))
}

#[test]
fn the_authority_has_one_production_call_site() {
    const CALL: &str = "install_schedule_extension_authority_from_block1(";
    let mut callers = Vec::new();
    for (file, text) in production_sources() {
        for line in code_lines(&text) {
            if line.contains(CALL) && !line.contains("pub fn install_schedule_extension_authority_from_block1") {
                callers.push(file.clone());
            }
        }
    }
    assert_eq!(
        callers,
        vec!["native-chain/src/upgrade.rs".to_string()],
        "the authority must be built through upgrade::install_schedule_extension_authority_for_block1 only"
    );
}

#[test]
fn no_production_code_can_construct_an_arbitrary_authority() {
    const TEST_ONLY: &str = "install_schedule_extension_authority_for_tests";
    for (file, text) in production_sources() {
        let lines: Vec<&str> = code_lines(&text).collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains(TEST_ONLY) {
                continue;
            }
            // The one allowed occurrence: its own definition, gated to tests.
            let gated = line.contains("pub fn install_schedule_extension_authority_for_tests")
                && index > 0
                && lines[index - 1].contains("#[cfg(any(test, feature = \"test-support\"))]");
            assert!(gated, "{file}: production code references {TEST_ONLY}: {line}");
        }
        for line in &lines {
            assert!(
                !line.contains("install_schedule_extension_authority(")
                    && !line.contains("fn install_schedule_extension_authority<")
                    && !line.contains("pub fn install_schedule_extension_authority("),
                "{file}: the unrestricted installer must not come back: {line}"
            );
        }
    }
}
