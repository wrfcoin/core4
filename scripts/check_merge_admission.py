#!/usr/bin/env python3
"""Fail when the live branch ruleset stops matching the written merge-admission contract.

WHY THIS EXISTS (merge-admission epic, story 7; anchors #2896 #2960 #3005)
--------------------------------------------------------------------------
`docs/ci/MERGE_ADMISSION.md` says what must be true before a commit reaches
`main`. A document like that is worth exactly as much as its agreement with the
ruleset GitHub actually enforces, and nothing keeps them agreeing: rulesets are
edited in a web form by the owner, the document is edited in PRs by everyone
else, and neither edit is visible from the other side. On 2026-08-31 the repo
went from zero required contexts to three in one owner action; #2960 and #2711
reasoned from the old state for hours afterwards because the only artifact they
could read (a 404 from legacy `branches/main/protection`) still said "nothing".

So: `.github/merge-admission.json` records what the owner set, this script reads
the LIVE ruleset and fails when the two differ, and it also fails when the
document stops naming a required context verbatim. Drift in either direction is
RED. Nothing here edits a ruleset — that stays owner-only.

WHAT IS COMPARED, per repo in the manifest
------------------------------------------
From `GET repos/<repo>/rules/branches/main` (the effective rules; this is the
endpoint that answers "what gates a merge", unlike legacy branch protection):

  * the SET of rule types contributed by the manifest's ruleset id
  * rules contributed by any OTHER ruleset id (unexpected — reported as drift)
  * the SET of required status contexts (order-insensitive)
  * `strict_required_status_checks_policy` (None when there is no status rule)
  * `pull_request` parameters the manifest names (approving-review count,
    allowed merge methods)

From `GET repos/<repo>/rulesets/<id>`: name, `enforcement == active`, and bypass
actors. **This read fails closed.** The manifest asserts those three fields, so
if the endpoint answers anything but 200 with a JSON object — 403 for a token
without the permission, 404, a network error, a malformed body — the check is
VOID (exit 2), never OK. An earlier revision printed "UNVERIFIED" and exited 0
here; the cross-provider review on #3018 correctly rejected that as a checker
that passes when it is blind, which converts an unknown into false reassurance.
If the workflow token cannot read this endpoint, the right fix is a token that
can, not a softer check.

From the document: every required context must appear verbatim inside
backticks, and the ruleset id must appear.

EXIT CODES
----------
  0  contract holds for every repo checked
  1  DRIFT — at least one difference; each is printed as one line
  2  VOID  — could not evaluate (manifest/document missing or unparseable, API
     unreadable). Not a pass and not a failure (docs/ci/MERGE_ADMISSION.md §3).

`--self-test` runs the comparison against synthetic fixtures in both directions
(identical => 0, each single mutation => 1, unreadable => 2) so a green from the
live check is known to come from a comparator that can go red
(docs/ci/VACUOUS_GATES.md, check 1). `--fixture <json>` substitutes a recorded
API payload for the live one; scripts/test_check_merge_admission.py uses it.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / ".github" / "merge-admission.json"
DEFAULT_DOC = ROOT / "docs" / "ci" / "MERGE_ADMISSION.md"

EXIT_OK = 0
EXIT_DRIFT = 1
EXIT_VOID = 2


class Void(Exception):
    """The check cannot be evaluated. Not evidence in either direction."""


# --------------------------------------------------------------------------- API

def gh_json(path: str) -> object:
    result = subprocess.run(["gh", "api", path], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise Void(f"{path}: gh exited {result.returncode}: {result.stderr.strip()[:200]}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise Void(f"{path}: malformed JSON ({exc})") from exc
    if payload is None or not isinstance(payload, (dict, list)):
        raise Void(f"{path}: body is not an object or array")
    return payload


def gh_status(path: str) -> tuple[int, object | None]:
    """Like gh_json but returns (http_status, payload|None) instead of raising on 4xx."""
    result = subprocess.run(
        ["gh", "api", path, "--include"], capture_output=True, text=True, check=False
    )
    head, _, body = result.stdout.partition("\r\n\r\n")
    if not body:
        head, _, body = result.stdout.partition("\n\n")
    match = re.match(r"HTTP/\S+\s+(\d{3})", head)
    status = int(match.group(1)) if match else (200 if result.returncode == 0 else 0)
    if status != 200:
        return status, None
    try:
        payload = json.loads(body)
    except json.JSONDecodeError:
        return status, None
    return status, payload


def fetch_live(repo: str, ruleset_id: int) -> dict:
    rules = gh_json(f"repos/{repo}/rules/branches/main")
    if not isinstance(rules, list):
        raise Void(f"repos/{repo}/rules/branches/main: expected an array")
    status, ruleset = gh_status(f"repos/{repo}/rulesets/{ruleset_id}")
    if status != 200 or not isinstance(ruleset, dict):
        # FAIL CLOSED. The manifest asserts name/enforcement/bypass_actors from
        # this endpoint; without it those assertions cannot be evaluated.
        raise Void(f"repos/{repo}/rulesets/{ruleset_id}: HTTP {status}, no JSON object — "
                   f"ruleset metadata (name, enforcement, bypass_actors) cannot be verified")
    return {"rules": rules, "ruleset": ruleset, "ruleset_http": status}


# ---------------------------------------------------------------------- compare

def live_view(live: dict, expected_id: int) -> dict:
    rules = live.get("rules")
    if not isinstance(rules, list):
        raise Void("fixture/live payload has no `rules` array")
    own = [r for r in rules if r.get("ruleset_id") == expected_id]
    foreign = sorted({(r.get("type"), r.get("ruleset_id")) for r in rules
                      if r.get("ruleset_id") != expected_id})
    contexts = None
    strict = None
    pr_params: dict = {}
    for rule in own:
        params = rule.get("parameters") or {}
        if rule.get("type") == "required_status_checks":
            contexts = sorted(c.get("context") for c in params.get("required_status_checks", []))
            strict = params.get("strict_required_status_checks_policy")
        if rule.get("type") == "pull_request":
            pr_params = params
    ruleset = live.get("ruleset")
    if not isinstance(ruleset, dict):
        # Fixture path mirrors fetch_live: metadata absent/malformed is VOID.
        raise Void("ruleset metadata absent or malformed — name, enforcement and "
                   "bypass_actors cannot be verified (HTTP %s)" % live.get("ruleset_http"))
    return {
        "rule_types": sorted({r.get("type") for r in own}),
        "foreign_rules": foreign,
        "required_status_checks": contexts if contexts is not None else [],
        "has_status_rule": contexts is not None,
        "strict_required_status_checks_policy": strict,
        "pull_request": pr_params,
        "ruleset_name": ruleset.get("name") if ruleset else None,
        "enforcement": ruleset.get("enforcement") if ruleset else None,
        "bypass_actors": sorted(
            (a.get("actor_type"), a.get("bypass_mode")) for a in ruleset.get("bypass_actors", [])
        ),
        "ruleset_http": live.get("ruleset_http"),
    }


def compare(repo: str, expected: dict, live: dict) -> list[str]:
    """Return drift lines (empty = match). Raises Void when it cannot evaluate."""
    drift: list[str] = []
    view = live_view(live, int(expected["ruleset_id"]))

    exp_rules = sorted(expected["rules"])
    if view["rule_types"] != exp_rules:
        drift.append(f"{repo}: rule types live={view['rule_types']} expected={exp_rules}")
    if view["foreign_rules"]:
        drift.append(f"{repo}: rules on main from a ruleset the contract does not name: "
                     f"{view['foreign_rules']}")

    exp_ctx = sorted(expected["required_status_checks"])
    exp_has_rule = "required_status_checks" in expected["rules"]
    if view["has_status_rule"] != exp_has_rule:
        drift.append(f"{repo}: required_status_checks rule present={view['has_status_rule']} "
                     f"expected={exp_has_rule}")
    if view["required_status_checks"] != exp_ctx:
        missing = sorted(set(exp_ctx) - set(view["required_status_checks"]))
        extra = sorted(set(view["required_status_checks"]) - set(exp_ctx))
        drift.append(f"{repo}: required contexts differ — live lacks {missing}, live adds {extra}")

    if view["strict_required_status_checks_policy"] != expected.get("strict_required_status_checks_policy"):
        drift.append(f"{repo}: strict_required_status_checks_policy live="
                     f"{view['strict_required_status_checks_policy']} "
                     f"expected={expected.get('strict_required_status_checks_policy')}")

    for key, want in (expected.get("pull_request") or {}).items():
        got = view["pull_request"].get(key)
        if isinstance(want, list):
            want, got = sorted(want), sorted(got or [])
        if got != want:
            drift.append(f"{repo}: pull_request.{key} live={got} expected={want}")

    exp_bypass = sorted((a["actor_type"], a["bypass_mode"]) for a in expected.get("bypass_actors", []))
    if view["bypass_actors"] != exp_bypass:
        drift.append(f"{repo}: bypass_actors live={view['bypass_actors']} expected={exp_bypass}")
    if view["enforcement"] != "active":
        drift.append(f"{repo}: ruleset enforcement live={view['enforcement']} expected=active")
    if view["ruleset_name"] != expected.get("ruleset_name"):
        drift.append(f"{repo}: ruleset name live={view['ruleset_name']!r} "
                     f"expected={expected.get('ruleset_name')!r}")
    return drift


def check_document(doc_text: str, repo: str, expected: dict) -> list[str]:
    drift = []
    for ctx in expected["required_status_checks"]:
        if f"`{ctx}`" not in doc_text:
            drift.append(f"{repo}: document does not name required context `{ctx}` verbatim")
    if str(expected["ruleset_id"]) not in doc_text:
        drift.append(f"{repo}: document does not mention ruleset id {expected['ruleset_id']}")
    return drift


# --------------------------------------------------------------------- self-test

def _synthetic() -> tuple[dict, dict, str]:
    expected = {
        "ruleset_id": 1, "ruleset_name": "main-protection",
        "rules": ["deletion", "pull_request", "required_status_checks"],
        "required_status_checks": ["alpha", "beta (merge tree)"],
        "strict_required_status_checks_policy": False,
        "pull_request": {"required_approving_review_count": 0, "allowed_merge_methods": ["squash", "rebase"]},
        "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}],
    }
    live = {
        "rules": [
            {"type": "deletion", "ruleset_id": 1},
            {"type": "pull_request", "ruleset_id": 1,
             "parameters": {"required_approving_review_count": 0, "allowed_merge_methods": ["rebase", "squash"]}},
            {"type": "required_status_checks", "ruleset_id": 1,
             "parameters": {"strict_required_status_checks_policy": False,
                            "required_status_checks": [{"context": "beta (merge tree)"}, {"context": "alpha"}]}},
        ],
        "ruleset": {"name": "main-protection", "enforcement": "active",
                    "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}]},
        "ruleset_http": 200,
    }
    doc = "ruleset 1 requires `alpha` and `beta (merge tree)`."
    return expected, live, doc


def mutations() -> list[tuple[str, callable]]:
    def add_ctx(l):
        l["rules"][2]["parameters"]["required_status_checks"].append({"context": "gamma"})
    def drop_ctx(l):
        l["rules"][2]["parameters"]["required_status_checks"].pop()
    def flip_strict(l):
        l["rules"][2]["parameters"]["strict_required_status_checks_policy"] = True
    def drop_rule(l):
        l["rules"] = [r for r in l["rules"] if r["type"] != "deletion"]
    def foreign_rule(l):
        l["rules"].append({"type": "non_fast_forward", "ruleset_id": 99})
    def reviews(l):
        l["rules"][1]["parameters"]["required_approving_review_count"] = 2
    def bypass(l):
        l["ruleset"]["bypass_actors"] = []
    def disabled(l):
        l["ruleset"]["enforcement"] = "disabled"
    def status_rule_gone(l):
        l["rules"] = [r for r in l["rules"] if r["type"] != "required_status_checks"]
    return [("add a required context", add_ctx), ("drop a required context", drop_ctx),
            ("flip strict on", flip_strict), ("drop the deletion rule", drop_rule),
            ("a rule from an unnamed ruleset", foreign_rule), ("require 2 reviews", reviews),
            ("remove the bypass actor", bypass), ("disable the ruleset", disabled),
            ("remove the status rule entirely", status_rule_gone)]


def self_test() -> int:
    expected, live, doc = _synthetic()
    failures = []
    if compare("synthetic", expected, live) or check_document(doc, "synthetic", expected):
        failures.append("identical fixtures reported drift")
    for label, mutate in mutations():
        mutated = json.loads(json.dumps(live))
        mutate(mutated)
        if not compare("synthetic", expected, mutated):
            failures.append(f"mutation not detected: {label}")
    if not check_document("ruleset 1 requires `alpha` only.", "synthetic", expected):
        failures.append("document missing a context was not detected")
    if not check_document("requires `alpha` and `beta (merge tree)`.", "synthetic", expected):
        failures.append("document missing the ruleset id was not detected")
    # VOID must be REACHABLE, on every blind path, or the fail-closed branch is
    # the next version of the bug it fixes. Each case must raise Void — not
    # return a match, not return drift.
    void_cases = {
        "ruleset metadata 403 (token lacks permission)": lambda l: l.update(ruleset=None, ruleset_http=403),
        "ruleset metadata 404": lambda l: l.update(ruleset=None, ruleset_http=404),
        "ruleset metadata malformed (not an object)": lambda l: l.update(ruleset=["not", "an", "object"]),
        "ruleset metadata network failure (status 0)": lambda l: l.update(ruleset=None, ruleset_http=0),
        "effective-rules payload without `rules`": lambda l: [l.clear(), l.update(nonsense=True)],
    }
    for label, blind in void_cases.items():
        case = json.loads(json.dumps(live)); blind(case)
        try:
            outcome = compare("synthetic", expected, case)
            failures.append(f"VOID not reached: {label} returned {'drift' if outcome else 'MATCH'} instead of raising")
        except Void:
            pass
    for line in failures:
        print(f"SELF-TEST FAIL: {line}")
    if failures:
        return EXIT_DRIFT
    print(f"self-test: identical => match; {len(mutations())} mutations + 2 document mutations => DRIFT; "
          f"{len(void_cases)} blind paths => VOID (raised, never exit 0). All three answers reachable.")
    return EXIT_OK


# -------------------------------------------------------------------------- main

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    parser.add_argument("--doc", default=str(DEFAULT_DOC))
    parser.add_argument("--repo", action="append",
                        help="repo to check (repeatable); default: the repo this checkout belongs to, "
                             "wrfcoin/core4")
    parser.add_argument("--all", action="store_true", help="check every repo in the manifest")
    parser.add_argument("--fixture", help="JSON file with {repo: {rules, ruleset, ruleset_http}} in place of the API")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    try:
        manifest = json.loads(Path(args.manifest).read_text())
        repos = manifest["repos"]
    except (OSError, ValueError, KeyError) as exc:
        print(f"VOID: manifest unreadable: {exc}")
        return EXIT_VOID
    try:
        doc_text = Path(args.doc).read_text()
    except OSError as exc:
        print(f"VOID: contract document unreadable: {exc}")
        return EXIT_VOID

    targets = list(repos) if args.all else (args.repo or ["wrfcoin/core4"])
    fixture = None
    if args.fixture:
        try:
            fixture = json.loads(Path(args.fixture).read_text())
        except (OSError, ValueError) as exc:
            print(f"VOID: fixture unreadable: {exc}")
            return EXIT_VOID

    all_drift: list[str] = []
    for repo in targets:
        expected = repos.get(repo)
        if expected is None:
            print(f"VOID: {repo} is not in the manifest")
            return EXIT_VOID
        try:
            live = fixture[repo] if fixture is not None else fetch_live(repo, int(expected["ruleset_id"]))
            drift = compare(repo, expected, live)
        except (Void, KeyError, TypeError) as exc:
            print(f"VOID: {repo}: {exc}")
            return EXIT_VOID
        drift += check_document(doc_text, repo, expected)
        if drift:
            all_drift += drift
        else:
            print(f"OK: {repo}: live ruleset {expected['ruleset_id']} == manifest == document "
                  f"({len(expected['required_status_checks'])} required context(s), "
                  f"strict={expected.get('strict_required_status_checks_policy')})")
    if all_drift:
        print("DRIFT between the live ruleset and docs/ci/MERGE_ADMISSION.md / .github/merge-admission.json:")
        for line in all_drift:
            print(f"  - {line}")
        print("Fix by changing the document+manifest (a PR) or the ruleset (owner-only) — together, on "
              "purpose. See MERGE_ADMISSION.md §5.")
        return EXIT_DRIFT
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
