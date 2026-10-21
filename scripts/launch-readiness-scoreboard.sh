#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/launch-readiness-scoreboard.sh [options]

Render the live testnet launch-readiness scoreboard from GitHub issue state.

Options:
  --org ORG                 GitHub org (default: wrfcoin)
  --repo REPO               Launch-readiness repo (default: core4)
  --milestone N             Launch-readiness milestone number (default: 2)
  --gate-issues LIST        Comma/space separated human gate issue numbers
                            (default: 720,721,724)
  --format FORMAT           markdown, json, or both (default: markdown)
  --fixture PATH            Read fixture JSON instead of calling gh
  -h, --help                Show this help

The script is read-only. It does not edit issues, comments, docs, services, or
live nodes.
EOF
}

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ORG="wrfcoin"
REPO="core4"
MILESTONE="2"
GATE_ISSUES="720 721 724"
FORMAT="markdown"
FIXTURE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org)
      ORG="${2:-}"
      shift 2
      ;;
    --repo)
      REPO="${2:-}"
      shift 2
      ;;
    --milestone)
      MILESTONE="${2:-}"
      shift 2
      ;;
    --gate-issues)
      GATE_ISSUES="${2:-}"
      shift 2
      ;;
    --format)
      FORMAT="${2:-}"
      shift 2
      ;;
    --fixture)
      FIXTURE="${2:-}"
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

case "$FORMAT" in
  markdown|json|both) ;;
  *)
    echo "ERROR: --format must be markdown, json, or both" >&2
    exit 2
    ;;
esac

if ! command -v python3 >/dev/null 2>&1; then
  echo "ERROR: required command not found: python3" >&2
  exit 127
fi

if [[ -z "$FIXTURE" ]] && ! command -v gh >/dev/null 2>&1; then
  echo "ERROR: required command not found: gh" >&2
  exit 127
fi

cd "$PROJECT_ROOT"

export ORG REPO MILESTONE GATE_ISSUES FORMAT FIXTURE

python3 <<'PY'
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def normalize_list(value):
    return [item for item in re.split(r"[\s,]+", value.strip()) if item]


def gh_json(args):
    proc = subprocess.run(["gh", *args], text=True, capture_output=True)
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout or "gh command failed").strip()
        raise RuntimeError(detail)
    return json.loads(proc.stdout)


def load_live(org, repo, milestone, gate_issue_numbers):
    milestone_data = gh_json(["api", f"repos/{org}/{repo}/milestones/{milestone}"])
    issues = gh_json(
        [
            "api",
            f"repos/{org}/{repo}/issues?milestone={milestone}&state=all&per_page=100",
        ]
    )
    issue_map = {
        int(item.get("number", 0)): item
        for item in issues
        if "pull_request" not in item and item.get("number")
    }
    for number in gate_issue_numbers:
        if number not in issue_map:
            issue_map[number] = gh_json(["api", f"repos/{org}/{repo}/issues/{number}"])
    return {
        "milestone": milestone_data,
        "issues": list(issue_map.values()),
    }


def load_fixture(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def issue_number(issue):
    return int(issue.get("number", 0))


def issue_state(issue):
    return str(issue.get("state", "")).upper()


def label_names(issue):
    return sorted(label.get("name", "") for label in issue.get("labels", []) if label.get("name"))


def issue_row(issue, repo):
    labels = label_names(issue)
    return {
        "ref": f"{repo}#{issue_number(issue)}",
        "number": issue_number(issue),
        "title": issue.get("title", ""),
        "state": issue_state(issue),
        "url": issue.get("html_url") or issue.get("url") or "",
        "labels": labels,
    }


def has_any_label(issue, wanted):
    labels = set(label_names(issue))
    return any(label in labels for label in wanted)


def md_escape(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


org = os.environ["ORG"]
repo = os.environ["REPO"]
milestone_no = os.environ["MILESTONE"]
gate_issue_numbers = {int(item) for item in normalize_list(os.environ["GATE_ISSUES"])}
output_format = os.environ["FORMAT"]
fixture = os.environ["FIXTURE"]

try:
    source = load_fixture(fixture) if fixture else load_live(org, repo, milestone_no, gate_issue_numbers)
except Exception as exc:
    print(f"ERROR: unable to load launch-readiness data: {exc}", file=sys.stderr)
    sys.exit(1)

timestamp = datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")
milestone = source.get("milestone", {})
issues = sorted(source.get("issues", []), key=issue_number)
rows = [issue_row(issue, repo) for issue in issues]
open_rows = [row for row in rows if row["state"] != "CLOSED"]
closed_rows = [row for row in rows if row["state"] == "CLOSED"]

gate_rows = [row for row in rows if row["number"] in gate_issue_numbers]
missing_gate_numbers = sorted(gate_issue_numbers - {row["number"] for row in gate_rows})
open_gate_rows = [row for row in gate_rows if row["state"] != "CLOSED"]

launch_blocker_labels = {"launch-blocker", "testnet-hardening", "security", "P0", "P1"}
open_blocker_rows = [
    issue_row(issue, repo)
    for issue in issues
    if issue_state(issue) != "CLOSED" and has_any_label(issue, launch_blocker_labels)
]

total = len(rows)
closed = len(closed_rows)
score_percent = round((closed / total) * 100, 1) if total else 100.0
no_go_reasons = []

if missing_gate_numbers:
    no_go_reasons.append(
        "missing_human_gate_issues:" + ",".join(f"{repo}#{number}" for number in missing_gate_numbers)
    )
if open_gate_rows:
    no_go_reasons.append(
        "open_human_gate_issues:" + ",".join(row["ref"] for row in open_gate_rows)
    )
if open_rows:
    no_go_reasons.append(f"open_milestone_issues:{len(open_rows)}")

status = "GO" if not no_go_reasons else "NO-GO"

scoreboard = {
    "generated_at": timestamp,
    "repo": f"{org}/{repo}",
    "milestone": {
        "number": int(milestone_no),
        "title": milestone.get("title", f"milestone-{milestone_no}"),
        "url": milestone.get("html_url", ""),
        "open_issues": int(milestone.get("open_issues", len(open_rows) if rows else 0)),
        "closed_issues": int(milestone.get("closed_issues", len(closed_rows) if rows else 0)),
    },
    "status": status,
    "score": {
        "closed": closed,
        "total": total,
        "percent": score_percent,
    },
    "counts": {
        "open": len(open_rows),
        "closed": len(closed_rows),
        "human_gates_open": len(open_gate_rows),
        "human_gates_missing": len(missing_gate_numbers),
        "open_labeled_blockers": len(open_blocker_rows),
    },
    "reasons": no_go_reasons or ["none"],
    "human_gates": gate_rows,
    "missing_human_gates": [f"{repo}#{number}" for number in missing_gate_numbers],
    "open_labeled_blockers": open_blocker_rows,
    "open_items": open_rows,
}


def render_markdown(data):
    milestone_data = data["milestone"]
    lines = [
        "# Launch Readiness Scoreboard",
        "",
        f"Generated: `{data['generated_at']}`",
        f"Repository: `{data['repo']}`",
        f"Milestone: [{md_escape(milestone_data['title'])}]({milestone_data['url']})"
        if milestone_data["url"]
        else f"Milestone: {md_escape(milestone_data['title'])}",
        "",
        f"## Overall: {data['status']}",
        "",
        (
            f"Score: **{data['score']['closed']} / {data['score']['total']} closed** "
            f"({data['score']['percent']}%)."
        ),
        (
            f"Open milestone issues: **{data['counts']['open']}**. "
            f"Open human gates: **{data['counts']['human_gates_open']}**. "
            f"Open labeled blockers: **{data['counts']['open_labeled_blockers']}**."
        ),
        "",
        "Reasons: `" + ", ".join(data["reasons"]) + "`",
        "",
        "## Human Gates",
        "",
        "| Gate | State | Title |",
        "|---|---:|---|",
    ]
    if data["human_gates"]:
        for row in data["human_gates"]:
            ref = f"[{row['ref']}]({row['url']})" if row["url"] else row["ref"]
            lines.append(f"| {ref} | {row['state']} | {md_escape(row['title'])} |")
    else:
        lines.append("| none | - | - |")

    if data["missing_human_gates"]:
        lines.extend(["", "Missing human gates: `" + ", ".join(data["missing_human_gates"]) + "`"])

    lines.extend(
        [
            "",
            "## Open Labeled Blockers",
            "",
            "| Issue | State | Labels | Title |",
            "|---|---:|---|---|",
        ]
    )
    if data["open_labeled_blockers"]:
        for row in data["open_labeled_blockers"][:50]:
            ref = f"[{row['ref']}]({row['url']})" if row["url"] else row["ref"]
            lines.append(
                f"| {ref} | {row['state']} | {md_escape(', '.join(row['labels']))} | "
                f"{md_escape(row['title'])} |"
            )
    else:
        lines.append("| none | - | - | - |")

    lines.extend(
        [
            "",
            "## Open Milestone Items",
            "",
            "| Issue | State | Labels | Title |",
            "|---|---:|---|---|",
        ]
    )
    if data["open_items"]:
        for row in data["open_items"][:75]:
            ref = f"[{row['ref']}]({row['url']})" if row["url"] else row["ref"]
            lines.append(
                f"| {ref} | {row['state']} | {md_escape(', '.join(row['labels']))} | "
                f"{md_escape(row['title'])} |"
            )
        if len(data["open_items"]) > 75:
            lines.append(f"| ... | ... | ... | {len(data['open_items']) - 75} more open items omitted |")
    else:
        lines.append("| none | - | - | - |")

    return "\n".join(lines) + "\n"


if output_format in {"json", "both"}:
    print(json.dumps(scoreboard, indent=2, sort_keys=True))
    if output_format == "both":
        print()

if output_format in {"markdown", "both"}:
    print(render_markdown(scoreboard), end="")
PY
