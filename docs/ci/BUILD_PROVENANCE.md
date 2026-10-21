# Build provenance — asking a binary which commit it is (#2721)

The genesis cut card's **A1** abort states that a node built from the wrong commit is *invisible to
the chain*: two genuinely different binaries peered, synced and converged on an identical
`state_root` with no complaint. It concludes that build discipline is the only defence. Until #2721
that defence had no verification step at the binary level — there was no command an operator holding
a binary could run to answer "which commit is this?".

## What every binary answers now

An ordinary developer build:

```
$ cargo build -p wrfcoin_cli && wrfcoin --version
wrfcoin 0.1.0 (ba836f1ac unverified, built 2026-08-31T12:31:54Z)
tree:      unverified
```

A ceremony or release build, which is the only kind that may claim `clean`:

```
$ WRFCOIN_BUILD_PROVENANCE_ATTEST=1 cargo build --release -p wrfcoin_cli && wrfcoin --version
wrfcoin 0.1.0 (ba836f1ac clean, built 2026-08-31T12:32:22Z)
commit:    ba836f1acbe92c6486892e7b2a10c5baeebcc95e
tree:      clean
built:     2026-08-31T12:32:22Z (build-time)
```

`oracle_ceremony_sign --version` and `oracle_ceremony_verify --version` print the same block. Before
#2721 the first exited 2 with a usage line and the second **panicked** (exit 101) trying to open a
file literally named `--version`.

## The three fields

| Field | Values | Meaning |
|---|---|---|
| `commit` | 40 hex chars, or `unknown` | `git rev-parse HEAD` at build time |
| `tree` | `clean`, `dirty`, `unverified`, `unknown` | see the table below — `clean` requires an attested build |
| `built` | RFC3339 UTC, or `unknown` | `SOURCE_DATE_EPOCH` when set, else wall clock; the source is printed beside it |

**Neither `unknown` nor `unverified` is `clean`.** `is_clean()` returns true only for an attested
clean tree. A binary built from a source tarball with no `.git` says `unknown`; a binary from an
ordinary developer build of a clean tree says `unverified`. In both cases an operator must treat the
SHA as unverified rather than trusted — which, before #2882, they could not, because such binaries
said `clean`.

## What each word means, and which one you can trust

| Value | Meaning | What an operator does |
|---|---|---|
| `clean` | An **attested** build, tree clean. The only affirmative claim. | Trust the SHA. |
| `dirty` | The tree had uncommitted changes when provenance was computed. | Do not ship it. |
| `unverified` | The tree was clean when provenance was computed, but this build did not attest, so the value may have been cached from an earlier tree. | Re-run with attestation if you need a trustworthy answer. |
| `unknown` | No provenance at all — no `git`, or not a repository (a source tarball). | The SHA itself is unverified; nothing to re-run. |

`unverified` and `unknown` are deliberately **not** the same word. One says "ask again properly", the other says "there is nothing here to ask".

## Why this is not an input list (#2882)

The provenance a build script emits is **cached**, and cargo replays it into binaries compiled later without running any of our code. So a value computed while the tree was clean can be stamped onto a binary built after it was dirtied. That is the whole defect.

Two rounds tried to prevent it by making the script rerun whenever it could matter — enumerating the inputs that cause a rebuild. Round 1 watched `Cargo.toml`, `src` and `examples`. Round 2 replaced that with cargo's own dep-info, on the argument that dep-info is exactly the set cargo uses to decide whether to rebuild. Each round shipped with a completeness argument. Each was falsified by one more mechanism:

- **Round 1** — `include_str!` of `docs/governance/constitution.md`, a file outside every watched crate directory, compiled into production `shared-protocol`.
- **Round 2** — a tracked file read by **another crate's build script**, which appears in no dep-info at all, because rustc never opens it. Dep-info records what rustc read, not everything that can force a rebuild. That was the flaw in round 2's argument, and it is structural rather than a missed case.

The conclusion is not that the list was short twice. It is that the list has to be *complete*, completeness was never demonstrable, and a third list would fail for the same reason.

### The inversion

Stop trying to keep every value fresh. Make the **cached** value harmless.

A replayed `dirty` under-claims and misleads nobody. A replayed `unknown` likewise. Only a replayed **`clean`** does damage, precisely because an operator trusts it. So `clean` is the only value that needs proof:

- An **attested** build is one **published** by `scripts/attest_build.sh`: it verifies the tree is identical before and after the artifact is produced, and moves the artifact out of a private staging directory only if it is. The build script emits `clean` only when a 64-hex tree digest AND the wrapper's live token file agree, so `ATTEST=1` by hand — or a digest replayed from a rejected run — yields `unverified`. Such a build also makes the provenance script rerun every time, so no input needs enumerating. This is the ceremony and release path.
- Every other build is a warm developer build whose value may be replayed, so a clean tree there reports `unverified`.

There is no list left to keep complete, which is why neither falsifying mechanism above still applies — and why the next one will not either.

```bash
# Ceremony / release: the only builds that may claim `clean`.
WRFCOIN_BUILD_PROVENANCE_ATTEST=1 cargo build --release -p wrfcoin_cli
```

### Measured

On the fix, with a default target directory and direct exit codes:

| Situation | Reported |
|---|---|
| unattested, clean tree | `unverified` |
| unattested, edit `constitution.md`, ordinary build (2 units recompiled) | `unverified` — **the false `clean` is gone** |
| attested, clean tree | `clean` |
| attested, edit `docs/ci/BUILD_PROVENANCE.md` — a file **nothing compiles** | `dirty` |
| attested, restore | `clean` |
| unattested rebuild, nothing changed | 0.26s, zero `Compiling` lines |

The fourth row is the one that shows this is not another list. That file is in no dep-info, no source closure and no `include_str!`; no enumeration could have covered it, and the attested build reports it correctly anyway — because attestation does not enumerate.

The last row is the cost check. Developers pay nothing: attestation is what buys the always-rerun, and unattested builds still watch only the paths that move when the commit moves.

### Why the always-rerun is affordable here

Measured on cargo 1.88.0: a build-script rerun recompiles the crate **and every dependent**, even when the emitted output is byte-identical — verified with `SOURCE_DATE_EPOCH` pinned, where the output file's sha256 was unchanged and cargo still printed `Compiling wrfcoin-build-provenance` then `Compiling wrfcoin_cli`. That is why "just rerun always" was rejected as a general policy, and it still is. It becomes affordable only under attestation, because attestation marks the cold single-shot build where that cost is already being paid.

### The `clean` branch is tested, not just reachable

Under this design `clean` is otherwise produced only during a release ceremony, and **a path exercised only at ceremony is a path that rots unobserved** — the same failure mode as a gate that never runs. The four-way decision therefore lives in one pure function, `claim::classify`, unit-tested on every CI run. `clean_is_reachable_only_from_an_attested_clean_tree` sweeps all six input combinations and asserts exactly one yields `clean`; `no_unattested_input_can_produce_an_affirmative_claim` pins the other direction.

### What the build script alone cannot do, and why the wrapper exists

**Round 3 claimed the affirmative claim had no staleness window. That was false.** `build.rs` samples `git status` and exits; cargo then compiles dependents and links, and anything dirtying the tree inside that window is invisible to the sample. Demonstrated on `bc4ca80ec`: a build started on a clean tree, a tracked file appended 1.2s later while rustc ran, exit 0, and `--version` reported `clean, built 13:20:48Z` — stamped *before* the edit — while `git status` showed the file modified.

No work inside `build.rs` can fix that. It runs before the artifact exists, and is being asked to assert something only knowable afterwards.

### The claim is bound three ways

`clean` means **"published by a run of `scripts/attest_build.sh` that exited 0"**. Round 4 had the idea right and the binding wrong in three places.

**1. Artifact bound to verdict — by ordering, not cleanup.** The build runs with `CARGO_TARGET_DIR` set to a private staging directory; the artifact moves to `target/` **only after** the post-check passes. Round 4 built into `target/` and deleted on failure, so a wrapper killed between link and cleanup left a false-clean binary exactly where an operator collects it. Deletion afterwards cannot make pre-verification publication transactional.

**2. Token bound to a live run.** The wrapper writes its digest to a per-PID file inside staging and passes both; the build script emits `clean` only when they agree. **The digest is never printed** — round 4 printed it, and a reviewer replayed a *rejected* run's digest into direct cargo and reproduced the TOCTOU.

**3. Sample bound to sub-second reality.** The tracked-file digest uses `%y` (nanoseconds), not `%Y` (whole seconds). Round 4 used `%Y` and its proof forced a boundary with `sleep 1.1`; remove the sleep and a real edit-and-revert was invisible. That test passed because of its own scaffolding. The self-test now does the revert with **no sleep**.

**Measured on `5facc14c5`:**

| Attack | Result |
|---|---|
| replay a valid digest into direct cargo, no wrapper file | **`unverified`** |
| SIGKILL the wrapper *after* the staging binary exists, before promote | **nothing published**, no stale token |
| wrapper happy path | `ATTESTED`, published, binary reports `clean` |
| same-instant edit-and-revert, no sleep | rejected |

### Residual, stated plainly

- **A builder who controls the machine can write both the variable and the token file.** Nothing available to a build script could prevent it, and round 4's claim that a forged token cannot pass was wrong. What is closed is replay-from-logs and *accidental* attestation.
- **A SIGKILLed wrapper leaves its token** — traps do not run. It is swept at the next run's start and named per PID, but a deliberate act could reuse it in between.
- **Staging holds an unpublished binary** after a failed run. Not at the path anyone collects from, and the run says so, but it is on disk.
- Pre/post sampling detects that the tree moved, not every instant it was inconsistent.

The artifact-level guarantee rests on **publication ordering**, not on the token. That is the part to attack next.

## Reproducible builds

Embedding a timestamp makes a build non-reproducible byte-for-byte. That is not academic here:
#2721 measured that `oracle_ceremony_sign` and `oracle_ceremony_verify` currently build
**byte-identically** across build paths, which makes a recorded `sha256` a real check on those two
today. Defaulting to a wall clock would have destroyed a working mitigation.

`SOURCE_DATE_EPOCH` is therefore honoured, and the stamp's origin is embedded so a reader can tell
which kind they are looking at:

```
SOURCE_DATE_EPOCH=1756600000 cargo build --release -p wrfcoin_cli
# built:     2025-08-31T00:26:40Z (SOURCE_DATE_EPOCH)
```

A malformed `SOURCE_DATE_EPOCH` **fails the build** rather than falling back to wall clock: someone
who set it asked for reproducibility, and silently giving them a non-reproducible binary is the
failure mode this whole issue is about.

## Why `shared-protocol` takes this as a dev-dependency

`wrfcoin-shared-protocol`'s lib is consumed by `native-chain`, `p2p`, `consensus-engines`,
`privacy`, the SDK and the CLI. A regular dependency would put a git-watching build script in front
of all of them, and — per the measurement above — every commit would recompile the workspace. That
is a provenance surface that gets reverted.

A **dev-dependency** is linked only into the package's example/test/bench targets, so the lib unit
downstream crates consume is untouched. The two ceremony examples get provenance; `native-chain`
does not rebuild on commit.

## Worktree safety

Every git path is resolved through `git rev-parse`, never by joining `.git/`. In a linked worktree
`.git` is a **file**, `HEAD` lives under `<common>/worktrees/<name>/`, and refs live in the common
dir. Every lane in this repo runs in a linked worktree, so hand-rolled path math is wrong here by
default rather than by accident.
