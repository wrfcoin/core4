#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/launch-gate-check.sh [options]

Run a read-only launch-readiness GO/NO-GO checkpoint for active sprint windows.

Checks:
  - core4 launch milestone open_issues count is readable and optionally matches
    --expected-open-issues
  - human-gated launch issues exist and their current state is reported
  - 3-5 open milestone issues are spot-checked through gh issue view
  - repo-prefixed issue references in sprint docs and milestone issue text
    resolve in the repository named by the prefix

Options:
  --org ORG                    GitHub org (default: wrfcoin)
  --repo REPO                  Launch-gate repo (default: core4)
  --milestone N                GitHub milestone number (default: 2)
  --expected-open-issues N     Fail if milestone open_issues differs
  --gate-issues LIST           Comma/space separated issue numbers
                               (default: 720,721,724)
  --scan-path PATH             Additional local path to scan for repo#issue
                               references. May be repeated. Defaults always
                               include docs/plans and docs/operations.
  --known-repos LIST           Repos whose repo#issue refs should be verified
  -h, --help                   Show this help

Output:
  Prints detail lines plus one final sprint-doc-ready line:
    2026-05-30T12:00:00Z GO launch_gate ...
    2026-05-30T12:00:00Z NO-GO launch_gate reasons="..."

Environment:
  WRFCOIN_LAUNCH_GATE_TOKEN can be used by gh auth, but the script itself does
  not read or print token values.
EOF
}

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ORG="wrfcoin"
CORE_REPO="core4"
MILESTONE="2"
EXPECTED_OPEN_ISSUES=""
GATE_ISSUES="720 721 724"
SCAN_PATHS=("docs/plans" "docs/operations")
KNOWN_REPOS="core4 backend frontend smart-contracts infra security-testing mobile docs docs-private"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org)
      ORG="${2:-}"
      shift 2
      ;;
    --repo)
      CORE_REPO="${2:-}"
      shift 2
      ;;
    --milestone)
      MILESTONE="${2:-}"
      shift 2
      ;;
    --expected-open-issues)
      EXPECTED_OPEN_ISSUES="${2:-}"
      shift 2
      ;;
    --gate-issues)
      GATE_ISSUES="${2:-}"
      shift 2
      ;;
    --scan-path)
      SCAN_PATHS+=("${2:-}")
      shift 2
      ;;
    --known-repos)
      KNOWN_REPOS="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

for cmd in gh python3 git; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $cmd" >&2
    exit 127
  fi
done

cd "$PROJECT_ROOT"

export ORG CORE_REPO MILESTONE EXPECTED_OPEN_ISSUES GATE_ISSUES KNOWN_REPOS
export SCAN_PATHS_JOINED
SCAN_PATHS_JOINED="$(printf '%s\n' "${SCAN_PATHS[@]}")"

python3 <<'PY'
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def run(cmd, *, json_output=True):
    proc = subprocess.run(cmd, text=True, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError((proc.stderr or proc.stdout or "command failed").strip())
    if not json_output:
        return proc.stdout
    return json.loads(proc.stdout)


def normalize_list(value):
    return [item for item in re.split(r"[\s,]+", value.strip()) if item]


def repo_ref_source(path, line_no):
    return f"{path}:{line_no}"


org = os.environ["ORG"]
core_repo = os.environ["CORE_REPO"]
milestone_no = os.environ["MILESTONE"]
expected_open = os.environ["EXPECTED_OPEN_ISSUES"]
gate_issues = normalize_list(os.environ["GATE_ISSUES"])
known_repos = set(normalize_list(os.environ["KNOWN_REPOS"]))
scan_paths = [line for line in os.environ["SCAN_PATHS_JOINED"].splitlines() if line]

errors = []
timestamp = datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")

try:
    milestone = run(["gh", "api", f"repos/{org}/{core_repo}/milestones/{milestone_no}"])
except Exception as exc:
    print(f"milestone ERROR {org}/{core_repo}#{milestone_no}: {exc}")
    print(f'{timestamp} NO-GO launch_gate reasons="milestone_unreadable"')
    sys.exit(1)

open_issues = int(milestone.get("open_issues", 0))
milestone_title = milestone.get("title", f"milestone-{milestone_no}")
print(
    f"milestone OK repo={org}/{core_repo} number={milestone_no} "
    f"title={milestone_title!r} open_issues={open_issues}"
)

if expected_open:
    try:
        expected = int(expected_open)
    except ValueError:
        errors.append(f"invalid_expected_open_issues:{expected_open}")
    else:
        if open_issues != expected:
            errors.append(f"milestone_open_issues_expected_{expected}_actual_{open_issues}")

gate_status = []
for issue_no in gate_issues:
    try:
        issue = run(
            [
                "gh",
                "issue",
                "view",
                issue_no,
                "--repo",
                f"{org}/{core_repo}",
                "--json",
                "number,state,title,url",
            ]
        )
        gate_status.append(f"{core_repo}#{issue['number']}:{issue['state']}")
        print(
            f"gate_issue OK {core_repo}#{issue['number']} "
            f"state={issue['state']} title={issue['title']!r} url={issue['url']}"
        )
    except Exception as exc:
        gate_status.append(f"{core_repo}#{issue_no}:MISSING")
        errors.append(f"missing_gate_issue_{core_repo}#{issue_no}")
        print(f"gate_issue ERROR {core_repo}#{issue_no}: {exc}")

try:
    milestone_issues = run(
        [
            "gh",
            "api",
            f"repos/{org}/{core_repo}/issues?milestone={milestone_no}&state=open&per_page=100",
        ]
    )
except Exception as exc:
    milestone_issues = []
    errors.append("milestone_issue_list_unreadable")
    print(f"milestone_issues ERROR: {exc}")

spot_checked = []
for issue in [item for item in milestone_issues if "pull_request" not in item][:5]:
    number = str(issue.get("number"))
    try:
        run(
            [
                "gh",
                "issue",
                "view",
                number,
                "--repo",
                f"{org}/{core_repo}",
                "--json",
                "number,state,title,url",
            ]
        )
        spot_checked.append(number)
    except Exception as exc:
        errors.append(f"milestone_spot_check_failed_{core_repo}#{number}")
        print(f"spot_check ERROR {core_repo}#{number}: {exc}")

if spot_checked:
    print(f"spot_check OK issues={','.join(f'{core_repo}#{n}' for n in spot_checked)}")
else:
    print("spot_check WARN no open milestone issues available to spot-check")

ref_pattern = re.compile(r"\b([A-Za-z0-9_.-]+)#([0-9]+)\b")
refs = {}

for root_text in scan_paths:
    root = Path(root_text)
    if not root.exists():
        print(f"local_scan WARN missing_path={root}")
        continue
    files = [root] if root.is_file() else [p for p in root.rglob("*") if p.is_file()]
    for path in files:
        if ".git" in path.parts:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        for line_no, line in enumerate(text.splitlines(), start=1):
            for repo, number in ref_pattern.findall(line):
                if repo in known_repos:
                    refs.setdefault((repo, number), set()).add(repo_ref_source(path, line_no))

for issue in milestone_issues:
    source = f"milestone:{core_repo}#{issue.get('number')}"
    text = "\n".join(
        part
        for part in [issue.get("title") or "", issue.get("body") or ""]
        if part
    )
    for repo, number in ref_pattern.findall(text):
        if repo in known_repos:
            refs.setdefault((repo, number), set()).add(source)

verified_refs = []
for (repo, number), sources in sorted(refs.items()):
    local_sources = [source for source in sources if not source.startswith("milestone:")]
    if repo != core_repo and number in gate_issues and local_sources:
        sample_sources = ",".join(sorted(local_sources)[:3])
        errors.append(f"wrong_repo_gate_ref_{repo}#{number}")
        print(
            f"cross_repo_ref ERROR {repo}#{number} sources={sample_sources}: "
            f"human launch gate #{number} must be cited as {core_repo}#{number}"
        )
        continue
    try:
        run(
            [
                "gh",
                "issue",
                "view",
                number,
                "--repo",
                f"{org}/{repo}",
                "--json",
                "number,state,title,url",
            ]
        )
        verified_refs.append(f"{repo}#{number}")
    except Exception as exc:
        sample_sources = ",".join(sorted(sources)[:3])
        errors.append(f"bad_cross_repo_ref_{repo}#{number}")
        print(f"cross_repo_ref ERROR {repo}#{number} sources={sample_sources}: {exc}")

if verified_refs:
    print(f"cross_repo_ref OK checked={len(verified_refs)} refs={','.join(verified_refs[:20])}")
else:
    print("cross_repo_ref OK checked=0 refs")

status = "GO" if not errors else "NO-GO"
reason_text = ",".join(errors) if errors else "none"
gate_text = ",".join(gate_status) if gate_status else "none"
print(
    f'{timestamp} {status} launch_gate repo={org}/{core_repo} '
    f"milestone={milestone_no} open_issues={open_issues} "
    f"gate_issues={gate_text} spot_checked={','.join(spot_checked) or 'none'} "
    f"cross_repo_refs_checked={len(verified_refs)} reasons=\"{reason_text}\""
)

sys.exit(0 if not errors else 1)
PY
