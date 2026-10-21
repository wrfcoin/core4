#!/usr/bin/env python3
"""Emit an in-toto/SLSA-shaped provenance statement per release artifact (#2151).

WHY THIS EXISTS
----------------
An audit report names an exact commit SHA. Provenance is what lets an operator
confirm the BINARY they are about to run actually came from that commit,
built by the workflow the project publishes, rather than trusting the
filename or the release page's prose. Without it, the audit's scope statement
is unenforceable — nothing ties the artifact in your hand back to what was
reviewed.

WHAT THIS IS, AND WHAT IT IS NOT
---------------------------------
This emits an in-toto v1 Statement whose predicate follows the shape of
SLSA Provenance v1 (https://slsa.dev/spec/v1.0/provenance) closely enough to
be useful to a human or a script cross-checking a build, but it does **not**
run through `slsa-framework/slsa-github-generator`'s keyless-signing /
Rekor-transparency-log pipeline. #312 found that generator wired in as a
*step* inside a job (a reusable workflow must be called at the *job* level,
`jobs.<id>.uses:`), and this repo has not re-attempted that specific
integration since. Calling this statement "SLSA provenance" without that
caveat would overstate what it proves: it proves the artifact's digest and
the recorded source/workflow/builder facts are internally consistent and
were emitted by a run with the values it claims — it does NOT carry a
Sigstore-anchored, third-party-verifiable signature the way the full SLSA
Level 3 generator does. `scripts/verify_provenance.sh` and the CI job that
calls it verify exactly this weaker, self-attested claim; the GPG signature
already applied to `SHA256SUMS` (the same key that signs the release
binaries) is what gives the *whole bundle*, provenance files included, a
verifiable signature. Upgrading to the full generator is future work, not
claimed here.

USAGE
-----
    python3 scripts/generate_provenance.py \
        --artifact path/to/wrfcoin-node-linux-x86_64 \
        --out path/to/wrfcoin-node-linux-x86_64.provenance.json \
        --source-repo wrfcoin/core4 \
        --source-commit <sha> \
        --workflow-ref .github/workflows/release-signing.yml@refs/tags/v1.2.3 \
        --run-id 123456 --run-attempt 1 \
        --builder-id "https://github.com/wrfcoin/core4/actions/runs/123456" \
        --build-info path/to/wrfcoin-node-linux-x86_64.buildinfo.json

The source/workflow/run/builder values are read from real GitHub Actions
context (`github.*`) by the caller (the `release-provenance.yml` reusable
workflow). `buildCommand` and `toolchain` come from `--build-info`, the sidecar
the BUILD job wrote with `scripts/write_build_info.py` from the exact command
it executed and the `rustc -vV` it ran — measured where the artifact was built,
not asserted by the provenance job. (`--toolchain` / `--build-command` remain
for direct use; exactly one source is accepted.) This script does not invent or
default any value; a missing one is a hard error, not a silently-empty field.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

IN_TOTO_STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
SLSA_PREDICATE_TYPE = "https://slsa.dev/provenance/v1"


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_statement(
    artifact_path: Path,
    source_repo: str,
    source_commit: str,
    workflow_ref: str,
    run_id: str,
    run_attempt: str,
    builder_id: str,
    toolchain: str,
    build_command: str,
    generated_at: str,
) -> dict:
    return {
        "_type": IN_TOTO_STATEMENT_TYPE,
        "subject": [
            {
                "name": artifact_path.name,
                "digest": {"sha256": sha256_of(artifact_path)},
            }
        ],
        "predicateType": SLSA_PREDICATE_TYPE,
        "predicate": {
            "buildDefinition": {
                "buildType": "https://github.com/wrfcoin/core4/build-types/cargo-release@v1",
                "externalParameters": {
                    "source": {
                        "repository": f"https://github.com/{source_repo}",
                        "ref": source_commit,
                    },
                    "workflow": workflow_ref,
                    "buildCommand": build_command,
                },
                "internalParameters": {
                    "toolchain": toolchain,
                },
                "resolvedDependencies": [
                    {
                        "uri": f"git+https://github.com/{source_repo}@{source_commit}",
                        "digest": {"gitCommit": source_commit},
                    }
                ],
            },
            "runDetails": {
                "builder": {"id": builder_id},
                "metadata": {
                    "invocationId": f"{run_id}-{run_attempt}",
                    "startedOn": generated_at,
                },
            },
            # Explicit, not implied by omission: this statement is
            # self-attested by the CI run, not by the full SLSA Level 3
            # generator's keyless-signing pipeline. See the module docstring.
            "wrfcoinNote": (
                "Self-attested provenance emitted by generate_provenance.py, not the "
                "slsa-framework/slsa-github-generator pipeline (#312, #2151). Verify via "
                "scripts/verify_provenance.sh; integrity of this statement itself relies on "
                "the GPG signature over SHA256SUMS, which is computed over this file too."
            ),
        },
    }


def load_build_info(path: Path) -> tuple[str, str]:
    """Return (toolchain, build_command) from a build job's sidecar."""
    try:
        info = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise ValueError(f"unreadable build-info sidecar {path}: {exc}") from exc
    toolchain = info.get("toolchain") if isinstance(info, dict) else None
    command = info.get("buildCommand") if isinstance(info, dict) else None
    if not isinstance(toolchain, str) or not toolchain.strip():
        raise ValueError(f"build-info sidecar {path} has no toolchain")
    if not isinstance(command, str) or not command.strip():
        raise ValueError(f"build-info sidecar {path} has no buildCommand")
    return toolchain, command


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--source-repo", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", required=True)
    parser.add_argument("--builder-id", required=True)
    parser.add_argument("--build-info", type=Path)
    parser.add_argument("--toolchain")
    parser.add_argument("--build-command")
    args = parser.parse_args(argv)

    direct = args.toolchain is not None or args.build_command is not None
    if args.build_info is not None and direct:
        parser.error("use --build-info OR --toolchain/--build-command, not both")
    if args.build_info is not None:
        try:
            toolchain, build_command = load_build_info(args.build_info)
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
    elif args.toolchain and args.build_command:
        toolchain, build_command = args.toolchain, args.build_command
    else:
        parser.error("--build-info (or both --toolchain and --build-command) is required")

    if not args.artifact.is_file():
        print(f"error: artifact not found: {args.artifact}", file=sys.stderr)
        return 1

    statement = build_statement(
        artifact_path=args.artifact,
        source_repo=args.source_repo,
        source_commit=args.source_commit,
        workflow_ref=args.workflow_ref,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        builder_id=args.builder_id,
        toolchain=toolchain,
        build_command=build_command,
        generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(statement, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out} for subject {args.artifact.name} (sha256 {statement['subject'][0]['digest']['sha256'][:12]}…)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
