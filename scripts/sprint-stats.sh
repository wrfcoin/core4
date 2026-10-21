#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/sprint-stats.sh --sprint N --since ISO8601 [options]

Collect WRFCoin sprint velocity stats from GitHub:
  - merged PR totals by repo for the sprint window
  - open PR triage counts: ready, review-needed, blocked, stale
  - optional history.csv append for scheduled dashboard runs

Options:
  --sprint N             Sprint number or label (required)
  --since ISO8601        Inclusive merged-PR window start (required)
  --until ISO8601        Inclusive merged-PR window end (default: since + 4h)
  --window-hours N       Default window duration when --until is absent (default: 4)
  --repos LIST           Comma or space separated repos
  --org ORG              GitHub org (default: wrfcoin)
  --history PATH         CSV path (default: docs/sprint-stats/history.csv)
  --update-history       Append this run to history.csv
  --stale-days N         Open PR stale threshold (default: 7)
  --limit N              Per-repo gh result limit (default: 1000)
  -h, --help             Show this help

Environment:
  WRFCOIN_SPRINT_REPOS   Overrides the default repo list when --repos is absent
EOF
}

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SPRINT=""
SINCE=""
UNTIL=""
ORG="wrfcoin"
HISTORY_PATH="docs/sprint-stats/history.csv"
UPDATE_HISTORY=false
STALE_DAYS=7
LIMIT=1000
WINDOW_HOURS=4
REPOS="${WRFCOIN_SPRINT_REPOS:-core4 backend frontend smart-contracts infra security-testing mobile docs docs-private}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sprint)
      SPRINT="${2:-}"
      shift 2
      ;;
    --since)
      SINCE="${2:-}"
      shift 2
      ;;
    --until)
      UNTIL="${2:-}"
      shift 2
      ;;
    --repos)
      REPOS="${2:-}"
      shift 2
      ;;
    --org)
      ORG="${2:-}"
      shift 2
      ;;
    --history)
      HISTORY_PATH="${2:-}"
      shift 2
      ;;
    --update-history)
      UPDATE_HISTORY=true
      shift
      ;;
    --stale-days)
      STALE_DAYS="${2:-}"
      shift 2
      ;;
    --limit)
      LIMIT="${2:-}"
      shift 2
      ;;
    --window-hours)
      WINDOW_HOURS="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SPRINT" || -z "$SINCE" ]]; then
  echo "ERROR: --sprint and --since are required" >&2
  usage >&2
  exit 2
fi

for cmd in gh python3; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $cmd" >&2
    exit 127
  fi
done

if [[ -z "$UNTIL" ]]; then
  UNTIL="$(python3 - "$SINCE" "$WINDOW_HOURS" <<'PY'
import sys
from datetime import datetime, timedelta, timezone

since = sys.argv[1].replace("Z", "+00:00")
hours = float(sys.argv[2])
until = datetime.fromisoformat(since).astimezone(timezone.utc) + timedelta(hours=hours)
print(until.isoformat(timespec="seconds").replace("+00:00", "Z"))
PY
)"
fi

REPOS="${REPOS//,/ }"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cd "$PROJECT_ROOT"

for repo in $REPOS; do
  gh pr list \
    --repo "$ORG/$repo" \
    --state merged \
    --limit "$LIMIT" \
    --json number,title,mergedAt,url \
    > "$TMP_DIR/$repo.merged.json"

  gh pr list \
    --repo "$ORG/$repo" \
    --state open \
    --limit "$LIMIT" \
    --json number,title,updatedAt,isDraft,labels,reviewDecision,url \
    > "$TMP_DIR/$repo.open.json"
done

export SPRINT SINCE UNTIL ORG HISTORY_PATH UPDATE_HISTORY STALE_DAYS TMP_DIR REPOS
python3 <<'PY'
import csv
import json
import os
from datetime import datetime, timezone, timedelta
from pathlib import Path


def parse_time(value):
    if not value:
        return None
    normalized = value.replace("Z", "+00:00")
    return datetime.fromisoformat(normalized).astimezone(timezone.utc)


def label_names(pr):
    return {label.get("name", "").lower() for label in pr.get("labels", [])}


def csv_escape_breakdown(counts):
    return ";".join(f"{repo}:{count}" for repo, count in counts.items() if count)


sprint = os.environ["SPRINT"]
since = os.environ["SINCE"]
until = os.environ["UNTIL"]
org = os.environ["ORG"]
history_path = Path(os.environ["HISTORY_PATH"])
update_history = os.environ["UPDATE_HISTORY"] == "true"
stale_days = int(os.environ["STALE_DAYS"])
tmp_dir = Path(os.environ["TMP_DIR"])
repos = os.environ["REPOS"].split()
now = datetime.now(timezone.utc)
window_end = parse_time(until) or now
window_start = parse_time(since)
stale_before = window_end - timedelta(days=stale_days)

merged_counts = {}
merged_total = 0
open_counts = {
    "open_total": 0,
    "ready": 0,
    "review_needed": 0,
    "blocked": 0,
    "stale": 0,
    "draft": 0,
}
open_by_repo = {}

for repo in repos:
    merged_raw = json.loads((tmp_dir / f"{repo}.merged.json").read_text())
    merged = [
        pr
        for pr in merged_raw
        if window_start <= parse_time(pr.get("mergedAt")) <= window_end
    ]
    open_prs = json.loads((tmp_dir / f"{repo}.open.json").read_text())
    merged_counts[repo] = len(merged)
    merged_total += len(merged)

    repo_counts = {
        "open_total": len(open_prs),
        "ready": 0,
        "review_needed": 0,
        "blocked": 0,
        "stale": 0,
        "draft": 0,
    }

    for pr in open_prs:
        labels = label_names(pr)
        updated_at = parse_time(pr.get("updatedAt"))
        is_blocked = any(
            name in labels
            for name in {
                "blocked",
                "ci-blocked",
                "do-not-merge",
                "do not merge",
                "needs-decision",
            }
        )
        is_draft = bool(pr.get("isDraft"))
        is_ready = (
            not is_blocked
            and not is_draft
            and (
                pr.get("reviewDecision") == "APPROVED"
                or "ready" in labels
                or "ready-to-merge" in labels
            )
        )
        is_stale = bool(updated_at and updated_at < stale_before)

        repo_counts["blocked"] += int(is_blocked)
        repo_counts["draft"] += int(is_draft)
        repo_counts["ready"] += int(is_ready)
        repo_counts["stale"] += int(is_stale)
        repo_counts["review_needed"] += int(
            not is_blocked and not is_draft and not is_ready
        )

    for key, value in repo_counts.items():
        open_counts[key] += value
    open_by_repo[repo] = repo_counts

previous_total = None
previous_sprint = None
if history_path.exists():
    with history_path.open(newline="") as f:
        for row in csv.DictReader(f):
            try:
                row_sprint = int(row.get("sprint", ""))
                this_sprint = int(sprint)
            except ValueError:
                continue
            if row_sprint < this_sprint:
                if previous_sprint is None or row_sprint > previous_sprint:
                    previous_sprint = row_sprint
                    previous_total = int(row.get("merged_total", 0))

delta_text = "n/a"
if previous_total is not None:
    delta = merged_total - previous_total
    delta_text = f"{delta:+d} vs sprint {previous_sprint}"

print(f"# Sprint {sprint} stats")
print()
print(f"- Window: `{since}` to `{until}`")
print(f"- Repos: `{', '.join(f'{org}/{repo}' for repo in repos)}`")
print(f"- Merged PRs: **{merged_total}** ({delta_text})")
print(
    "- Open PR triage: "
    f"**{open_counts['open_total']}** open, "
    f"{open_counts['ready']} ready, "
    f"{open_counts['review_needed']} review-needed, "
    f"{open_counts['blocked']} blocked, "
    f"{open_counts['stale']} stale, "
    f"{open_counts['draft']} draft"
)
print()
print("## Merged PRs by repo")
print()
print("| Repo | Merged |")
print("| --- | ---: |")
for repo, count in sorted(merged_counts.items(), key=lambda item: (-item[1], item[0])):
    print(f"| `{repo}` | {count} |")
print()
print("## Open PR triage by repo")
print()
print("| Repo | Open | Ready | Review-needed | Blocked | Stale | Draft |")
print("| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
for repo, counts in sorted(open_by_repo.items(), key=lambda item: (-item[1]["open_total"], item[0])):
    print(
        f"| `{repo}` | {counts['open_total']} | {counts['ready']} | "
        f"{counts['review_needed']} | {counts['blocked']} | "
        f"{counts['stale']} | {counts['draft']} |"
    )

if update_history:
    history_path.parent.mkdir(parents=True, exist_ok=True)
    new_file = not history_path.exists() or history_path.stat().st_size == 0
    with history_path.open("a", newline="") as f:
        fieldnames = [
            "sprint",
            "since",
            "until",
            "generated_at",
            "merged_total",
            "open_total",
            "ready_count",
            "review_needed_count",
            "blocked_count",
            "stale_count",
            "draft_count",
            "repo_breakdown",
        ]
        writer = csv.DictWriter(f, fieldnames=fieldnames, lineterminator="\n")
        if new_file:
            writer.writeheader()
        writer.writerow(
            {
                "sprint": sprint,
                "since": since,
                "until": until,
                "generated_at": now.isoformat(timespec="seconds").replace("+00:00", "Z"),
                "merged_total": merged_total,
                "open_total": open_counts["open_total"],
                "ready_count": open_counts["ready"],
                "review_needed_count": open_counts["review_needed"],
                "blocked_count": open_counts["blocked"],
                "stale_count": open_counts["stale"],
                "draft_count": open_counts["draft"],
                "repo_breakdown": csv_escape_breakdown(merged_counts),
            }
        )
    print()
    print(f"_Updated `{history_path}`._")
PY
