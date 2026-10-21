#!/usr/bin/env python3
"""Fail-closed workspace integration-test inventory and failure ratchet (#2490).

Cargo's broad-looking ``--workspace --lib`` command does not execute integration
targets.  This gate discovers every target whose Cargo metadata kind contains
``test``, compares that set with a reviewed manifest, executes the targets not
already owned by an enforced CI step, and accepts only the exact known failures
with the exact reviewed diagnostic class.

The known-broken list is deliberately shrink-only:

* an unlisted failure is RED;
* a listed failure with changed text is RED;
* a listed failure that starts passing is RED until its entry is removed;
* a listed failure whose test no longer executes (#[ignore]d, filtered, or
  renamed inside a still-executing target) is RED until the test runs again —
  removing the entry is NOT the fix for that state, and the process exits 3
  (EXIT_GUARD_NOT_EXECUTED) instead of 1 so callers can tell it apart from
  every other red, including "now passes" (#2902); and
* a new, removed, renamed, or unexecuted target is RED.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = Path(__file__).with_name("integration_test_ratchet.json")
DEFAULT_WORKFLOW = ROOT / ".github" / "workflows" / "ci-readiness.yml"
# Repository-relative name of DEFAULT_WORKFLOW; an `external_coverage` entry
# that names no `workflow` is assumed to mean this one, which is how every
# entry was written before #2888 made the field explicit.
DEFAULT_WORKFLOW_REL = ".github/workflows/ci-readiness.yml"
# #2888: an external-coverage claim subtracts a target from this ratchet, so the
# workflow it points at must actually run on the events that gate main. A
# `workflow_dispatch`-only workflow "covers" a target the way a spare key in a
# drawer nobody opens covers a door.
FIRING_EVENTS = frozenset({"push", "pull_request"})
Target = tuple[str, str]
FailureKey = tuple[str, str, str]


def workflow_triggers(workflow_text: str) -> list[str]:
    """Event names a workflow fires on, read from the PARSED `on:` block.

    Parsed, not grepped: this repository has a worked example of a workflow
    whose banner comment claims a `pull_request` trigger it does not have
    (`ci-readiness.yml`, #2888), so the comment layer is exactly the layer
    that must not be trusted here. PyYAML reads the bare `on:` key as the
    boolean True (YAML 1.1), hence the two-key lookup.
    """
    try:
        import yaml  # PyYAML 6.0.x is provisioned on CT102 (ci-deploy-features.yml verifies it)
    except ImportError as exc:  # pragma: no cover - environment, not logic
        raise SystemExit(
            "integration_test_ratchet: PyYAML is required to verify that an "
            "external_coverage workflow fires; refusing to validate without it"
        ) from exc
    document = yaml.safe_load(workflow_text) or {}
    if not isinstance(document, dict):
        return []
    on = document.get(True, document.get("on"))
    if isinstance(on, dict):
        return sorted(str(key) for key in on)
    if isinstance(on, list):
        return sorted(str(item) for item in on)
    if isinstance(on, str):
        return [on]
    return []


def _step_job_continues_on_error(workflow_text: str, step: str) -> bool | None:
    """True if the job owning step `step` (or the step itself) sets
    continue-on-error, False if neither does, None if the step is not found
    in the parsed document."""
    try:
        import yaml
    except ImportError:  # pragma: no cover - handled by workflow_triggers first
        return None
    document = yaml.safe_load(workflow_text) or {}
    jobs = document.get("jobs") if isinstance(document, dict) else None
    if not isinstance(jobs, dict):
        return None
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        for candidate in job.get("steps") or []:
            if isinstance(candidate, dict) and candidate.get("id") == step:
                return bool(job.get("continue-on-error")) or bool(
                    candidate.get("continue-on-error")
                )
    return None

# Process exit codes. 0 and 1 are the classic OK/RED pair; 2 is the #2898
# capability-refused convention (docker preflight — "not evidence about the
# code"). 3 is #2902: at least one known-failure test DID NOT EXECUTE inside a
# target that still ran. That state gets its own exit code because "known
# failure now passes" (exit 1, remove the entry) and "known failure vanished"
# (exit 3, restore the test) prescribe opposite operator actions, and a caller
# reading only the exit status must not be able to confuse them — exit 1's
# remediation applied to an exit-3 state is precisely the retire-a-guard-
# because-its-test-stopped-running hazard this gate exists to prevent. When
# both states occur in one run, 3 wins: the missing guard must never be masked
# by a co-occurring generic red.
EXIT_OK = 0
EXIT_RED = 1
EXIT_CAPABILITY_REFUSED = 2
EXIT_GUARD_NOT_EXECUTED = 3


@dataclass
class Observation:
    executed: set[Target]
    failures: dict[FailureKey, str]
    # #2902: every per-test libtest outcome line, keyed per test — "ok",
    # "FAILED", "ignored", "ignored, <reason>", ... `failures` records only
    # tests that FAILED and `test_counts` only sums per TARGET, so neither
    # can say whether one specific known-failure test actually ran. A known
    # failure absent from this map did not execute, which is NOT the same
    # state as passing and must not be reported as if it were.
    outcomes: dict[FailureKey, str]
    unknown_executables: list[str]
    # #2898 Blocker 3: a target's binary printing "Running tests/x.rs (...)"
    # proves cargo COMPILED and RAN the binary, not that the binary contained
    # any test. A target compiled out by a missing feature flag still prints
    # a "Running" line, then "running 0 tests" / "test result: ok. 0 passed;
    # 0 failed; 0 ignored" -- and that alone was enough for `executed` above
    # to accept it as covered. This maps each executed target to its actual
    # (passed, failed, ignored) counts so evaluate() can tell "ran and did
    # nothing" apart from "ran and proved something", which `executed` alone
    # cannot.
    test_counts: dict[Target, tuple[int, int, int]]


@dataclass(frozen=True)
class DockerPreflight:
    """A Docker capability result, separate from a Cargo test result."""

    available: bool
    reason: str
    detail: str = ""


def _target(entry: dict[str, Any]) -> Target:
    return (entry["package"], entry["target"])


def _failure(entry: dict[str, Any]) -> FailureKey:
    return (entry["package"], entry["target"], entry["test"])


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    )
    return json.loads(completed.stdout)


def discovered_targets(metadata: dict[str, Any]) -> set[Target]:
    found: set[Target] = set()
    for package in metadata["packages"]:
        for target in package["targets"]:
            if "test" in target["kind"]:
                found.add((package["name"], target["name"]))
    return found


def validate_manifest(
    manifest: dict[str, Any],
    discovered: set[Target],
    workflow_text: str,
    workflow_texts: dict[str, str] | None = None,
) -> list[str]:
    errors: list[str] = []
    if manifest.get("schema") != 1:
        errors.append("manifest schema must be 1")

    inventory_entries = manifest.get("inventory", [])
    inventory = [_target(entry) for entry in inventory_entries]
    inventory_set = set(inventory)
    if not inventory:
        errors.append("integration target inventory is empty")
    if len(inventory) != len(inventory_set):
        errors.append("integration target inventory contains duplicate entries")

    for entry in inventory_entries:
        review = entry.get("review")
        if review is None:
            continue
        target = _target(entry)
        if not isinstance(review, dict):
            errors.append(f"inventory review metadata must be an object: {target}")
            continue
        for field in ("source_pr", "owning_issue"):
            value = review.get(field)
            if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
                errors.append(
                    f"inventory review metadata {field} must be a positive integer: "
                    f"{target}"
                )
        covers = review.get("covers")
        if not isinstance(covers, str) or not covers.strip():
            errors.append(
                f"inventory review metadata covers must be non-empty: {target}"
            )

    added = sorted(discovered - inventory_set)
    removed = sorted(inventory_set - discovered)
    if added:
        errors.append(f"unreviewed discovered targets: {added}")
    if removed:
        errors.append(f"manifest targets no longer discovered: {removed}")

    external_entries = manifest.get("external_coverage", [])
    external = [_target(entry) for entry in external_entries]
    external_set = set(external)
    if len(external) != len(external_set):
        errors.append("external coverage contains duplicate targets")
    for entry in external_entries:
        target = _target(entry)
        step = entry["step"]
        if target not in inventory_set:
            errors.append(f"external target absent from inventory: {target}")
            continue
        # #2888: which workflow makes the claim. Unnamed means the historical
        # default (ci-readiness.yml); named must be repository-relative and
        # loaded by the caller. A claim against a workflow that is not in the
        # tree covers nothing and says so here, not at runtime.
        workflow_name = entry.get("workflow", DEFAULT_WORKFLOW_REL)
        if workflow_texts is not None and workflow_name in workflow_texts:
            text = workflow_texts[workflow_name]
        elif workflow_name == DEFAULT_WORKFLOW_REL:
            text = workflow_text
        else:
            errors.append(
                f"external target {target[1]} names workflow {workflow_name}, "
                "which does not exist in the tree; a coverage claim against a "
                "missing workflow covers nothing"
            )
            continue
        triggers = workflow_triggers(text)
        if not FIRING_EVENTS.intersection(triggers):
            errors.append(
                f"external target {target[1]} is covered by {workflow_name}, "
                f"which fires on {triggers or ['nothing']} only; a workflow that "
                "does not fire on push/pull_request covers nothing (#2888)"
            )
            continue
        lines = text.splitlines()
        id_line = next(
            (index for index, line in enumerate(lines) if line.strip() == f"id: {step}"),
            None,
        )
        if id_line is None:
            errors.append(
                f"external target {target} has no workflow step id {step} in {workflow_name}"
            )
            continue
        start = id_line
        while start > 0 and not lines[start].lstrip().startswith("- name:"):
            start -= 1
        end = id_line + 1
        while end < len(lines) and not lines[end].lstrip().startswith("- name:"):
            end += 1
        step_block = "\n".join(lines[start:end])
        if target[1] not in step_block:
            errors.append(
                f"workflow step {step} no longer names external target {target[1]}"
            )
        # The step's result has to reach a verdict. Two shapes do: the
        # ci-readiness summary pattern, which reads `steps.<id>.outcome` in a
        # result step and a summary step (hence 2), or a plain hard step whose
        # failure fails its job -- which is only true if neither the step nor
        # its job carries continue-on-error.
        references = text.count(f"steps.{step}.outcome")
        if references < 2:
            soft = _step_job_continues_on_error(text, step)
            if soft is None or soft:
                errors.append(
                    f"workflow step {step} in {workflow_name} has only {references} "
                    "verdict reference(s) and is not a hard step (continue-on-error, "
                    "or not parseable); its result reaches no verdict"
                )

    known_entries = manifest.get("known_failures", [])
    known = [_failure(entry) for entry in known_entries]
    if len(known) != len(set(known)):
        errors.append("known-failure manifest contains duplicate entries")
    for entry in known_entries:
        key = _failure(entry)
        target = key[:2]
        if target not in inventory_set:
            errors.append(f"known failure belongs to unknown target: {key}")
        if target in external_set:
            errors.append(f"known failure belongs to externally covered target: {key}")
        try:
            re.compile(entry["pattern"], re.DOTALL)
        except re.error as error:
            errors.append(f"invalid pattern for {key}: {error}")

    # #2898 Blocker 3: reviewed, shrink-only exceptions to the nonzero-test
    # floor (see evaluate()). Same discipline as known_failures -- an entry
    # must name a real selected target and carry a non-empty, human-written
    # reason, so this cannot become a silent dumping ground the way the
    # unguarded `executed` check already was.
    zero_entries = manifest.get("zero_test_exceptions", [])
    zero_targets = [_target(entry) for entry in zero_entries]
    if len(zero_targets) != len(set(zero_targets)):
        errors.append("zero-test-exception manifest contains duplicate entries")
    for entry in zero_entries:
        target = _target(entry)
        if target not in inventory_set:
            errors.append(f"zero-test exception belongs to unknown target: {target}")
        if target in external_set:
            errors.append(f"zero-test exception belongs to externally covered target: {target}")
        reason = entry.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            errors.append(f"zero-test exception for {target} has no reason")

    # #2898 Blocker 1 + 3: targets that need a live Docker daemon to assert
    # anything (bollard-backed execution-engine harnesses). Routed to a
    # separate, explicitly Docker-gated invocation (see docker_gated_targets()
    # and main()'s --docker-gated path) rather than run inside the main
    # --features hsm,zk invocation, where they either fail on an undeclared
    # host permission (basic_docker, #2893) or -- once docker-tests is
    # enabled -- would need Docker too and fail the same way. Same shrink-only
    # discipline as known_failures/zero_test_exceptions: a real target, a
    # reason, not a dumping ground.
    docker_entries = manifest.get("docker_gated", [])
    docker_targets = [_target(entry) for entry in docker_entries]
    if len(docker_targets) != len(set(docker_targets)):
        errors.append("docker_gated manifest contains duplicate entries")
    for entry in docker_entries:
        target = _target(entry)
        if target not in inventory_set:
            errors.append(f"docker_gated entry belongs to unknown target: {target}")
        if target in external_set:
            errors.append(f"docker_gated entry belongs to externally covered target: {target}")
        if target in zero_targets:
            errors.append(
                f"docker_gated entry {target} also listed in zero_test_exceptions -- "
                "pick one: either it needs Docker to assert anything (docker_gated) "
                "or it asserts nothing by design (zero_test_exceptions), not both"
            )
        reason = entry.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            errors.append(f"docker_gated entry for {target} has no reason")

    return errors


def selected_targets(manifest: dict[str, Any]) -> list[Target]:
    inventory = {_target(entry) for entry in manifest["inventory"]}
    external = {_target(entry) for entry in manifest.get("external_coverage", [])}
    docker_gated = {_target(entry) for entry in manifest.get("docker_gated", [])}
    return sorted(inventory - external - docker_gated)


def docker_gated_targets(manifest: dict[str, Any]) -> list[Target]:
    return sorted({_target(entry) for entry in manifest.get("docker_gated", [])})


def docker_preflight() -> DockerPreflight:
    """#2893/#2898: fail-closed Docker capability preflight.

    `docker version` (not `docker ps`, which needs a running container list
    the daemon may reject for other reasons) is the minimal real check: it
    round-trips to the daemon over the socket this runner's OS user must be
    able to read. A permission error here (the live #2893/#881 finding --
    `gh-runner` is not in the `docker` group) exits non-zero exactly like a
    genuinely absent daemon would, which is the correct behavior: this
    function answers "can the docker-gated targets actually assert
    anything," not "is a docker daemon running somewhere."
    """
    try:
        result = subprocess.run(
            ["docker", "version"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except FileNotFoundError:
        return DockerPreflight(
            False, "DOCKER_CLI_MISSING", "docker executable was not found"
        )
    except PermissionError as error:
        return DockerPreflight(False, "DOCKER_CLI_UNAVAILABLE", str(error))
    except subprocess.TimeoutExpired:
        return DockerPreflight(
            False, "DOCKER_PREFLIGHT_TIMEOUT", "docker version exceeded 30 seconds"
        )

    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        return DockerPreflight(
            False,
            "DOCKER_DAEMON_UNAVAILABLE",
            " ".join(detail.split())[:400],
        )
    return DockerPreflight(True, "DOCKER_READY")


def docker_available() -> bool:
    """Compatibility predicate for callers that only need the capability bit."""
    return docker_preflight().available


def build_cargo_command(targets: Iterable[Target], features: str = "hsm,zk") -> list[str]:
    command = [
        "cargo",
        "test",
        "--locked",
        "--workspace",
        "--features",
        features,
        "--no-fail-fast",
        "--message-format=json",
    ]
    # Target names are unique in the reviewed inventory. Repeating --test keeps
    # Cargo on integration targets only; plain --tests would also rerun every
    # library unit suite and hide this gate's actual selection.
    for _, target_name in targets:
        command.extend(["--test", target_name])
    command.extend(["--", "--test-threads=1"])
    return command


def run_cargo(command: list[str], timeout_seconds: int) -> tuple[int, list[str]]:
    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    environment.setdefault("WRFCOIN_RUN_TLS_INTEGRATION_TESTS", "0")
    # Developer machines may export a host-level sccache wrapper that is not
    # available inside CI/sandbox namespaces. The gate measures repository
    # tests, not wrapper reachability, so invoke rustc directly and uniformly.
    # An absent environment override still permits Cargo config to re-enable a
    # host wrapper.  An explicit empty value is Cargo's documented override for
    # invoking rustc directly.
    environment["RUSTC_WRAPPER"] = ""
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        output, _ = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        process.kill()
        output, _ = process.communicate()
        print(output, end="")
        raise RuntimeError(
            f"integration target command exceeded {timeout_seconds} seconds"
        )
    # Cargo's JSON artifact stream is parser input, not useful CI prose. Keep
    # human libtest output visible while avoiding hundreds of kilobytes of
    # dependency metadata in every gate log.
    for line in output.splitlines():
        if not line.startswith("{"):
            print(line)
    return process.returncode, output.splitlines()


def _normal_executable(value: str) -> str:
    path = Path(value)
    if not path.is_absolute():
        path = ROOT / path
    return str(path.resolve())


def _failure_diagnostic(segment: str, test_name: str) -> str:
    """The `---- <test> stdout ----` section for one failed test, or ''."""
    marker = f"---- {test_name} stdout ----"
    marker_at = segment.find(marker)
    if marker_at < 0:
        return ""
    remainder = segment[marker_at + len(marker):]
    boundaries = [
        boundary
        for boundary in (
            remainder.find("\n---- "),
            remainder.find("\nfailures:\n"),
        )
        if boundary >= 0
    ]
    return remainder[: min(boundaries) if boundaries else None].strip()


_FAILED_RESULT_LINE = re.compile(r"^test result: FAILED\.")
_FAILURE_LIST_NAME = re.compile(r"^    (\S.*?)\s*$")


def _failure_list_names(segment_lines: list[str]) -> list[str]:
    """#3276: names from libtest's closing `failures:` name list.

    For each `test result: FAILED.` line, take the LAST `failures:` line
    before it and collect the 4-space-indented names directly below it, up
    to the first line that is not one. The earlier `failures:` block (the one
    holding `---- <test> stdout ----` sections) is never the last one before
    the summary, so captured stdout cannot be mistaken for names.
    """
    names: list[str] = []
    for result_at, line in enumerate(segment_lines):
        if not _FAILED_RESULT_LINE.match(line):
            continue
        header_at = next(
            (
                i
                for i in range(result_at - 1, -1, -1)
                if segment_lines[i].strip() == "failures:"
            ),
            None,
        )
        if header_at is None:
            continue
        for candidate in segment_lines[header_at + 1 : result_at]:
            match = _FAILURE_LIST_NAME.match(candidate)
            if not match:
                break
            if match.group(1) not in names:
                names.append(match.group(1))
    return names


def parse_observation(
    lines: list[str],
    metadata: dict[str, Any],
    selected: set[Target],
) -> Observation:
    package_by_id = {package["id"]: package["name"] for package in metadata["packages"]}
    executable_map: dict[str, Target] = {}

    for line in lines:
        if not line.startswith("{"):
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact" or not message.get("executable"):
            continue
        target = message.get("target", {})
        if "test" not in target.get("kind", []):
            continue
        package = package_by_id.get(message.get("package_id"))
        if package is None:
            continue
        executable_map[_normal_executable(message["executable"])] = (
            package,
            target["name"],
        )

    basename_map: dict[str, Target | None] = {}
    for executable, target in executable_map.items():
        basename = Path(executable).name
        basename_map[basename] = (
            target if basename not in basename_map else None
        )

    running = re.compile(r"^\s*Running .+ \(([^)]+)\)\s*$")
    segments: dict[Target, list[str]] = {}
    unknown: list[str] = []
    current: Target | None = None
    for line in lines:
        match = running.match(line)
        if match:
            executable = _normal_executable(match.group(1))
            current = executable_map.get(executable)
            if current is None:
                current = basename_map.get(Path(executable).name)
            if current not in selected:
                unknown.append(match.group(1))
                current = None
            else:
                segments.setdefault(current, [])
            continue
        if current is not None:
            segments[current].append(line)

    failures: dict[FailureKey, str] = {}
    outcomes: dict[FailureKey, str] = {}
    # #2902: capture EVERY per-test outcome line, not just FAILED ones.
    # "test result: ..." summary lines lack the " ... " infix and never match.
    outcome_line = re.compile(r"^test (.+) \.\.\. (\S.*)$")
    for target, segment_lines in segments.items():
        segment = "\n".join(segment_lines)
        for line in segment_lines:
            match = outcome_line.match(line)
            if not match:
                continue
            test_name, outcome = match.group(1), match.group(2)
            outcomes[(target[0], target[1], test_name)] = outcome
            if outcome != "FAILED":
                continue
            failures[(target[0], target[1], test_name)] = _failure_diagnostic(
                segment, test_name
            )
        # #3276: the outcome line above only matches when libtest prints the
        # verdict on the SAME line as the test name. A test that re-execs its
        # own binary with inherited stdout prints `test X ... `, then the
        # child's output, and only then a bare `FAILED` line -- which matched
        # nothing, so a real failure never reached `failures` and was never
        # reported as unexpected (zk_key_persistence, #3200, hidden behind the
        # known #3131 red). libtest's closing name-list block (`failures:`
        # followed by 4-space-indented names, just before `test result:
        # FAILED`) is immune to that interleaving, so it is unioned in.
        for test_name in _failure_list_names(segment_lines):
            key = (target[0], target[1], test_name)
            outcomes.setdefault(key, "FAILED")
            if key not in failures:
                failures[key] = _failure_diagnostic(segment, test_name)

    # #2898 Blocker 3: sum every "test result:" summary line in a target's
    # segment. Normally there is exactly one per `--test` binary; summing
    # rather than taking the last is defensive against a binary that prints
    # more than one (a doctest-style sub-suite, a retry) without assuming
    # which is authoritative -- either way, (0, 0, 0) total means nothing in
    # this segment ever asserted anything, which is the fact this exists to
    # catch regardless of how many summary lines produced it.
    result_line = re.compile(
        r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
    )
    test_counts: dict[Target, tuple[int, int, int]] = {}
    for target, segment_lines in segments.items():
        passed = failed = ignored = 0
        for line in segment_lines:
            match = result_line.match(line)
            if match:
                passed += int(match.group(1))
                failed += int(match.group(2))
                ignored += int(match.group(3))
        test_counts[target] = (passed, failed, ignored)

    return Observation(
        executed=set(segments),
        failures=failures,
        outcomes=outcomes,
        unknown_executables=unknown,
        test_counts=test_counts,
    )


def unexecuted_known_failures(
    manifest: dict[str, Any],
    selected: set[Target],
    observation: Observation,
) -> list[FailureKey]:
    """#2902: known-failure entries whose test did not run in an executed target.

    A key lands here when it neither failed (not in ``failures``) nor printed
    an ``ok`` outcome, while its target DID execute — the #[ignore]d /
    filtered / renamed shape. Keys on targets that did not execute at all are
    excluded: those are owned by evaluate()'s missing_targets check. This is
    the single source for the state — evaluate() derives its error from it and
    main() derives EXIT_GUARD_NOT_EXECUTED from it, so the exit code cannot
    drift from the reported data.
    """
    expected = {
        _failure(entry)
        for entry in manifest.get("known_failures", [])
        if _failure(entry)[:2] in selected
    }
    return sorted(
        key
        for key in expected - set(observation.failures)
        if observation.outcomes.get(key) != "ok"
        and key[:2] in observation.executed
    )


def evaluate(
    manifest: dict[str, Any],
    selected: set[Target],
    observation: Observation,
    cargo_returncode: int,
) -> list[str]:
    errors: list[str] = []
    missing_targets = sorted(selected - observation.executed)
    extra_targets = sorted(observation.executed - selected)
    if missing_targets:
        errors.append(f"selected targets did not execute: {missing_targets}")
    if extra_targets:
        errors.append(f"unselected targets executed: {extra_targets}")
    if observation.unknown_executables:
        errors.append(
            f"unmapped/unselected Cargo executables ran: {observation.unknown_executables}"
        )

    # #2898 Blocker 3: a target printing "Running ..." (the sole basis for
    # `executed` above) proves cargo compiled and ran the binary -- not that
    # the binary asserted anything. A target compiled out by a missing
    # feature flag runs 0 tests and reports "0 passed; 0 failed; 0 ignored",
    # which `executed` alone accepted as covered. That is vacuity inside the
    # instrument built to catch vacuity. A target reporting (0, 0, 0) is RED
    # unless a reviewed, named entry in `zero_test_exceptions` says why that
    # specific target is legitimately test-free.
    #
    # Scoped to `selected`: the manifest's zero_test_exceptions and
    # known_failures lists cover the WHOLE inventory, but a single evaluate()
    # call only ever observes ONE invocation's selected subset (#2898 added a
    # second, --docker-gated invocation of a different subset). An entry for
    # a target this invocation didn't select is neither stale nor satisfied
    # -- it is simply not this invocation's business. Without this scoping,
    # running the docker-gated subset made every OTHER target's known
    # failures and zero-test exceptions look stale, since none of them ran
    # here either -- a real bug, caught by running this invocation for real
    # against the actual manifest, not just in a synthetic unit test.
    zero_exceptions = {
        _target(entry)
        for entry in manifest.get("zero_test_exceptions", [])
        if _target(entry) in selected
    }
    zero_test_targets = sorted(
        target
        for target in selected & observation.executed
        for passed, failed, _ignored in [observation.test_counts.get(target, (0, 0, 0))]
        if passed == 0 and failed == 0
    )
    unreviewed_zero = sorted(set(zero_test_targets) - zero_exceptions)
    if unreviewed_zero:
        errors.append(
            "selected targets executed but asserted nothing (0 passed, 0 failed) "
            f"and are not a reviewed zero_test_exceptions entry: {unreviewed_zero}"
        )
    stale_zero_exceptions = sorted(zero_exceptions - set(zero_test_targets))
    if stale_zero_exceptions:
        errors.append(
            "zero_test_exceptions entries now execute real tests; remove the "
            f"stale exception (the target is no longer test-free): {stale_zero_exceptions}"
        )

    # Same scoping as zero_exceptions above, and for the same reason: a
    # known_failures entry for a target outside this invocation's `selected`
    # set was never given the chance to occur here.
    expected_entries = {
        _failure(entry): entry
        for entry in manifest.get("known_failures", [])
        if _failure(entry)[:2] in selected
    }
    expected = set(expected_entries)
    actual = set(observation.failures)
    unexpected = sorted(actual - expected)
    if unexpected:
        errors.append(f"unexpected integration failures: {unexpected}")

    # #3276 fail-closed cross-check: libtest's own summary is the count of
    # record. If a target reports more failed tests than the parser could
    # attribute to named tests, some failure is invisible to the known /
    # unexpected comparison above -- exactly how zk_key_persistence (#3200)
    # stayed hidden behind the known #3131 red. Never let that pass silently.
    unattributed = []
    for target in sorted(selected & observation.executed):
        _passed, failed, _ignored = observation.test_counts.get(target, (0, 0, 0))
        attributed = sum(1 for key in actual if key[:2] == target)
        if failed > attributed:
            unattributed.append((target[0], target[1], failed, attributed))
    if unattributed:
        errors.append(
            "failed tests the parser could not attribute to a named test "
            "(package, target, summary failed, attributed): "
            f"{unattributed}"
        )

    # #2902: `expected - actual` alone is satisfied identically by "the test
    # now passes" and by "the test never ran" (#[ignore]d, filtered, or
    # renamed inside a target that still executed). Those demand OPPOSITE
    # operator actions — retire the entry vs. restore the test — so split on
    # the per-test outcome record instead of reporting them as one state.
    # Target-level non-execution stays owned by the missing_targets check at
    # the top of this function: a key whose whole target did not run is
    # already RED there, and repeating it per test would report the same
    # fact at two granularities.
    stale_unexecuted = unexecuted_known_failures(manifest, selected, observation)
    stale_passed = [
        key
        for key in sorted(expected - actual)
        if observation.outcomes.get(key) == "ok"
    ]
    if stale_passed:
        errors.append(
            f"known failures now pass; remove/fix entries: {stale_passed}"
        )
    if stale_unexecuted:
        errors.append(
            "known-failure tests did not execute (ignored, filtered, or "
            "renamed); restore the tests — removing their manifest entries "
            f"would retire a guard whose test vanished: {stale_unexecuted}"
        )

    for key in sorted(actual & expected):
        diagnostic = observation.failures[key]
        if not diagnostic:
            errors.append(f"failure {key} has no captured diagnostic section")
            continue
        pattern = expected_entries[key]["pattern"]
        if re.search(pattern, diagnostic, re.DOTALL) is None:
            errors.append(
                f"failure reason changed for {key}; pattern {pattern!r} did not match"
            )

    if cargo_returncode not in (0, 101):
        errors.append(f"Cargo exited {cargo_returncode}, not a test-result code")
    if expected and cargo_returncode == 0:
        errors.append("Cargo exited 0 while known failures remain in the manifest")
    if not expected and cargo_returncode != 0:
        errors.append("Cargo failed even though the known-failure manifest is empty")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--workflow", type=Path, default=DEFAULT_WORKFLOW)
    parser.add_argument("--timeout-seconds", type=int, default=3_600)
    parser.add_argument("--validate-only", action="store_true")
    # #2898 Blocker 1: run ONLY the Docker-gated targets, with docker-tests
    # enabled, behind a fail-closed capability preflight. Separate invocation
    # from the default path (which now excludes these targets entirely --
    # see selected_targets()) so a Docker outage is attributable to exactly
    # the targets that need Docker, not smeared across all 104.
    parser.add_argument("--docker-gated", action="store_true")
    args = parser.parse_args()

    manifest = load_json(args.manifest)
    metadata = cargo_metadata()
    discovered = discovered_targets(metadata)
    workflow_text = args.workflow.read_text(encoding="utf-8")
    # #2888: every workflow an external_coverage entry names, loaded from the
    # tree so the validator can prove it fires. A missing file is left out of
    # the map on purpose; validate_manifest reports it as covering nothing.
    workflow_texts: dict[str, str] = {}
    for entry in manifest.get("external_coverage", []):
        name = entry.get("workflow", DEFAULT_WORKFLOW_REL)
        path = ROOT / name
        if name not in workflow_texts and path.is_file():
            workflow_texts[name] = path.read_text(encoding="utf-8")
    errors = validate_manifest(manifest, discovered, workflow_text, workflow_texts)
    from node_share_matrix import validate_registration
    has_native_chain = any(package["name"] == "wrfcoin-native-chain" for package in metadata["packages"])
    if has_native_chain:
        errors.extend(validate_registration(manifest))

    print(f"INTEGRATION_TARGET_INVENTORY count={len(discovered)}", flush=True)
    external = {
        _target(entry): entry["step"]
        for entry in manifest.get("external_coverage", [])
    }
    docker_gated_set = {_target(entry) for entry in manifest.get("docker_gated", [])}
    zero_exception_set = {
        _target(entry) for entry in manifest.get("zero_test_exceptions", [])
    }
    for target in sorted(discovered):
        if target in external:
            coverage = f"external-step:{external[target]}"
        elif target in docker_gated_set:
            coverage = "docker-gated"
        elif target in zero_exception_set:
            coverage = "ratchet-run,zero-test-exception"
        else:
            coverage = "ratchet-run"
        print(f"  {target[0]}::{target[1]} [{coverage}]", flush=True)

    if errors:
        for error in errors:
            print(f"INTEGRATION_RATCHET_ERROR {error}", file=sys.stderr)
        return EXIT_RED
    if args.validate_only:
        print(
            "INTEGRATION_TARGET_INVENTORY_OK "
            f"inventory={len(discovered)} external={len(external)}"
        )
        return EXIT_OK

    if args.docker_gated:
        targets = docker_gated_targets(manifest)
        if not targets:
            print("INTEGRATION_RATCHET_ERROR --docker-gated: manifest has no docker_gated targets", file=sys.stderr)
            return EXIT_RED
        # FAIL-CLOSED PREFLIGHT (#2898 Blocker 1). This must refuse loudly,
        # not silently pass and not produce a confusing low-level cargo/bollard
        # connection error several minutes into a compile. Exit 2: same
        # "this is not evidence about the code" convention as the disk
        # preflight in integration-ratchet.yml -- a capability gap, not a
        # test result.
        preflight = docker_preflight()
        if not preflight.available:
            print(
                "::error::DOCKER_PREFLIGHT_REFUSED "
                f"reason={preflight.reason} — `docker version` failed for this "
                "runner user. Docker-gated targets "
                f"({[f'{p}::{t}' for p, t in targets]}) require a working Docker "
                "daemon connection to assert anything; refusing to run them rather "
                "than report a silent pass or a confusing low-level test failure. "
                "This is an environment refusal, not execution-engine evidence. "
                f"detail={preflight.detail or 'no diagnostic returned'} "
                "(core4#2893; infra#881).",
                file=sys.stderr,
            )
            return EXIT_CAPABILITY_REFUSED
        print(
            "DOCKER_PREFLIGHT_OK "
            f"reason={preflight.reason} docker version succeeded for this runner user"
        )
        command = build_cargo_command(targets, features="hsm,zk,docker-tests")
    else:
        targets = selected_targets(manifest)
        command = build_cargo_command(targets)
    mode = "docker-gated" if args.docker_gated else "main"
    print(
        f"INTEGRATION_TARGET_SELECTION mode={mode} run={len(targets)} external={len(external)}",
        flush=True,
    )
    try:
        returncode, output_lines = run_cargo(command, args.timeout_seconds)
    except RuntimeError as error:
        print(f"INTEGRATION_RATCHET_ERROR {error}", file=sys.stderr)
        return EXIT_RED

    observation = parse_observation(output_lines, metadata, set(targets))

    # #2898 Blocker 3: explicit, always-printed count -- not just something
    # evaluate() folds into a pass/fail decision. A reviewer (or a future
    # regression against this exact class of vacuity) should be able to read
    # this number off every run's log without having to parse an error list.
    zero_exceptions = {
        _target(entry) for entry in manifest.get("zero_test_exceptions", [])
    }
    zero_test_targets = sorted(
        target
        for target in set(targets) & observation.executed
        for passed, failed, _ignored in [observation.test_counts.get(target, (0, 0, 0))]
        if passed == 0 and failed == 0
    )
    print(
        f"ZERO_TEST_SUMMARY_COUNT total={len(zero_test_targets)} "
        f"reviewed_exceptions={len(zero_exceptions)} "
        f"unreviewed={len(set(zero_test_targets) - zero_exceptions)}"
    )
    for target in zero_test_targets:
        reviewed = "reviewed" if target in zero_exceptions else "UNREVIEWED"
        _passed, _failed, ignored = observation.test_counts.get(target, (0, 0, 0))
        # Distinguish genuinely-empty (0 passed; 0 failed; 0 ignored -- e.g.
        # stress_tests, no #[test] functions at all) from real tests that
        # exist but are all #[ignore]d pending hardware/live infra this
        # runner doesn't have (0 passed; 0 failed; N ignored, N>0 -- e.g.
        # hsm_signer, yubikey_signer, cold_start_convergence). Both are RED
        # without a reviewed exception; the shape is worth seeing at a
        # glance, not just the pass/fail totals.
        shape = f"0 passed; 0 failed; {ignored} ignored" if ignored else "genuinely empty"
        print(f"  ZERO_TEST {target[0]}::{target[1]} [{reviewed}] ({shape})")

    errors = evaluate(manifest, set(targets), observation, returncode)
    # Scoped to this invocation's own selection, same reason and same fix as
    # evaluate()'s internal scoping: printing a manifest-wide known_failures
    # entry as "stale/missing" here when it simply belongs to a target this
    # invocation didn't select (e.g. --docker-gated's 4-target run, next to
    # the main run's other 100) reads as a false alarm, even though it isn't
    # scored as one.
    expected = {
        _failure(entry)
        for entry in manifest.get("known_failures", [])
        if _failure(entry)[:2] in set(targets)
    }
    # #2902: the old joint "stale/missing" label hid which of two opposite
    # states an entry was in. The outcome record now says which.
    for key in sorted(expected):
        if key in observation.failures:
            state = "observed"
        else:
            outcome = observation.outcomes.get(key)
            if outcome == "ok":
                state = "passed"
            elif outcome is None:
                state = "did-not-run"
            else:
                state = f"did-not-run:{outcome}"
        print(f"KNOWN_FAILURE {key[0]}::{key[1]}::{key[2]} [{state}]")
    if errors:
        for error in errors:
            print(f"INTEGRATION_RATCHET_ERROR {error}", file=sys.stderr)
        # #2902: the two stale states must differ at the PROCESS level, not
        # just in message text — a caller reading only the exit status must
        # not be able to apply exit 1's "remove/fix entries" remediation to a
        # vanished-guard run. Derived from the same function evaluate() used,
        # so exit code and reported data cannot disagree.
        if unexecuted_known_failures(manifest, set(targets), observation):
            return EXIT_GUARD_NOT_EXECUTED
        return EXIT_RED

    if has_native_chain and not args.docker_gated:
        from tempfile import TemporaryDirectory
        from node_share_matrix import run as run_node_share_matrix
        with TemporaryDirectory(prefix="node-share-ratchet-") as directory:
            run_node_share_matrix(Path(directory) / "node-share-roots.json")

    print(
        "INTEGRATION_TARGET_RATCHET_OK "
        f"inventory={len(discovered)} executed={len(observation.executed)} "
        f"external={len(external)} known_broken={len(expected)}"
    )
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
