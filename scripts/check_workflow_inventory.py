#!/usr/bin/env python3
"""Make the workflow directory say what it gates, and make the API denominator honest (#2890).

# The defect

Two `state=active` workflows in this repo had **never run once**:

    Deployment Gate                .github/workflows/deploy-gate.yml                 total_runs=0
    Semantic Evidence Cut Verdict  .github/workflows/semantic-evidence-cut-ready.yml total_runs=0

Both are `workflow_dispatch`-only, so zero runs is consistent with their config rather
than a misfire — and that is exactly the problem. `Deployment Gate` declares jobs named
`smoke-tests`, `performance-check`, `security-audit` and `deploy-decision`. To anyone
scanning the directory it reads as though deployment is gated on a security audit.
Nothing is gated on it. This is #2923's family in its purest form: **a name asserting a
guarantee that no execution backs.**

The remedy is not to make every manual tool automatic — some genuinely are on-demand.
It is to make the difference *legible without opening the Actions API*, and then to
enforce that legibility so it cannot rot back.

# Rule 1 (enforced): a dispatch-only workflow must declare itself

A workflow whose only trigger is `workflow_dispatch` must carry BOTH:

* a ``(manual …)`` marker inside its ``name:`` — this is the string GitHub shows in the
  Actions sidebar and in check listings, so it is what a reader sees without opening
  anything; and
* a ``# MANUAL-ONLY: <why>`` comment in the header, **above** the ``on:`` key — what a
  reader sees on opening the file.

Neither alone is enough: the name is what you see from outside, the comment is what you
see from inside, and the two audiences are the ones who misread the directory.

# Rule 2 (enforced): the converse, so the declaration cannot go stale

A workflow with a real trigger (`push`, `pull_request`, `schedule`, `release`,
`workflow_call`, …) must NOT carry those markers. Without this half, someone adds a
`pull_request:` trigger to a manual tool and the "(manual)" label stays forever — a gate
labelled as a toy, which is the same misreading with the sign flipped. A one-directional
declaration check is a stale exemption waiting to happen.

# Rule 3 (enforced): a workflow with no trigger at all is a file that can never run

# The denominator, and why `--reconcile` is REPORTING rather than a gate

The Actions API lists registrations, not files. GitHub keeps a registration after its
file is deleted, so the API over-counts: 50 active registrations against 38 files on
`main` at `6ce83ec2f`. An auditor working from the API alone sees reassuring names like
`enhanced-security-gate` that correspond to nothing — which is how #2888's gaps stayed
invisible. `--reconcile` prints the four classes so the next audit starts from the right
denominator:

    LIVE            registration has a file on main — the real inventory
    GITHUB_MANAGED  `dynamic/…` — Dependabot's own workflows, not files, still running
    BRANCH_ONLY     no file on main but present on an unmerged branch; dispatchable on
                    that ref, and self-corrects on merge or branch deletion
    GHOST           no file anywhere; historical runs only. Inert — it cannot be
                    dispatched, and GitHub exposes no API to delete the registration.

`--reconcile` is deliberately NOT the CI gate: it needs network and a token, so a
transient API failure would produce a red that says nothing about the tree. The static
rules above are the gate; this is the instrument.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

try:
    import yaml
except ImportError as error:  # pragma: no cover
    raise SystemExit(f"FAIL-CLOSED: PyYAML is required to inspect workflow structure: {error}")

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_WORKFLOWS = ROOT / ".github/workflows"

# Events that make a workflow run without a human pressing a button. `workflow_dispatch`
# is deliberately absent, and so is `repository_dispatch` — the latter fires only when
# something outside the repo posts to the API, which is still on-demand.
AUTOMATIC_EVENTS = {
    "push", "pull_request", "pull_request_target", "schedule", "release",
    "workflow_call", "workflow_run", "issues", "issue_comment", "check_run",
    "check_suite", "deployment", "deployment_status", "merge_group",
    "registry_package", "milestone", "label", "discussion", "create", "delete",
    "fork", "gollum", "page_build", "project", "public", "status", "watch",
}

# Two declaration KINDS, because a dispatch-only workflow is one of two very
# different things and calling both "manual" is its own misreading:
#
#   manual  — an on-demand tool. It gates nothing BY DESIGN and never will.
#   parked  — a gate whose automatic trigger was deliberately removed. It is
#             supposed to come back, so it must say how.
#
# `Docker Security Scan` and `Supply Chain Security` are parked; `Deployment Gate`
# is not a gate at all. Labelling the first two "manual" would quietly convert a
# temporary suppression into a permanent design decision, which is the stale
# exemption this repo has been bitten by repeatedly.
NAME_MARKER_RE = {
    "manual": re.compile(r"\(manual\b[^)]*\)", re.IGNORECASE),
    "parked": re.compile(r"\(parked\b[^)]*\)", re.IGNORECASE),
}
# ONE token, with the kind as a field. Two separate words (`MANUAL-ONLY` / `PARKED`)
# were the first design and were wrong: seven workflows in this repo already say
# "PARKED" in prose about a *sub-trigger* — `wrfcoin-cli-integration.yml` has a live
# `pull_request` and a parked weekly `schedule`, and that is a legitimate, common state.
# A rule keyed on the word alone flagged all seven. The declaration has to be about the
# WHOLE workflow, so it gets a token no prose uses and says exactly that: this workflow
# has no automatic trigger at all.
PREAMBLE_MARKER_RE = re.compile(
    r"^#\s*NO-AUTOMATIC-TRIGGER:\s*(?P<kind>manual|parked)\b\s*[-—:]?\s*(?P<why>.*)$"
)
JOBS_KEY_RE = re.compile(r"^jobs\s*:")
# An explicit declaration, rather than guessing from names such as "gate".  A
# workflow may be a useful on-demand tool without protecting main; only files
# that opt into this contract owe main coverage.
WORKFLOW_GATE_RE = re.compile(r"^#\s*WORKFLOW-GATE:\s*(?P<why>.*?)\s*$", re.IGNORECASE)
# A parked gate with no route back is a stale exemption with a banner. The
# declaration must cite the issue that owns the restoration, or the register that
# lists it — the two things a reader can actually follow.
RESTORE_POINTER_RE = re.compile(r"#\d+|parked-workflows|reenable-parked-workflows")

AUTOMATIC, MANUAL, NONE = "automatic", "manual", "none"


class WorkflowLoader(yaml.SafeLoader):
    """Keep GitHub Actions' `on` key as the string 'on' under YAML 1.1 bool resolution.

    Without this, PyYAML resolves the bare key `on` to boolean True and every trigger
    inspection silently reads an empty dict — the checker would then classify every
    workflow as NONE and be uniformly, confidently wrong. Same guard as
    scripts/check_terminal_skip_guards.py.
    """


for _character, _resolvers in list(WorkflowLoader.yaml_implicit_resolvers.items()):
    WorkflowLoader.yaml_implicit_resolvers[_character] = [
        (tag, regex) for tag, regex in _resolvers if tag != "tag:yaml.org,2002:bool"
    ]


@dataclass
class Finding:
    path: str
    code: str
    message: str

    def __str__(self) -> str:
        return f"[{self.code}] {self.path}: {self.message}"


@dataclass
class Inventory:
    findings: list[Finding] = field(default_factory=list)
    broken: list[Finding] = field(default_factory=list)
    manual: list[tuple[str, str]] = field(default_factory=list)  # (file, kind)
    automatic: list[str] = field(default_factory=list)


def triggers(document: dict) -> set[str]:
    """The event names a workflow declares, normalised across YAML's three shapes."""
    raw = document.get("on", document.get(True, {}))
    if isinstance(raw, str):
        return {raw}
    if isinstance(raw, list):
        return {event for event in raw if isinstance(event, str)}
    if isinstance(raw, dict):
        return {str(event) for event in raw}
    return set()


def classify(events: set[str]) -> str:
    if events & AUTOMATIC_EVENTS:
        return AUTOMATIC
    if "workflow_dispatch" in events or "repository_dispatch" in events:
        return MANUAL
    return NONE


def declaration_block(lines: list[str], index: int) -> str:
    """The contiguous comment block starting at `index`.

    The rationale for a real declaration does not fit on one line, and the restore
    pointer usually lands on the second or third. Searching only the marker line made
    every correctly-declared parked workflow report PARKED_WITHOUT_RESTORE — the rule
    firing on its own formatting rather than on the fact it checks.
    """
    block = []
    for line in lines[index:]:
        stripped = line.strip()
        if not stripped.startswith("#"):
            break
        block.append(stripped)
    return "\n".join(block)


def preamble_markers(text: str) -> dict[str, str]:
    """Declaration lines found in the PREAMBLE — the region above `jobs:`.

    Position is the point: a declaration buried among the job steps is not something a
    reader scanning the top of the file will see, and "you can tell by opening it" is the
    claim this rule exists to make true. The boundary is `jobs:` rather than `on:`
    because several workflows here keep their trigger rationale as comments *inside* the
    `on:` block (ci-readiness.yml puts `on:` on line 2), and that is exactly where a
    statement about triggers belongs.
    """
    found: dict[str, str] = {}
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if JOBS_KEY_RE.match(line):
            break
        match = PREAMBLE_MARKER_RE.match(line.strip())
        if match and match.group("kind") not in found:
            found[match.group("kind")] = declaration_block(lines, index)
    return found


def workflow_gate_reason(text: str) -> str | None:
    """Return an explicit gate declaration from the preamble, if present."""
    for line in text.splitlines():
        if JOBS_KEY_RE.match(line):
            break
        match = WORKFLOW_GATE_RE.match(line.strip())
        if match:
            return match.group("why").strip()
    return None


def declaration_reason(kind: str, block: str) -> tuple[str, str]:
    """The prose of a declaration: (reason, explanation).

    `reason` is the block with the comment markers and the
    `NO-AUTOMATIC-TRIGGER: <kind>` prefix removed. `explanation` is `reason` with the
    restore pointers (issue numbers, the parked register) taken out as well.

    The two differ because a pointer is not a reason. #2942 review, blocking finding 1:
    the marker's text was captured as `(?P<why>.*)`, which matches the empty string, so a
    bare `# NO-AUTOMATIC-TRIGGER: manual` with no reason at all was accepted as a valid
    declaration — Rule 1 documents a required `<why>`, the checker's own message says "a
    label without a reason is a label nobody can check or retire", and neither was
    enforced. The reviewer also noted the narrower version: for a parked workflow the
    restore pointer must not stand in for the explanation, or `#2695` alone counts as
    saying why coverage is off.
    """
    body = "\n".join(
        line.lstrip("#").strip() for line in block.splitlines()
    )
    reason = PREAMBLE_MARKER_RE.sub("", body, count=0)
    reason = re.sub(rf"NO-AUTOMATIC-TRIGGER:\s*{kind}\b\s*[-—:]?", "", reason)
    # Drop whole TOKENS that carry a pointer, not just the matched substring. Removing
    # the substring alone left `docs/ci/.txt` behind from `docs/ci/parked-workflows.txt`
    # — non-empty, so a declaration whose entire reason was the register still passed.
    # The residue of a deleted pointer is not an explanation.
    explanation = " ".join(
        token for token in reason.split()
        if not RESTORE_POINTER_RE.search(token)
    )
    # Punctuation left behind by the removal is not prose either.
    explanation = re.sub(r"[\s\-—:;,.()\[\]/+]+", " ", explanation)
    return reason.strip(), explanation.strip()


def check_workflow(path: Path, text: str) -> tuple[str | None, list[Finding], list[Finding]]:
    """Classify one workflow. Returns (class, findings, broken).

    `broken` means the checker could not do its job on this file, which is a different
    outcome from a file that is simply undeclared — one is exit 2, the other exit 1.
    """
    name = path.name
    try:
        document = yaml.load(text, Loader=WorkflowLoader)
    except yaml.YAMLError as error:
        first = str(error).splitlines()[0]
        return None, [], [Finding(name, "UNPARSEABLE", f"not valid YAML ({first}). The inventory is incomplete, so 'every workflow declares itself' cannot be asserted about this tree.")]
    if not isinstance(document, dict):
        return None, [], [Finding(name, "UNPARSEABLE", "top level is not a mapping; this is not a workflow file.")]

    events = triggers(document)
    workflow_class = classify(events)
    declared_name = str(document.get("name") or "")
    in_name = {k: bool(r.search(declared_name)) for k, r in NAME_MARKER_RE.items()}
    in_preamble = preamble_markers(text)
    findings: list[Finding] = []

    gate_reason = workflow_gate_reason(text)
    if gate_reason is not None and not gate_reason:
        findings.append(Finding(
            name, "GATE_DECLARATION_WITHOUT_REASON",
            "declares itself a workflow gate without saying what guarantee it protects.",
        ))
    if gate_reason is not None:
        push = document.get("on", document.get(True, {}))
        push_main = isinstance(push, dict) and "push" in push
        push_config = push.get("push") if push_main else None
        if isinstance(push_config, dict):
            branches = push_config.get("branches", [])
            push_main = "main" in branches or "**" in branches
        elif push_main:
            # `push:` with no options evaluates every branch, including main.
            push_main = True
        if not push_main:
            findings.append(Finding(
                name, "GATE_WITHOUT_MAIN_TRIGGER",
                f"declares itself a gate ({gate_reason!r}) but has no push trigger that "
                "evaluates main. A gate that only checks PRs cannot report the state of "
                "the protected branch.",
            ))

    if workflow_class == NONE:
        findings.append(Finding(
            name, "NO_TRIGGER",
            "declares no trigger at all, so nothing can ever start it — not even a "
            "human. A file that can never run reads as coverage and provides none.",
        ))
        return workflow_class, findings, []

    if workflow_class == MANUAL:
        kinds = sorted(set(in_preamble) | {k for k, v in in_name.items() if v})
        if not kinds:
            findings.append(Finding(
                name, "UNDECLARED_MANUAL",
                f"is {sorted(events)}-only — nothing starts it automatically — but "
                f"neither its name ({declared_name!r}) nor its preamble says so. Declare "
                "it: add '(manual …)' or '(parked …)' to `name:` — the string the Actions "
                "sidebar and every check listing show — and a matching "
                "'# NO-AUTOMATIC-TRIGGER: manual|parked — <why>' comment above `jobs:`. "
                "A name implying a standing gate over a workflow nothing triggers is "
                "#2890's defect.",
            ))
        elif len(kinds) > 1:
            findings.append(Finding(
                name, "AMBIGUOUS_DECLARATION",
                f"declares more than one kind ({', '.join(kinds)}). A workflow is either "
                "an on-demand tool that gates nothing by design, or a gate whose trigger "
                "is temporarily off. Claiming both leaves a reader unable to tell whether "
                "restoring it is owed.",
            ))
        else:
            kind = kinds[0]
            if not in_name[kind]:
                findings.append(Finding(
                    name, "UNDECLARED_MANUAL",
                    f"declares 'NO-AUTOMATIC-TRIGGER: {kind}' in its preamble but its name "
                    f"({declared_name!r}) carries no '({kind} …)' marker. The preamble "
                    "serves the reader who opens the file; the name serves the one who "
                    "only ever sees the Actions sidebar, and that is the reader who "
                    "mistakes it for a gate.",
                ))
            if kind not in in_preamble:
                findings.append(Finding(
                    name, "UNDECLARED_MANUAL",
                    f"carries a '({kind} …)' name marker but no matching "
                    f"'# NO-AUTOMATIC-TRIGGER: {kind} — <why>' comment above `jobs:`. A "
                    "label without a reason is a label nobody can check or retire.",
                ))
            reason, explanation = declaration_reason(kind, in_preamble.get(kind, ""))
            if kind in in_preamble and not reason:
                findings.append(Finding(
                    name, "DECLARATION_WITHOUT_REASON",
                    f"declares 'NO-AUTOMATIC-TRIGGER: {kind}' but gives no reason. The "
                    "marker says THAT the workflow gates nothing; the reason is the only "
                    "part that says why, and it is what a reader needs in order to "
                    "decide whether the state is still correct. A label nobody can check "
                    "is a label nobody can retire.",
                ))
            elif kind in in_preamble and not explanation:
                findings.append(Finding(
                    name, "DECLARATION_WITHOUT_REASON",
                    f"declares 'NO-AUTOMATIC-TRIGGER: {kind}' whose entire reason is a "
                    "restore pointer. An issue number or the parked register says where "
                    "the work is tracked, not why the trigger is off — the pointer is "
                    "required IN ADDITION to the explanation, not instead of it.",
                ))
            if kind == "parked" and not RESTORE_POINTER_RE.search(in_preamble[kind]):
                findings.append(Finding(
                    name, "PARKED_WITHOUT_RESTORE",
                    "is declared PARKED but its declaration cites no issue (#N) and no "
                    "register (docs/ci/parked-workflows.txt). A suppression with no route "
                    "back is a permanent coverage reduction wearing a temporary label — "
                    "name what restores it, or declare it MANUAL-ONLY and mean it.",
                ))
    else:
        for kind, present in in_name.items():
            if present:
                findings.append(Finding(
                    name, "STALE_DECLARATION",
                    f"carries a '({kind} …)' marker in its name ({declared_name!r}) but is "
                    f"triggered automatically by {sorted(events & AUTOMATIC_EVENTS)}. A "
                    "live gate labelled as an off one is the same misreading with the sign "
                    "flipped — remove the marker in the change that added the trigger.",
                ))
        for kind in in_preamble:
            findings.append(Finding(
                name, "STALE_DECLARATION",
                f"declares 'NO-AUTOMATIC-TRIGGER: {kind}' but IS triggered automatically "
                f"by {sorted(events & AUTOMATIC_EVENTS)}. The declaration outlived the fact "
                "it described.",
            ))
    return workflow_class, findings, []


def take_inventory(directory: Path) -> Inventory:
    inventory = Inventory()
    files = sorted(p for p in directory.glob("*.yml")) + sorted(directory.glob("*.yaml"))
    if not files:
        inventory.broken.append(Finding(
            str(directory), "EMPTY_INVENTORY",
            "no workflow files found. A check over nothing reports success while "
            "covering nothing — refusing instead.",
        ))
        return inventory
    for path in files:
        workflow_class, findings, broken = check_workflow(path, path.read_text(errors="replace"))
        inventory.findings.extend(findings)
        inventory.broken.extend(broken)
        if workflow_class == MANUAL:
            kinds = sorted(preamble_markers(path.read_text(errors="replace")))
            inventory.manual.append((path.name, kinds[0] if len(kinds) == 1 else "undeclared"))
        elif workflow_class == AUTOMATIC:
            inventory.automatic.append(path.name)
    return inventory


# ─────────────────────── the denominator instrument ───────────────────────

LIVE, GITHUB_MANAGED, BRANCH_ONLY, GHOST = "LIVE", "GITHUB_MANAGED", "BRANCH_ONLY", "GHOST"


def classify_registration(path: str, on_main: bool, on_branches: list[str]) -> str:
    """Which of the four classes a registration belongs to. Pure, so it is testable.

    The classes exist because the obvious action — "delete the registrations with no
    file" — is wrong for three of the four. `dynamic/…` are GitHub's own Dependabot
    workflows and are still running; BRANCH_ONLY entries are live unmerged work and are
    dispatchable on their own ref; and a GHOST cannot be deleted at all, because GitHub
    exposes no endpoint for it. Only the classification is actionable, and what it makes
    actionable is the DENOMINATOR, not a cleanup.
    """
    if on_main:
        return LIVE
    if path.startswith("dynamic/"):
        return GITHUB_MANAGED
    if on_branches:
        return BRANCH_ONLY
    return GHOST


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True
    ).stdout.strip()


def reconcile(repo: str, workflows_dir: Path) -> int:
    """Print the API-vs-tree reconciliation. Reporting only; never the CI gate."""
    raw = subprocess.run(
        ["gh", "api", "--paginate", f"repos/{repo}/actions/workflows?per_page=100",
         "--jq", ".workflows[] | [.id,.state,.path,.name] | @tsv"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if raw.returncode != 0 or not raw.stdout.strip():
        print(
            "INCONCLUSIVE: could not read the Actions workflow list "
            f"({raw.stderr.strip()[:200] or 'empty response'}). This says nothing about "
            "the repository — it is why --reconcile is an instrument and not a gate."
        )
        return 2

    branches = [
        line for line in _git("for-each-ref", "--format=%(refname:short)",
                              "refs/remotes/origin").splitlines() if line
    ]
    buckets: dict[str, list[tuple[str, str, str]]] = {
        LIVE: [], GITHUB_MANAGED: [], BRANCH_ONLY: [], GHOST: []
    }
    for line in raw.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < 4:
            continue
        _id, state, path, name = parts[0], parts[1], parts[2], parts[3]
        on_main = (ROOT / path).is_file()
        found_on = []
        if not on_main and not path.startswith("dynamic/"):
            for branch in branches:
                if subprocess.run(["git", "cat-file", "-e", f"{branch}:{path}"],
                                  cwd=ROOT, capture_output=True).returncode == 0:
                    found_on.append(branch)
                    if len(found_on) >= 2:
                        break
        buckets[classify_registration(path, on_main, found_on)].append(
            (path, name, state if state != "active" else ",".join(found_on))
        )

    on_disk = len(list(workflows_dir.glob("*.yml"))) + len(list(workflows_dir.glob("*.yaml")))
    total = sum(len(v) for v in buckets.values())
    print(
        f"\nActions registrations (all states): {total}    "
        f"workflow files on this tree: {on_disk}"
    )
    print(
        "The API is NOT the gate inventory. It lists REGISTRATIONS, and GitHub keeps one "
        f"after its file is deleted — {total - on_disk} of these have no file here. An "
        "audit that counts from the API sees names that correspond to nothing, which is "
        "how a coverage review starts from the wrong denominator (#2890, #2888).\n"
    )
    for bucket, blurb in (
        (LIVE, "registration has a file on this tree — this is the real inventory"),
        (GITHUB_MANAGED, "GitHub-managed dynamic workflows (Dependabot); not files, still running — never 'clean these up'"),
        (BRANCH_ONLY, "no file on this tree, but present on an unmerged branch: dispatchable there, self-corrects on merge or branch deletion"),
        (GHOST, "no file anywhere: historical runs only. Inert — cannot be dispatched, and GitHub exposes no API to delete the registration. Nothing to do but know the number"),
    ):
        rows = sorted(buckets[bucket])
        print(f"── {bucket} ({len(rows)}) — {blurb}")
        if bucket != LIVE:
            for path, name, extra in rows:
                print(f"     {path}" + (f"   [{extra}]" if extra else ""))
        print()
    return 0


# ──────────────────────────── the CONTROL ────────────────────────────

MANUAL_GOOD = """\
# NO-AUTOMATIC-TRIGGER: manual — an on-demand tool; it gates nothing.
name: Cut Verdict (manual tool)
on:
  workflow_dispatch: {}
jobs:
  a: {runs-on: ubuntu-latest, steps: [{run: "true"}]}
"""
MANUAL_NO_NAME = MANUAL_GOOD.replace("Cut Verdict (manual tool)", "Deployment Gate")
MANUAL_NO_PREAMBLE = "\n".join(MANUAL_GOOD.splitlines()[1:])
MANUAL_UNDECLARED = MANUAL_NO_NAME.replace(
    "# NO-AUTOMATIC-TRIGGER: manual — an on-demand tool; it gates nothing.\n", ""
)
MARKER_BELOW_JOBS = (
    "name: Cut Verdict (manual tool)\non:\n  workflow_dispatch: {}\n"
    "jobs:\n"
    "  # NO-AUTOMATIC-TRIGGER: manual — too far down to be what a reader sees\n"
    "  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
)
PARKED_GOOD = """\
name: Weekly Scan (parked)
on:
  # NO-AUTOMATIC-TRIGGER: parked — schedule commented out for the Actions budget (#2652);
  # register docs/ci/parked-workflows.txt.
  # schedule: [{cron: "0 4 * * 1"}]
  workflow_dispatch: {}
jobs:
  a: {runs-on: ubuntu-latest, steps: [{run: "true"}]}
"""
# #2942 review, blocking finding 1: a marker with no reason was accepted. The
# capture was `(?P<why>.*)`, which matches the empty string.
MANUAL_NO_REASON = (
    "# NO-AUTOMATIC-TRIGGER: manual\n"
    "name: Cut Verdict (manual tool)\n"
    "on:\n  workflow_dispatch: {}\n"
    "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
)
PARKED_NO_REASON = (
    "# NO-AUTOMATIC-TRIGGER: parked\n"
    "name: Weekly Scan (parked)\n"
    "on:\n  workflow_dispatch: {}\n"
    "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
)
# The narrower variant the reviewer named: the restore pointer standing in for the
# explanation. `#2695` says where the work is tracked, never why coverage is off.
PARKED_POINTER_ONLY = (
    "# NO-AUTOMATIC-TRIGGER: parked — docs/ci/parked-workflows.txt\n"
    "name: Weekly Scan (parked)\n"
    "on:\n  workflow_dispatch: {}\n"
    "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
)
PARKED_ISSUE_ONLY = PARKED_POINTER_ONLY.replace(
    "docs/ci/parked-workflows.txt", "#2695"
)
PARKED_NO_RESTORE = PARKED_GOOD.replace(
    "schedule commented out for the Actions budget (#2652);",
    "schedule commented out for the Actions budget;",
).replace("  # register docs/ci/parked-workflows.txt.\n", "")
BOTH_KINDS = PARKED_GOOD.replace(
    "  workflow_dispatch: {}", "  # NO-AUTOMATIC-TRIGGER: manual — also claims to be a tool\n  workflow_dispatch: {}"
)
AUTOMATIC_GOOD = """\
name: Real Gate
on:
  pull_request:
    paths: ['src/**']
jobs:
  a: {runs-on: ubuntu-latest, steps: [{run: "true"}]}
"""
AUTOMATIC_STALE_NAME = AUTOMATIC_GOOD.replace("Real Gate", "Real Gate (parked)")
AUTOMATIC_STALE_PREAMBLE = "# NO-AUTOMATIC-TRIGGER: manual — left behind when pull_request was added\n" + AUTOMATIC_GOOD
NO_TRIGGER = "name: Orphan\njobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
UNPARSEABLE = "name: Broken\non:\n  pull_request:\n   - [unclosed\n"
# `on:` under YAML 1.1 resolves to boolean True unless the loader is patched. If that
# guard regresses, every workflow classifies as NONE and the checker is confidently
# wrong about the whole tree, so the corpus pins it.
ON_AS_BOOL_TRAP = AUTOMATIC_GOOD
GATE_NO_MAIN = """\
name: Example gate
# WORKFLOW-GATE: protects the default branch
on:
  pull_request:
jobs:
  a: {runs-on: ubuntu-latest, steps: [{run: "true"}]}
"""
GATE_MAIN = GATE_NO_MAIN.replace(
    "  pull_request:\n", "  pull_request:\n  push:\n    branches: [main]\n"
)
GATE_NO_REASON = GATE_MAIN.replace("# WORKFLOW-GATE: protects the default branch", "# WORKFLOW-GATE:")


def self_test() -> int:
    cases: list[tuple[str, str, str | None, str | set[str] | None]] = [
        ("dispatch-only tool, fully declared", MANUAL_GOOD, MANUAL, None),
        ("parked gate, fully declared with a restore pointer", PARKED_GOOD, MANUAL, None),
        ("dispatch-only, nothing declared anywhere", MANUAL_UNDECLARED, MANUAL, "UNDECLARED_MANUAL"),
        ("declared in the preamble, but the name still reads as a gate", MANUAL_NO_NAME, MANUAL, "UNDECLARED_MANUAL"),
        ("name marked, but no reason given", MANUAL_NO_PREAMBLE, MANUAL, "UNDECLARED_MANUAL"),
        ("declaration below `jobs:` does not count", MARKER_BELOW_JOBS, MANUAL, "UNDECLARED_MANUAL"),
        ("parked with no issue and no register", PARKED_NO_RESTORE, MANUAL, "PARKED_WITHOUT_RESTORE"),
        ("manual marker with no reason at all", MANUAL_NO_REASON, MANUAL, "DECLARATION_WITHOUT_REASON"),
        # Neither a reason nor a pointer: both rules fire, and the corpus says so
        # exactly rather than settling for whichever one happens to come first.
        ("parked marker with no reason at all", PARKED_NO_REASON, MANUAL,
         {"DECLARATION_WITHOUT_REASON", "PARKED_WITHOUT_RESTORE"}),
        ("parked whose whole reason is the register", PARKED_POINTER_ONLY, MANUAL, "DECLARATION_WITHOUT_REASON"),
        ("parked whose whole reason is an issue number", PARKED_ISSUE_ONLY, MANUAL, "DECLARATION_WITHOUT_REASON"),
        ("claims to be both a tool and a parked gate", BOTH_KINDS, MANUAL, "AMBIGUOUS_DECLARATION"),
        ("automatically triggered, undeclared as it should be", AUTOMATIC_GOOD, AUTOMATIC, None),
        ("live gate with a stale name marker", AUTOMATIC_STALE_NAME, AUTOMATIC, "STALE_DECLARATION"),
        ("live gate with a stale preamble declaration", AUTOMATIC_STALE_PREAMBLE, AUTOMATIC, "STALE_DECLARATION"),
        ("declared gate without main coverage", GATE_NO_MAIN, AUTOMATIC, "GATE_WITHOUT_MAIN_TRIGGER"),
        ("declared gate with main coverage", GATE_MAIN, AUTOMATIC, None),
        ("declared gate without a reason", GATE_NO_REASON, AUTOMATIC, "GATE_DECLARATION_WITHOUT_REASON"),
        ("no trigger at all", NO_TRIGGER, NONE, "NO_TRIGGER"),
    ]

    failures = 0
    for label, text, expected_class, expected_code in cases:
        workflow_class, findings, broken = check_workflow(Path("fixture.yml"), text)
        codes = {f.code for f in findings}
        # Exact-set comparison, and the expectation may name MORE THAN ONE code. A
        # bare `parked` marker legitimately trips two (no reason AND no restore
        # pointer); accepting "at least one of" instead would let an unexpected extra
        # finding pass unnoticed, which is the looseness this corpus exists to refuse.
        expected_codes = (
            set() if expected_code is None
            else {expected_code} if isinstance(expected_code, str)
            else set(expected_code)
        )
        ok = workflow_class == expected_class and not broken and codes == expected_codes
        failures += not ok
        print(f"  [{'ok' if ok else 'SELF-TEST FAILED'}] {label}: class={workflow_class} codes={sorted(codes) or 'none'}")

    _c, _f, broken = check_workflow(Path("fixture.yml"), UNPARSEABLE)
    if not any(f.code == "UNPARSEABLE" for f in broken):
        print("  [SELF-TEST FAILED] invalid YAML must be GATE-BROKEN, not a finding")
        failures += 1
    else:
        print("  [ok] invalid YAML is GATE-BROKEN (exit 2), not a finding (exit 1)")

    # The YAML 1.1 `on:` → True trap. Assert the trigger set is actually read.
    if triggers(yaml.load(ON_AS_BOOL_TRAP, Loader=WorkflowLoader)) != {"pull_request"}:
        print("  [SELF-TEST FAILED] the `on:`-as-boolean guard regressed; every workflow "
              "would classify as NONE and the checker would be uniformly wrong")
        failures += 1
    else:
        print("  [ok] `on:` survives YAML 1.1 boolean resolution")

    # Reachability of the KINDS, not just of "some finding": every case above could have
    # produced UNDECLARED_MANUAL and a laxer check would still pass.
    produced = {
        f.code
        for _l, text, _c, _e in cases
        for f in check_workflow(Path("fixture.yml"), text)[1]
    }
    for code in ("UNDECLARED_MANUAL", "STALE_DECLARATION", "NO_TRIGGER",
                 "PARKED_WITHOUT_RESTORE", "AMBIGUOUS_DECLARATION",
                 "DECLARATION_WITHOUT_REASON"):
        if code not in produced:
            print(f"FAIL-CLOSED: the corpus never produced a {code} finding")
            failures += 1

    # An empty directory must refuse rather than report success over nothing.
    import tempfile
    with tempfile.TemporaryDirectory() as empty:
        if not take_inventory(Path(empty)).broken:
            print("FAIL-CLOSED: an empty workflow directory reported success")
            failures += 1
        else:
            print("  [ok] an empty workflow directory is GATE-BROKEN, not green")

    if failures:
        print(f"SELF-TEST FAILED ({failures}) — the observation below would be VOID")
        return 2
    print(f"SELF-TEST OK: {len(cases) + 4} cases; RED reachable for every finding code")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflows-dir", type=Path, default=DEFAULT_WORKFLOWS)
    parser.add_argument("--self-test", action="store_true", help="run the control and exit")
    parser.add_argument(
        "--reconcile", metavar="OWNER/REPO", nargs="?", const="wrfcoin/core4",
        help="ALSO print the Actions-registration reconciliation (needs gh + network). "
             "Reporting only: it never changes the exit code of the static rules.",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    inventory = take_inventory(args.workflows_dir)
    total = len(inventory.manual) + len(inventory.automatic)
    print(
        f"Workflow inventory at {args.workflows_dir}: {total} file(s) — "
        f"{len(inventory.automatic)} automatically triggered, "
        f"{len(inventory.manual)} manual-only."
    )
    if inventory.manual:
        # Split by kind, because the two mean opposite things about the future: a
        # manual tool is finished and gates nothing by design; a parked gate is a
        # coverage reduction someone still owes.
        for kind, blurb in (
            ("manual", "on-demand tools — they gate nothing, by design"),
            ("parked", "gates whose trigger is OFF — coverage owed, restoration tracked"),
            ("undeclared", "declaration missing or ambiguous"),
        ):
            rows = [n for n, k in inventory.manual if k == kind]
            if rows:
                print(f"\n  {kind.upper()} ({len(rows)}) — {blurb}")
                for name in rows:
                    print(f"      {name}")

    if args.reconcile:
        reconcile(args.reconcile, args.workflows_dir)

    if inventory.broken:
        for finding in inventory.broken:
            print(f"\nGATE BROKEN: {finding}")
        print(
            "\nThe inventory could not be read, so 'every workflow declares what it "
            "gates' is not asserted about this tree. Exit 2."
        )
        return 2
    if inventory.findings:
        print()
        for finding in inventory.findings:
            print(f"FINDING {finding}")
        print(
            f"\n{len(inventory.findings)} workflow declaration(s) disagree with their "
            "triggers. Every file parsed and was classified, so this is a finding about "
            "the directory, not about the checker. Exit 1."
        )
        return 1
    print("\nEvery workflow's declaration matches its triggers. Exit 0.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
