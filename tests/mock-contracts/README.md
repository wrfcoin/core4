# Mock-contract archive

Files below `legacy/` are preserved historical coordinator fixtures. They are
not integration tests, end-to-end tests, launch evidence, merge evidence, or
proof that a blockchain transition occurred.

Each archived file supplied the reward, balance, signature verdict, block
confirmation, or pool result from a file-local mock and then asserted that same
value. Several could not execute at all after the backend/frontend repository
split. Their `.fixture.js` / `.fixture.ts` suffixes and location outside
`tests/integration` and Playwright discovery are deliberate.

Do not rename one back to `*.test.*`. To promote a case into executable
evidence, replace its mock with a production subject, read the claimed result
from post-application state, and add an asymmetric control that removes only
the transition under test.

The guard scans every discovered `*.test.*` and `*.spec.*` file (including
`.test.cjs` and `.spec.cjs`) below
`tests/integration/` and `tests/e2e/`. It rejects an archived marker or a full
manifest fingerprint at any active path, not only the nine retired filenames.
The CI workflow likewise triggers on both complete discovery trees. This is a
regression guard for the archived cases, not a general static proof that every
future test uses production subjects; new evidence still requires review
against the promotion rule above.

The machine-readable dispositions and the bounded set of production evidence
that replaces these claims live in
`scripts/semantic_evidence_manifest.json`. Run:

```text
python3 scripts/semantic_evidence_gate.py --self-test
python3 scripts/semantic_evidence_gate.py
python3 scripts/semantic_evidence_gate.py --require-cut-ready
```

The first command proves the guard rejects an old active mock path, the same
mock fingerprint renamed under test discovery, a drifted production subject,
and an explicitly stale cut claim. The second validates the archive and
evidence inventory. The third is the fail-closed cut verdict; it remains RED
while any production evidence entry is `stale_for_cut`. Once that status gate
passes, the owner runs the retained local gate at the candidate checkout:

```text
python3 scripts/semantic_evidence_gate.py --require-cut-ready --run-cut-ready-commands
```

This executes every recorded manifest command; labels and source tokens alone
cannot declare cut-ready. The unused hosted workflow wrapper was retired in
r21; these commands remain separate from unrelated CI Readiness jobs. Running
them still requires the owner's reviewed environment and resource admission.
