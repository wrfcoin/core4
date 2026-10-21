use std::path::PathBuf;
use std::{
    env,
    fs,
};

fn main() {
    const SOURCE: &str = "src/bin/launch_api/transition_extractor.rs";
    const TEST_MODULE_MARKER: &str = "\n#[cfg(test)]\nmod tests {";

    println!("cargo:rerun-if-changed={SOURCE}");

    if env::var_os("CARGO_FEATURE_CUT_COMPOSITION_HARNESS").is_none() {
        return;
    }

    let source = fs::read_to_string(SOURCE)
        .unwrap_or_else(|error| panic!("cut composition cannot read {SOURCE}: {error}"));
    let production_end = source.find(TEST_MODULE_MARKER).unwrap_or_else(|| {
        panic!(
            "cut composition cannot locate the transition extractor's test-module boundary; \
             refusing to compile a partial or unknown production extractor"
        )
    });
    let production_source = &source[..production_end];
    let mut production = String::with_capacity(production_source.len());
    for line in production_source.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("//!") {
            // The source file's crate-level docs become ordinary comments when
            // included inside this integration-test module.
            production.push_str("//");
            production.push_str(rest);
        } else {
            production.push_str(line);
        }
    }
    if !production.contains("pub fn extract_transitions(") {
        panic!(
            "cut composition found no extract_transitions implementation before the test module"
        );
    }

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"))
        .join("cut_composition_transition_extractor.rs");
    fs::write(&output, &production).unwrap_or_else(|error| {
        panic!(
            "cut composition cannot write generated production extractor {}: {error}",
            output.display()
        )
    });
}
