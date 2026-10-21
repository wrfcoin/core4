#!/usr/bin/env bash
# Verify a release artifact against its provenance statement (#2151).
#
# Signing that nobody verifies is a no-op: this is the operator-facing (and
# CI-facing) other half of generate_provenance.py. It recomputes the
# artifact's own sha256 and checks it against the digest the provenance
# statement claims for it, then prints the source commit and workflow run so
# a human can cross-check them against the audit report or the release notes
# BY EYE — this script does not (and cannot) prove the provenance statement's
# claims are true, only that the artifact you have matches the digest the
# statement records for it, and that the statement's own subject name matches
# the artifact filename you pointed it at.
#
# Usage:
#   scripts/verify_provenance.sh <artifact-file> <provenance.json>
#
# Exit 0 only when every check passes. Exit 1 on any mismatch or missing
# input, with the specific reason on stderr — never a silent "looks fine".

set -euo pipefail

usage() {
  echo "Usage: $0 <artifact-file> <provenance.json>" >&2
  exit 2
}

[[ $# -eq 2 ]] || usage
ARTIFACT="$1"
PROVENANCE="$2"

fail() {
  echo "::error::VERIFY FAILED — $1" >&2
  exit 1
}

[[ -f "$ARTIFACT" ]] || fail "artifact not found: $ARTIFACT"
[[ -f "$PROVENANCE" ]] || fail "provenance file not found: $PROVENANCE"

command -v python3 >/dev/null 2>&1 || fail "python3 is required (for JSON parsing) but was not found"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required but was not found"

ARTIFACT_NAME="$(basename "$ARTIFACT")"
ACTUAL_SHA256="$(sha256sum "$ARTIFACT" | awk '{print $1}')"

read -r STMT_TYPE PRED_TYPE SUBJECT_NAME SUBJECT_SHA256 SOURCE_REPO SOURCE_COMMIT BUILDER_ID <<PYEOF
$(python3 - "$PROVENANCE" <<'PYSCRIPT'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    stmt = json.load(handle)

subjects = stmt.get("subject") or []
if len(subjects) != 1:
    print("PARSE_ERROR PARSE_ERROR PARSE_ERROR PARSE_ERROR PARSE_ERROR PARSE_ERROR PARSE_ERROR")
    sys.exit(0)

subject = subjects[0]
predicate = stmt.get("predicate") or {}
build_def = predicate.get("buildDefinition") or {}
ext_params = build_def.get("externalParameters") or {}
source = ext_params.get("source") or {}
run_details = predicate.get("runDetails") or {}
builder = run_details.get("builder") or {}


def field(value: str) -> str:
    # Fields are shown to a human on one line; refuse to let a value with
    # whitespace silently misalign the columns rather than escaping it.
    text = str(value) if value is not None else "MISSING"
    return text.replace(" ", "_") or "MISSING"


print(
    field(stmt.get("_type")),
    field(stmt.get("predicateType")),
    field(subject.get("name")),
    field((subject.get("digest") or {}).get("sha256")),
    field(source.get("repository")),
    field(source.get("ref")),
    field(builder.get("id")),
)
PYSCRIPT
)
PYEOF

[[ "$STMT_TYPE" != "PARSE_ERROR" ]] || fail "$PROVENANCE does not have exactly one subject — refusing to guess which one applies to $ARTIFACT_NAME"
[[ "$STMT_TYPE" == "https://in-toto.io/Statement/v1" ]] || fail "unexpected _type: $STMT_TYPE"
[[ "$PRED_TYPE" == "https://slsa.dev/provenance/v1" ]] || fail "unexpected predicateType: $PRED_TYPE"
[[ "$SUBJECT_NAME" == "$ARTIFACT_NAME" ]] || fail "provenance subject name '$SUBJECT_NAME' does not match artifact filename '$ARTIFACT_NAME'"
[[ "$SUBJECT_SHA256" == "$ACTUAL_SHA256" ]] || fail "digest mismatch — provenance says sha256=$SUBJECT_SHA256, artifact actually hashes to $ACTUAL_SHA256"

echo "OK: $ARTIFACT_NAME matches its provenance (sha256 $ACTUAL_SHA256)"
echo "  source:  ${SOURCE_REPO//_/ } @ $SOURCE_COMMIT"
echo "  builder: ${BUILDER_ID//_/ }"
echo "Cross-check the source commit above against the audit report / release notes by eye — this script proves the artifact matches ITS OWN provenance statement, not that the statement's claims are independently attested."
