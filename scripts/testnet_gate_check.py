#!/usr/bin/env python3
"""Evaluate every testnet launch gate and refuse `ready` while any is missing (#2160).

A HAND-MAINTAINED CHECKLIST DRIFTS. Someone reads it once, checks a box, and the
box never gets re-evaluated against the thing it claims. This script re-derives
every MACHINE gate from live evidence on each run — an open critical issue, a red
CI run, a script exit code, a config value — so a gate cannot stay green by
inertia after the thing it measures goes red.

THE SPLIT THIS SCRIPT ENFORCES IN CODE, NOT IN PROSE
-----------------------------------------------------
A judgement call ("has the audit remediation actually been retested?", "is the
disaster-recovery exercise good enough?") is not a fact a script can derive from
a git diff or an API response. Encoding a guess for it is worse than leaving it
blank, because a wrong guess LOOKS like evidence. So:

  MACHINE gate: this script computes `pass` / `fail` itself, from a live source
                (an API call, a subprocess exit code, a file's content). It may
                say `missing` if the source itself could not be read, but it
                never accepts a human's word for what it can check itself.

  HUMAN gate:   a person attests, in `docs/security/attestations.yaml`, that a
                specific gate id is satisfied, and names themselves, a date and
                an evidence URL for it. This script only ever reads that file —
                it never sets `attested_by` to anything, and it never promotes a
                human gate to `pass` because a machine signal happened to look
                favourable. `pass` requires attestation, always.

`ready` is `true` only when every gate in the active set is `pass`. A `missing`
machine gate (its evidence source could not be read) and a `missing` or `stale`
human gate both force `ready: false` — the same as an outright `fail`. The
distinction between `fail`/`missing`/`stale` is preserved in the JSON purely for
someone deciding what to do next; none of the three is "close enough."

USAGE
-----
    python3 scripts/testnet_gate_check.py                 # evaluate, print JSON, exit 0/1
    python3 scripts/testnet_gate_check.py --pretty         # indented JSON
    python3 scripts/testnet_gate_check.py --self-test      # fixtures only, no repo/network I/O
    python3 scripts/testnet_gate_check.py --gate-set incentivized   # #2165's higher bar

Exit code is 0 when `ready` is true, 1 otherwise (including `--self-test` failure).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

PROJECT_ROOT = Path(__file__).resolve().parent.parent

ATTESTATIONS_PATH = PROJECT_ROOT / "docs" / "security" / "attestations.yaml"
DEFAULT_MAX_ATTESTATION_AGE_DAYS = 30

PASS = "pass"
FAIL = "fail"
MISSING = "missing"
STALE = "stale"

MACHINE = "machine"
HUMAN = "human"


@dataclass
class GateResult:
    id: str
    kind: str  # MACHINE | HUMAN
    status: str  # PASS | FAIL | MISSING | STALE
    evidence: str
    attested_by: str | None = None
    attested_at: str | None = None
    evidence_url: str | None = None

    def to_dict(self) -> dict:
        d = {"id": self.id, "kind": self.kind, "status": self.status, "evidence": self.evidence}
        if self.kind == HUMAN:
            d["attested_by"] = self.attested_by
            d["attested_at"] = self.attested_at
            d["evidence_url"] = self.evidence_url
        return d


class EvidenceUnavailable(Exception):
    """A gate's evidence source could not be read.

    Raised, never returned as a falsy value. A caller that caught a failed `gh`
    call or a missing file and quietly treated it as "gate not applicable" would
    reproduce the exact defect #2923 already found in this repo's other gate
    scripts (`check_void_gates.py`): a failed READ of evidence is not the same
    fact as evidence that says PASS is false, and conflating them lets a broken
    evidence source silently report `ready: true`. Every evidence source in this
    file raises this on failure; every gate function catches it and reports
    `missing`, never `pass`.
    """


def run(cmd: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise EvidenceUnavailable(f"{' '.join(cmd)}: {exc}") from exc


def gh_json(args: list[str]) -> object:
    result = run(["gh", *args])
    if result.returncode != 0:
        raise EvidenceUnavailable(f"gh {' '.join(args)} exited {result.returncode}: {result.stderr.strip()}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise EvidenceUnavailable(f"gh {' '.join(args)}: malformed JSON: {exc}") from exc


# ---------------------------------------------------------------------------
# Machine gates — each is a pure function of an evidence blob, testable without
# a subprocess or network call. The `gather_*` functions below do the real I/O
# and are the only place that talks to `gh`, a subprocess or the filesystem for
# their gate; `--self-test` never calls them.
# ---------------------------------------------------------------------------


def gate_open_critical_issues(open_issue_numbers: list[int]) -> GateResult:
    if open_issue_numbers:
        listing = ", ".join(f"#{n}" for n in open_issue_numbers[:10])
        more = f" (+{len(open_issue_numbers) - 10} more)" if len(open_issue_numbers) > 10 else ""
        return GateResult(
            "no-open-critical-issues", MACHINE, FAIL,
            f"{len(open_issue_numbers)} open protocol-critical/high-priority issue(s): {listing}{more}",
        )
    return GateResult("no-open-critical-issues", MACHINE, PASS, "no open protocol-critical or high-priority issue")


def gate_ci_workflow_green(gate_id: str, workflow: str, conclusion: str | None, run_url: str) -> GateResult:
    if conclusion is None:
        return GateResult(gate_id, MACHINE, MISSING, f"{workflow}: no completed run found")
    if conclusion == "success":
        return GateResult(gate_id, MACHINE, PASS, f"{workflow} last run succeeded: {run_url}")
    return GateResult(gate_id, MACHINE, FAIL, f"{workflow} last run concluded {conclusion!r}: {run_url}")


def gate_script_exit_code(gate_id: str, label: str, returncode: int | None, detail: str) -> GateResult:
    if returncode is None:
        return GateResult(gate_id, MACHINE, MISSING, f"{label}: could not run — {detail}")
    if returncode == 0:
        return GateResult(gate_id, MACHINE, PASS, f"{label}: exit 0")
    return GateResult(gate_id, MACHINE, FAIL, f"{label}: exit {returncode} — {detail}")


def gate_doc_freshness(gate_id: str, path: str, last_commit_epoch: int | None, max_age_days: int, now: float) -> GateResult:
    if last_commit_epoch is None:
        return GateResult(gate_id, MACHINE, MISSING, f"{path}: not found or has no commit history")
    age_days = (now - last_commit_epoch) / 86400.0
    if age_days > max_age_days:
        return GateResult(
            gate_id, MACHINE, FAIL,
            f"{path}: last touched {age_days:.0f}d ago, exceeds the {max_age_days}d freshness ceiling",
        )
    return GateResult(gate_id, MACHINE, PASS, f"{path}: last touched {age_days:.0f}d ago")


# Reserved identity values that must NEVER be the resolved identity of a
# testnet deployment. `1` is the cross-ecosystem convention for "mainnet" (the
# hazard this mirrors: #2713/#3120 found a scorer that silently signed under a
# `mainnet` default when its env was unset — this gate is the launch-time
# version of that same check, run against what a deployment WOULD resolve to).
RESERVED_MAINNET_CHAIN_IDS = {"1", "mainnet"}
RESERVED_MAINNET_NETWORK_IDS = {"mainnet", "main"}


def gate_testnet_identity_diff(chain_id: str | None, network_id: str | None, genesis_hash: str | None) -> GateResult:
    if chain_id is None or network_id is None or genesis_hash is None:
        return GateResult(
            "testnet-identity-not-mainnet", MACHINE, MISSING,
            "chain_id/network_id/genesis_hash not resolvable — pass --chain-id/--network-id/--genesis-hash "
            "or set WRFCOIN_CHAIN_ID/WRFCOIN_NETWORK_ID/WRFCOIN_GENESIS_HASH for the target deployment",
        )
    if chain_id.strip().lower() in RESERVED_MAINNET_CHAIN_IDS or network_id.strip().lower() in RESERVED_MAINNET_NETWORK_IDS:
        return GateResult(
            "testnet-identity-not-mainnet", MACHINE, FAIL,
            f"resolved identity IS a reserved mainnet identity (chain_id={chain_id!r}, network_id={network_id!r}) "
            "— this deployment would sign and accept mainnet-domain objects",
        )
    if not re.fullmatch(r"[0-9a-fA-F]{64}", genesis_hash or ""):
        return GateResult(
            "testnet-identity-not-mainnet", MACHINE, FAIL,
            f"genesis_hash {genesis_hash!r} is not a well-formed 64-hex-char hash",
        )
    return GateResult(
        "testnet-identity-not-mainnet", MACHINE, PASS,
        f"chain_id={chain_id!r} network_id={network_id!r} genesis_hash={genesis_hash[:12]}… — none match a reserved mainnet identity",
    )


def gate_faucet_rpc_limit_probe(probe_result: dict | None) -> GateResult:
    """`probe_result` is `None` when no live target was configured — this gate
    is never faked to `pass` by omission; an unconfigured probe is `missing`.
    """
    if probe_result is None:
        return GateResult(
            "faucet-rpc-limit-rejects-over-limit", MACHINE, MISSING,
            "no live faucet/RPC target configured (--faucet-url / --rpc-url or "
            "WRFCOIN_GATE_FAUCET_URL / WRFCOIN_GATE_RPC_URL) — this gate requires a live probe "
            "against a running testnet endpoint and this script does not invent one",
        )
    if probe_result.get("rejected") is True:
        return GateResult(
            "faucet-rpc-limit-rejects-over-limit", MACHINE, PASS,
            f"over-limit request against {probe_result.get('url')} was rejected "
            f"(status {probe_result.get('status')})",
        )
    return GateResult(
        "faucet-rpc-limit-rejects-over-limit", MACHINE, FAIL,
        f"over-limit request against {probe_result.get('url')} was NOT rejected "
        f"(status {probe_result.get('status')}) — the rate/amount limit is not enforced",
    )


# ---------------------------------------------------------------------------
# Human gates — read-only from docs/security/attestations.yaml. This script
# never writes that file and never infers a human gate's status from a machine
# signal; the fields below are exactly what the story requires.
# ---------------------------------------------------------------------------


def parse_attestations_yaml(text: str) -> tuple[dict[str, dict], list[str]]:
    """A tiny, dependency-free YAML-subset parser for this one file's shape.

    Deliberately not a real YAML parser: this repo's Python tooling elsewhere
    avoids adding a PyYAML dependency for one file (the same call #3104 made
    for the key-custody manifest, which used JSON instead). `attestations.yaml`
    stays YAML because a human edits it by hand and a list of small objects
    reads better as YAML than as JSON with trailing-comma foot-guns; the format
    is fixed and small enough that a real parser is not worth the dependency.

    Shape (a top-level list of mappings, 2-space indented):

        - gate_id: incentivized-bug-bounty-live
          attested_by: alice
          attested_at: "2026-09-01"
          evidence_url: "https://example/bounty"

    Returns (entries keyed by gate_id, parse error messages). A gate_id that
    repeats is a parse error — an attestation is at most one thing, and a
    duplicate is far more likely a copy-paste mistake overwriting the intended
    values than a deliberate re-attestation.
    """
    entries: dict[str, dict] = {}
    errors: list[str] = []
    current: dict | None = None
    required = ("gate_id", "attested_by", "attested_at", "evidence_url")
    for lineno, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.rstrip("\n")
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith("- "):
            if current is not None:
                _finish_entry(current, entries, errors, required)
            current = {}
            stripped = stripped[2:]
            if not stripped:
                continue
        if current is None:
            errors.append(f"line {lineno}: content outside a list item: {raw_line!r}")
            continue
        if ":" not in stripped:
            errors.append(f"line {lineno}: expected `key: value`, got {raw_line!r}")
            continue
        key, _, value = stripped.partition(":")
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        current[key] = value
    if current is not None:
        _finish_entry(current, entries, errors, required)
    return entries, errors


def _finish_entry(entry: dict, entries: dict[str, dict], errors: list[str], required: tuple[str, ...]) -> None:
    missing_fields = [f for f in required if not entry.get(f)]
    if missing_fields:
        errors.append(f"entry {entry!r} is missing required field(s): {', '.join(missing_fields)}")
        return
    gate_id = entry["gate_id"]
    if gate_id in entries:
        errors.append(f"gate_id {gate_id!r} is attested more than once — the later entry would silently win")
        return
    entries[gate_id] = entry


def gate_human_attestation(gate_id: str, entries: dict[str, dict], parse_errors: list[str], max_age_days: int, now_epoch: float) -> GateResult:
    if parse_errors:
        return GateResult(
            gate_id, HUMAN, MISSING,
            f"{ATTESTATIONS_PATH.name} did not parse cleanly, refusing to trust any entry in it: "
            + "; ".join(parse_errors[:3]),
        )
    entry = entries.get(gate_id)
    if entry is None:
        return GateResult(
            gate_id, HUMAN, MISSING,
            f"no attestation for {gate_id!r} in {ATTESTATIONS_PATH.name}",
        )
    attested_at_raw = entry["attested_at"]
    try:
        import calendar

        attested_epoch = calendar.timegm(time.strptime(attested_at_raw[:10], "%Y-%m-%d"))
    except ValueError:
        return GateResult(
            gate_id, HUMAN, MISSING,
            f"attested_at {attested_at_raw!r} for {gate_id!r} is not an ISO date (YYYY-MM-DD)",
            attested_by=entry["attested_by"], attested_at=attested_at_raw, evidence_url=entry["evidence_url"],
        )
    age_days = (now_epoch - attested_epoch) / 86400.0
    if age_days > max_age_days:
        return GateResult(
            gate_id, HUMAN, STALE,
            f"attested {age_days:.0f}d ago by {entry['attested_by']}, exceeds the {max_age_days}d ceiling — "
            "a stale attestation is not a pass, it must be re-attested",
            attested_by=entry["attested_by"], attested_at=attested_at_raw, evidence_url=entry["evidence_url"],
        )
    return GateResult(
        gate_id, HUMAN, PASS,
        f"attested {age_days:.0f}d ago by {entry['attested_by']}",
        attested_by=entry["attested_by"], attested_at=attested_at_raw, evidence_url=entry["evidence_url"],
    )


# ---------------------------------------------------------------------------
# Gate sets. `testnet` is #2160's launch bar; `incentivized` (#2165) is strictly
# additive on top of it — nothing in `testnet` is removed or weakened when the
# higher bar is selected.
# ---------------------------------------------------------------------------

HUMAN_GATE_IDS_TESTNET = [
    "incident-response-plan-reviewed",
    "disaster-recovery-runbook-reviewed",
    "key-ceremony-witnessed",
    "mobile-launch-criteria-met",
]

HUMAN_GATE_IDS_INCENTIVIZED = [
    "external-audit-remediated-and-retested",
    "public-bug-bounty-live",
    "extended-fuzzing-campaign-quantified",
    "validator-key-rotation-exercised",
    "multiparty-upgrade-key-custody-in-place",
    "disaster-recovery-exercise-timed",
    "adversarial-validator-ran-against-network",
    "extended-run-without-unplanned-reset",
]


@dataclass
class GateInputs:
    """Everything a real evaluation needs, gathered once so the pure gate
    functions above never do I/O themselves. `--self-test` builds one of these
    by hand from fixtures instead of calling `gather_inputs()`.
    """

    open_critical_issue_numbers: list[int] = field(default_factory=list)
    fuzz_conclusion: str | None = None
    fuzz_run_url: str = ""
    supply_chain_conclusion: str | None = None
    supply_chain_run_url: str = ""
    panic_lint_returncode: int | None = None
    panic_lint_detail: str = ""
    determinism_returncode: int | None = None
    determinism_detail: str = ""
    threat_model_commit_epoch: int | None = None
    protocol_spec_commit_epoch: int | None = None
    chain_id: str | None = None
    network_id: str | None = None
    genesis_hash: str | None = None
    faucet_probe_result: dict | None = None
    attestation_entries: dict[str, dict] = field(default_factory=dict)
    attestation_parse_errors: list[str] = field(default_factory=list)
    now_epoch: float = 0.0
    max_attestation_age_days: int = DEFAULT_MAX_ATTESTATION_AGE_DAYS
    doc_freshness_max_age_days: int = 180


def gather_inputs(args: argparse.Namespace) -> GateInputs:
    """The only function in this file that talks to `gh`, a subprocess, the
    filesystem or the network. Every failure is caught locally and turned into
    a `None`/empty value on the returned struct, which the gate functions then
    report as `missing` — never as `pass`.
    """
    inputs = GateInputs(now_epoch=time.time(), max_attestation_age_days=args.max_attestation_age_days)

    try:
        payload = gh_json([
            "issue", "list", "--repo", "wrfcoin/core4", "--state", "open",
            "--search", "label:protocol-critical,high-priority,priority-high",
            "--json", "number",
        ])
        inputs.open_critical_issue_numbers = sorted(item["number"] for item in payload)
    except EvidenceUnavailable:
        inputs.open_critical_issue_numbers = []
        inputs._open_critical_unavailable = True  # type: ignore[attr-defined]

    for wf, conclusion_attr, url_attr in (
        ("fuzz-nightly.yml", "fuzz_conclusion", "fuzz_run_url"),
        ("supply-chain-security.yml", "supply_chain_conclusion", "supply_chain_run_url"),
    ):
        try:
            runs = gh_json([
                "run", "list", "--repo", "wrfcoin/core4", "--workflow", wf, "--branch", "main",
                "--limit", "1", "--json", "conclusion,url,status",
            ])
            completed = [r for r in runs if r.get("status") == "completed"]
            if completed:
                setattr(inputs, conclusion_attr, completed[0]["conclusion"])
                setattr(inputs, url_attr, completed[0]["url"])
        except EvidenceUnavailable:
            pass

    for cmd, rc_attr, detail_attr, label in (
        (["python3", "scripts/check_no_panic_on_input.py"], "panic_lint_returncode", "panic_lint_detail", "panic-input lint"),
        (["python3", "scripts/check_determinism.py"], "determinism_returncode", "determinism_detail", "determinism lint"),
    ):
        try:
            result = run([*cmd], timeout=600)
            setattr(inputs, rc_attr, result.returncode)
            tail = (result.stdout or result.stderr or "").strip().splitlines()
            setattr(inputs, detail_attr, tail[-1] if tail else f"{label} ran with no output")
        except EvidenceUnavailable as exc:
            setattr(inputs, detail_attr, str(exc))

    for path, attr in (
        (PROJECT_ROOT / "docs" / "security" / "THREAT_MODEL.md", "threat_model_commit_epoch"),
        (PROJECT_ROOT / "docs" / "architecture" / "PROTOCOL_SPEC.md", "protocol_spec_commit_epoch"),
        (PROJECT_ROOT / "docs" / "security" / "PROTOCOL_SPEC.md", "protocol_spec_commit_epoch"),
    ):
        if not path.exists():
            continue
        try:
            result = run(["git", "log", "-1", "--format=%at", "--", str(path.relative_to(PROJECT_ROOT))])
            if result.returncode == 0 and result.stdout.strip():
                setattr(inputs, attr, int(result.stdout.strip()))
        except (EvidenceUnavailable, ValueError):
            pass

    inputs.chain_id = args.chain_id or _env("WRFCOIN_CHAIN_ID")
    inputs.network_id = args.network_id or _env("WRFCOIN_NETWORK_ID")
    inputs.genesis_hash = args.genesis_hash or _env("WRFCOIN_GENESIS_HASH")

    if args.faucet_url or args.rpc_url or _env("WRFCOIN_GATE_FAUCET_URL") or _env("WRFCOIN_GATE_RPC_URL"):
        inputs.faucet_probe_result = probe_over_limit_request(
            args.faucet_url or _env("WRFCOIN_GATE_FAUCET_URL"),
            args.rpc_url or _env("WRFCOIN_GATE_RPC_URL"),
        )

    if ATTESTATIONS_PATH.exists():
        entries, errors = parse_attestations_yaml(ATTESTATIONS_PATH.read_text(encoding="utf-8"))
        inputs.attestation_entries = entries
        inputs.attestation_parse_errors = errors
    else:
        inputs.attestation_parse_errors = [f"{ATTESTATIONS_PATH} does not exist"]

    return inputs


def _env(name: str) -> str | None:
    import os

    return os.environ.get(name) or None


def probe_over_limit_request(url: str | None, rpc_url: str | None) -> dict:
    """A real HTTP probe against a live faucet/RPC endpoint.

    Not invoked unless a target is explicitly configured (see `gather_inputs`)
    — this script never guesses at a live node's address, and this loop's own
    rules forbid touching a live node from a build/review session. An operator
    running this gate for real, against their own testnet, supplies the URL.
    """
    import urllib.error
    import urllib.request

    target = url or rpc_url
    body = b'{"probe":"over-limit","amount":999999999999}'
    req = urllib.request.Request(target, data=body, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            status = resp.status
    except urllib.error.HTTPError as exc:
        status = exc.code
    except (urllib.error.URLError, OSError) as exc:
        return {"url": target, "status": None, "rejected": False, "error": str(exc)}
    return {"url": target, "status": status, "rejected": status in (400, 402, 403, 413, 422, 429)}


def evaluate(inputs: GateInputs, gate_set: str) -> list[GateResult]:
    results: list[GateResult] = []

    if getattr(inputs, "_open_critical_unavailable", False):
        results.append(GateResult("no-open-critical-issues", MACHINE, MISSING, "gh issue search failed — see stderr"))
    else:
        results.append(gate_open_critical_issues(inputs.open_critical_issue_numbers))

    results.append(gate_ci_workflow_green("fuzz-campaign-green", "fuzz-nightly.yml", inputs.fuzz_conclusion, inputs.fuzz_run_url))
    results.append(gate_ci_workflow_green("supply-chain-clean", "supply-chain-security.yml", inputs.supply_chain_conclusion, inputs.supply_chain_run_url))
    results.append(gate_script_exit_code("panic-policy-clean", "check_no_panic_on_input.py", inputs.panic_lint_returncode, inputs.panic_lint_detail))
    results.append(gate_script_exit_code("determinism-scan-clean", "check_determinism.py", inputs.determinism_returncode, inputs.determinism_detail))
    results.append(gate_doc_freshness("threat-model-current", "docs/security/THREAT_MODEL.md", inputs.threat_model_commit_epoch, inputs.doc_freshness_max_age_days, inputs.now_epoch))
    results.append(gate_doc_freshness("protocol-spec-current", "docs/architecture/PROTOCOL_SPEC.md", inputs.protocol_spec_commit_epoch, inputs.doc_freshness_max_age_days, inputs.now_epoch))
    results.append(gate_testnet_identity_diff(inputs.chain_id, inputs.network_id, inputs.genesis_hash))
    results.append(gate_faucet_rpc_limit_probe(inputs.faucet_probe_result))

    human_ids = list(HUMAN_GATE_IDS_TESTNET)
    if gate_set == "incentivized":
        human_ids += HUMAN_GATE_IDS_INCENTIVIZED
    for gid in human_ids:
        results.append(gate_human_attestation(gid, inputs.attestation_entries, inputs.attestation_parse_errors, inputs.max_attestation_age_days, inputs.now_epoch))

    return results


def render(results: list[GateResult], pretty: bool) -> tuple[dict, bool]:
    ready = all(r.status == PASS for r in results)
    payload = {"ready": ready, "gates": [r.to_dict() for r in results]}
    return payload, ready


# ---------------------------------------------------------------------------
# Self-test: fixtures only. Never touches `gh`, a subprocess, the network or
# the real `docs/security/attestations.yaml`.
# ---------------------------------------------------------------------------


def self_test() -> list[str]:
    failures: list[str] = []
    now = 1_800_000_000.0  # fixed instant so day-math below is exact and reproducible
    fresh_attested_at = time.strftime("%Y-%m-%d", time.gmtime(now - 5 * 86400))

    # --- all pass ---
    all_pass_inputs = GateInputs(
        open_critical_issue_numbers=[],
        fuzz_conclusion="success", fuzz_run_url="https://example/run/1",
        supply_chain_conclusion="success", supply_chain_run_url="https://example/run/2",
        panic_lint_returncode=0, panic_lint_detail="ok",
        determinism_returncode=0, determinism_detail="ok",
        threat_model_commit_epoch=now - 10 * 86400,
        protocol_spec_commit_epoch=now - 10 * 86400,
        chain_id="777", network_id="testnet", genesis_hash="a" * 64,
        faucet_probe_result={"url": "https://faucet.example/claim", "status": 429, "rejected": True},
        attestation_entries={
            gid: {"gate_id": gid, "attested_by": "alice", "attested_at": fresh_attested_at, "evidence_url": "https://example/ev"}
            for gid in HUMAN_GATE_IDS_TESTNET
        },
        attestation_parse_errors=[],
        now_epoch=now,
    )
    payload, ready = render(evaluate(all_pass_inputs, "testnet"), pretty=False)
    if not ready:
        failures.append(f"SELF_TEST_FAILED: an all-pass fixture must report ready=true, got {payload}")

    # --- one machine fail (open critical issue) forces ready=false ---
    machine_fail_inputs = GateInputs(**{**all_pass_inputs.__dict__, "open_critical_issue_numbers": [4242]})
    _, ready = render(evaluate(machine_fail_inputs, "testnet"), pretty=False)
    if ready:
        failures.append("SELF_TEST_FAILED: one open protocol-critical issue must force ready=false")

    # --- missing human attestation forces ready=false, and is reported as MISSING not FAIL ---
    missing_attestation_inputs = GateInputs(**{**all_pass_inputs.__dict__, "attestation_entries": {
        gid: e for gid, e in all_pass_inputs.attestation_entries.items() if gid != "key-ceremony-witnessed"
    }})
    results = evaluate(missing_attestation_inputs, "testnet")
    kc = next(r for r in results if r.id == "key-ceremony-witnessed")
    if kc.status != MISSING:
        failures.append(f"SELF_TEST_FAILED: an absent attestation must be MISSING, got {kc.status!r}")
    _, ready = render(results, pretty=False)
    if ready:
        failures.append("SELF_TEST_FAILED: a missing human attestation must force ready=false")

    # --- stale human attestation forces ready=false, distinctly from missing ---
    stale_entries = dict(all_pass_inputs.attestation_entries)
    stale_entries["key-ceremony-witnessed"] = {
        "gate_id": "key-ceremony-witnessed", "attested_by": "bob",
        "attested_at": "2020-01-01", "evidence_url": "https://example/old",
    }
    stale_inputs = GateInputs(**{**all_pass_inputs.__dict__, "attestation_entries": stale_entries})
    results = evaluate(stale_inputs, "testnet")
    kc = next(r for r in results if r.id == "key-ceremony-witnessed")
    if kc.status != STALE:
        failures.append(f"SELF_TEST_FAILED: an attestation older than the ceiling must be STALE, got {kc.status!r}")
    _, ready = render(results, pretty=False)
    if ready:
        failures.append("SELF_TEST_FAILED: a stale human attestation must force ready=false")

    # --- a machine signal must never promote a human gate to pass ---
    # (structural check: gate_human_attestation takes no machine-derived argument at all)
    import inspect

    sig = inspect.signature(gate_human_attestation)
    machine_looking_params = {"conclusion", "returncode", "probe_result"} & set(sig.parameters)
    if machine_looking_params:
        failures.append(
            "SELF_TEST_FAILED: gate_human_attestation must not accept a machine-derived parameter "
            f"(found {machine_looking_params}) — a human gate may be read only from the attestation file"
        )

    # --- an unparseable attestations file makes every human gate MISSING, not silently skipped ---
    broken_inputs = GateInputs(**{**all_pass_inputs.__dict__, "attestation_parse_errors": ["line 3: bad"]})
    results = evaluate(broken_inputs, "testnet")
    human_results = [r for r in results if r.kind == HUMAN]
    if not human_results or any(r.status != MISSING for r in human_results):
        failures.append("SELF_TEST_FAILED: a parse-broken attestations file must make every human gate MISSING")

    # --- the reserved-mainnet-identity gate actually refuses a mainnet-shaped config ---
    mainnet_shaped = gate_testnet_identity_diff("1", "mainnet", "a" * 64)
    if mainnet_shaped.status != FAIL:
        failures.append("SELF_TEST_FAILED: chain_id=1/network_id=mainnet must FAIL the identity-diff gate")
    unresolved = gate_testnet_identity_diff(None, None, None)
    if unresolved.status != MISSING:
        failures.append("SELF_TEST_FAILED: an unresolved identity must be MISSING, never silently PASS")

    # --- the faucet probe gate is MISSING (not PASS) when nothing was configured ---
    unconfigured_probe = gate_faucet_rpc_limit_probe(None)
    if unconfigured_probe.status != MISSING:
        failures.append("SELF_TEST_FAILED: an unconfigured live probe must be MISSING, never PASS-by-omission")

    # --- incentivized gate set is strictly additive over testnet ---
    testnet_ids = {r.id for r in evaluate(all_pass_inputs, "testnet") if r.kind == HUMAN}
    incentivized_ids = {r.id for r in evaluate(all_pass_inputs, "incentivized") if r.kind == HUMAN}
    if not testnet_ids.issubset(incentivized_ids):
        failures.append("SELF_TEST_FAILED: the incentivized gate set must be a superset of the testnet gate set")

    return failures


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="run fixtures only, no repo/network I/O")
    parser.add_argument("--pretty", action="store_true", help="indent the JSON output")
    parser.add_argument("--gate-set", choices=["testnet", "incentivized"], default="testnet")
    parser.add_argument("--max-attestation-age-days", type=int, default=DEFAULT_MAX_ATTESTATION_AGE_DAYS)
    parser.add_argument("--chain-id", default=None, help="override WRFCOIN_CHAIN_ID for the identity-diff gate")
    parser.add_argument("--network-id", default=None, help="override WRFCOIN_NETWORK_ID for the identity-diff gate")
    parser.add_argument("--genesis-hash", default=None, help="override WRFCOIN_GENESIS_HASH for the identity-diff gate")
    parser.add_argument("--faucet-url", default=None, help="live faucet endpoint to probe (opt-in; never guessed)")
    parser.add_argument("--rpc-url", default=None, help="live RPC endpoint to probe (opt-in; never guessed)")
    args = parser.parse_args(argv)

    if args.self_test:
        failures = self_test()
        for f in failures:
            print(f, file=sys.stderr)
        if failures:
            print(f"SELF-TEST FAILED: {len(failures)} failure(s)", file=sys.stderr)
            return 1
        print("SELF-TEST OK")
        return 0

    inputs = gather_inputs(args)
    results = evaluate(inputs, args.gate_set)
    payload, ready = render(results, args.pretty)
    print(json.dumps(payload, indent=2 if args.pretty else None))
    return 0 if ready else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
