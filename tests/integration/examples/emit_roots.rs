//! Emit the determinism fixture's per-height state roots as JSON to stdout.
//!
//! This is the CI entry point the determinism matrix (#2104) invokes on each
//! OS/arch/profile leg:
//!
//! ```sh
//! cargo run -p test_server --example emit_roots --release -- --out roots-<leg>.json
//! ```
//!
//! **Write to `--out`, never a `>` redirect.** Under `cross` the toolchain
//! prints to stdout inside the container (e.g. rustup's
//! `1.98.0-... unchanged - rustc 1.98.0 ...`), which a redirect captures into
//! the file ahead of the JSON. `json.loads` then parses the leading `1.98` as a
//! number and reports the remainder as `Extra data: line 2 column 7`, crashing
//! the comparator on five legs that had in fact agreed. `--out` writes the
//! document directly, so nothing the toolchain emits can corrupt it.
//!
//! It reuses [`replay_fixture`](test_server::determinism::replay_fixture) — the
//! single source of replay logic — so every leg computes roots identically and
//! `scripts/compare_state_roots.py` can diff the outputs.

use test_server::determinism::{fixture_dir, replay_fixture};

fn main() {
    // `--out <path>` (or a bare positional path) writes the JSON document
    // directly. Falls back to stdout so local ad-hoc use still works, but CI
    // must always pass --out — see the module docs.
    let mut args = std::env::args().skip(1);
    let mut out_path: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out_path = args.next(),
            other if !other.starts_with('-') => out_path = Some(other.to_string()),
            other => {
                eprintln!("emit_roots: unrecognized argument {other}");
                std::process::exit(2);
            },
        }
    }

    let roots = replay_fixture(&fixture_dir());
    // Stable, sorted-by-height JSON: { "<height>": "<state_root hex>" }.
    let mut items: Vec<(u64, String)> = roots;
    items.sort_by_key(|(h, _)| *h);
    let map: std::collections::BTreeMap<String, String> =
        items.into_iter().map(|(h, r)| (h.to_string(), r)).collect();
    let json = serde_json::to_string_pretty(&map).expect("serialize roots");
    match out_path {
        Some(path) => {
            std::fs::write(&path, format!("{json}\n"))
                .unwrap_or_else(|e| panic!("emit_roots: cannot write {path}: {e}"));
            // Progress goes to stderr so it can never land in the document.
            eprintln!("emit_roots: wrote {} heights to {path}", map.len());
        },
        None => println!("{json}"),
    }
}
