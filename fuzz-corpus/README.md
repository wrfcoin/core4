# Fuzz Regression Corpus

Permanent, committed regression inputs for the `wrfcoin-fuzz` targets (`fuzz/Cargo.toml`),
established for epic #2102 story #2110. This directory is **not** the fuzzing working corpus
(`fuzz/corpus/<target>/`, which the nightly campaign grows and does not commit) — it is the
committed set of inputs that must **never crash again**, plus a known-good seed per target so
the replay gate always exercises real work.

## Two kinds of entry

- `fuzz-corpus/<target>/seed-<name>` — a **known-good** input. Every target currently carries a
  `seed-minimal` because no historical crasher has been harvested yet; these keep the gate
  non-vacuous (each target is built and run once). Seeds need no note.
- `fuzz-corpus/<target>/crash-<sha256[:12]>` — a **reproducer** for a fixed crash. A crash entry
  **must** carry a paired note (below); the replay gate fails a crash entry that lacks one.

## Policy — how an input gets added, and what enforces it

1. **Every crash found by fuzzing becomes a committed regression, in the same PR as its fix.**
   A crash discovered but not committed as a corpus entry gets reintroduced. This is enforced by
   convention at review time (there is no bot that auto-harvests nightly crashers into this
   directory — the nightly job uploads them as artifacts; a human commits the ones worth
   freezing, with the fix). Replacing the current `seed-minimal` seeds with real reproducers as
   the nightly campaign finds them is the intended follow-up.
2. Store the crashing input at `fuzz-corpus/<target>/crash-<sha256[:12]>`, where `<target>` is
   the fuzz target name (e.g. `fuzz_p2p_message`) and the hash is the first 12 hex chars of the
   input's sha256.
3. Alongside it, commit a one-line note `fuzz-corpus/<target>/crash-<sha256[:12]>.md` naming the
   issue it corresponds to, e.g. `Fixes #1234 — panic in length prefix decode`. **This is
   enforced**: `scripts/replay_fuzz_corpus.sh` (and its `--check` mode) fails any `crash-*` entry
   with no paired `.md`, so the note requirement is a gate, not a wish.
4. A panic reachable from network input is a **security finding**, not a test failure — see
   [`docs/SECURITY.md`](../docs/SECURITY.md) for the private disclosure path. Do not paste a
   live-exploit reproducing input into a public issue.

## Replay gate

`scripts/replay_fuzz_corpus.sh` runs **every committed corpus entry through its target once**
(a single replay, not a fuzzing campaign) and fails on any panic. It is wired into PR CI
(`fuzz-nightly.yml`, `corpus-replay` job) so a regression is caught on the PR that causes it,
not at the next nightly. Locally:

```sh
scripts/replay_fuzz_corpus.sh --check     # validate targets + entries + notes (no cargo-fuzz)
scripts/replay_fuzz_corpus.sh             # real replay (needs nightly + cargo-fuzz)
```

`--check` fails on: an **empty** corpus (no target dirs or no entries), a target directory with
no replayable entry, a directory name that is not a `[[bin]]` in `fuzz/Cargo.toml`, or a
`crash-*` entry missing its note. An empty corpus is a **failure**, not a green "replayed
nothing" — a gate that cannot tell the two apart is the defect. The `corpus-replay` CI job also
runs a self-test that plants a crashing input behind a stubbed `cargo` and asserts the replay
exits non-zero, so the crash-detection path is proven on every run, not just asserted.
