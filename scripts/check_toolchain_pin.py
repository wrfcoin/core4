#!/usr/bin/env python3
"""Verify that Rust CI and container build paths honor rust-toolchain.toml."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NamedTuple

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover - exercised by the CI guard below
    yaml = None  # type: ignore[assignment]


EXACT_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
RUSTC_VERSION = re.compile(r"^rustc (?P<version>\d+\.\d+\.\d+)(?:\s|$)")
CARGO_BUILD_COMMAND = re.compile(
    r"\bcargo\s+(?:\+[A-Za-z0-9._-]+\s+)?(?:build|check|test|clippy)\b"
)
RUST_ACTION = re.compile(r"uses:\s*dtolnay/rust-toolchain@(?P<channel>\S+)")
TOOLCHAIN_VALUE = re.compile(r"^\s*toolchain:\s*['\"]?(?P<value>[^'\"\s#]+)")
RUST_VERSION_VALUE = re.compile(r"^\s*RUST_VERSION:\s*['\"]?(?P<value>[^'\"\s#]+)")
DOCKER_RUST_IMAGE = re.compile(r"^\s*FROM\s+rust:(?P<version>\d+\.\d+(?:\.\d+)?)")
# `toolchain: ${{ <expr> }}` — captures the expression body so a matrix-driven
# toolchain can be told apart from an env-driven one.
TOOLCHAIN_EXPRESSION = re.compile(
    r"^\s*toolchain:\s*\$\{\{\s*(?P<body>[^}]+?)\s*\}\}"
)
MATRIX_REFERENCE = re.compile(r"^matrix\.(?P<key>[A-Za-z0-9_-]+)$")
# NOTE: the `JOB_KEY` / `JOBS_BLOCK` / `MATRIX_BLOCK` line regexes and their
# `_indent` helper are deliberately GONE (#2419). They located job and matrix
# boundaries by scanning text, and three review rounds each found a different
# valid YAML shape they could not see — the last a quoted job key, which let one
# job's pin be attributed to another. Ownership is read from the parsed document
# in `_rust_action_owners`. Do not reintroduce a line regex for structure here;
# if a shape is not being recognised, the parser already knows the answer.


def _mapping_get(node, name: str):
    """Value node for key `name` in a MappingNode, or None."""
    if not isinstance(node, yaml.MappingNode):
        return None
    for key_node, value_node in node.value:
        if isinstance(key_node, yaml.ScalarNode) and key_node.value == name:
            return value_node
    return None


def _node_to_py(node):
    """Composed node -> plain Python, so matrix data keeps YAML's own semantics."""
    if isinstance(node, yaml.ScalarNode):
        return node.value
    if isinstance(node, yaml.SequenceNode):
        return [_node_to_py(item) for item in node.value]
    if isinstance(node, yaml.MappingNode):
        return {_node_to_py(k): _node_to_py(v) for k, v in node.value}
    return None


class _Occurrence(NamedTuple):
    """One logical (job, Rust action) pair read from the parsed document.

    The identity of a record is the PAIR, never the source line. GitHub
    officially supports YAML anchors and aliases for shared workflow
    configuration, so one anchored `steps:` list — and therefore one `uses:`
    node carrying one `start_mark` — can be executed by several jobs under
    several different matrices.

    Keying these by `start_mark.line` collapsed them into a single record and
    the last job traversed won, which recreated cross-job pin donation in a new
    shape: a pinned decoy overwrote an off-pin job's owner and donated its pin
    (review of f772e4301). Line marks are diagnostic metadata here and nothing
    more.
    """

    job_id: str
    matrix: object
    uses: str
    toolchain: str | None
    toolchain_line: int | None
    uses_line: int


def _rust_action_owners(path: Path) -> list[_Occurrence] | None:
    """Every (job, Rust action) occurrence in the document, in document order.

    Ownership comes from the PARSED document, not from scanning lines (#2419).

    Three rounds of review died on the line-scanning version, each on a
    different but perfectly valid YAML shape the job-key regex did not accept
    as a boundary:

    1. unresolved ownership widened to the whole workflow, letting an unrelated
       job donate the pin;
    2. the `None` path and the reviewer's exact fixture were fixed;
    3. a QUOTED job key (`"offpin":`) still slipped through, because the regex
       only recognised a bare `[A-Za-z0-9_-]+:` form, so the action was
       attributed to the PREVIOUS job and inherited its pin while actually
       running a different toolchain.

    Patching in each new shape is a losing game — the next one is a flow
    mapping, an anchor, or a key carrying a comment. The parser already knows
    exactly which job owns which step, so ownership is read from it. Every
    shape YAML considers a job key is therefore a boundary here by
    construction, including ones nobody has thought to write yet.

    Line numbers still come from the node marks, so diagnostics are unchanged.

    Returns None when the document cannot be classified at all — it does not
    parse, `jobs:` is not a mapping, or a job key is not a scalar. The caller
    turns that into an unconditional error: a file this function cannot
    understand must be REPORTED, never read half-blind and reported clean.
    """
    try:
        root = yaml.compose(path.read_text(encoding="utf-8"))
    except yaml.YAMLError:
        return None
    if root is None:
        return []
    jobs = _mapping_get(root, "jobs")
    if jobs is None:
        # No `jobs:` mapping at all. Not an error by itself: fixtures and
        # fragments exist. Callers only consult these records for matrix-driven
        # actions, and a missing occurrence there fails closed.
        return []
    if not isinstance(jobs, yaml.MappingNode):
        return None
    occurrences: list[_Occurrence] = []
    for key_node, job_node in jobs.value:
        if not isinstance(key_node, yaml.ScalarNode):
            return None  # a non-scalar job key is a shape we will not guess at
        job_id = key_node.value
        strategy = _mapping_get(job_node, "strategy")
        matrix = _mapping_get(strategy, "matrix") if strategy is not None else None
        steps = _mapping_get(job_node, "steps")
        if not isinstance(steps, yaml.SequenceNode):
            continue
        for step in steps.value:
            uses = _mapping_get(step, "uses")
            if not isinstance(uses, yaml.ScalarNode):
                continue
            if not str(uses.value).startswith("dtolnay/rust-toolchain@"):
                continue
            with_node = _mapping_get(step, "with")
            tc = _mapping_get(with_node, "toolchain") if with_node is not None else None
            # APPEND, never assign by line. An aliased step list yields the same
            # `uses` node — and the same start_mark — once per job that reuses
            # it; each of those is a real leg GitHub will run, under that job's
            # own matrix.
            occurrences.append(
                _Occurrence(
                    job_id=job_id,
                    matrix=matrix,
                    uses=str(uses.value),
                    toolchain=(
                        str(tc.value) if isinstance(tc, yaml.ScalarNode) else None
                    ),
                    toolchain_line=(
                        tc.start_mark.line if isinstance(tc, yaml.ScalarNode) else None
                    ),
                    uses_line=uses.start_mark.line,
                )
            )
    return occurrences


def _matrix_errors(
    occurrence: _Occurrence, key: str, pin: str, report_line: int
) -> list[str]:
    """Check ONE (job, action) occurrence's matrix axis against the pin.

    Shared by both discovery paths so an aliased occurrence is judged by its
    OWN job's matrix, and the job is named — with aliases, several occurrences
    can report against the same source line and must stay tellable apart.
    """
    where = f"line {report_line}: job {occurrence.job_id!r}"
    candidates, unresolved = _matrix_values(occurrence.matrix, key)
    if unresolved is not None:
        return [f"{where}: {unresolved}"]
    errors = [
        f"{where}: matrix toolchain {candidate!r} is not an exact version"
        for candidate in candidates
        if not EXACT_VERSION.fullmatch(candidate)
    ]
    if pin not in candidates:
        errors.append(
            f"{where}: matrix toolchain never exercises the pinned {pin!r}"
        )
    return errors


def _matrix_values(matrix, key: str) -> tuple[list[str], str | None]:
    """Values of matrix axis `key` that GitHub will actually execute.

    Returns `(effective_values, unresolvable_reason)`. A non-None reason means
    the shape could not be resolved and the caller must fail closed.

    The three sources are not interchangeable and must be told apart, because
    only some of them produce runs:

    * the axis itself expands into legs;
    * `include:` entries add legs;
    * `exclude:` entries REMOVE legs, so a value appearing only there is never
      run.

    Treating every key under `matrix:` as a candidate accepts a workflow whose
    axis is `['1.87.0']` with `exclude: [{toolchain: '1.88.0'}]`: the pinned
    value is "present" but never executes (review of `1b1f2a2304`). `exclude`
    is subtracted rather than ignored; where that is over-strict it fails
    closed, which is the safe direction for a reproducibility gate.
    """
    if matrix is None:
        return [], f"job declares no strategy.matrix, so {key!r} cannot be resolved"
    data = _node_to_py(matrix)
    if not isinstance(data, dict):
        return [], f"strategy.matrix is not a mapping, so {key!r} cannot be resolved"

    def _listify(value) -> list[str]:
        if value is None:
            return []
        if isinstance(value, list):
            return [str(item) for item in value if isinstance(item, (str, int, float))]
        if isinstance(value, (str, int, float)):
            return [str(value)]
        return []

    def _from_section(name: str) -> list[str]:
        section = data.get(name)
        if not isinstance(section, list):
            return []
        found: list[str] = []
        for entry in section:
            if isinstance(entry, dict) and key in entry:
                found.extend(_listify(entry[key]))
        return found

    declared = _listify(data.get(key)) + _from_section("include")
    excluded = set(_from_section("exclude"))
    if not declared:
        if excluded:
            return [], (
                f"matrix axis {key!r} appears only under `exclude:` — nothing "
                "declares a value that actually runs"
            )
        return [], f"matrix axis {key!r} has no resolvable values"
    effective = [value for value in declared if value not in excluded]
    if not effective:
        return [], f"every declared value of matrix axis {key!r} is excluded"
    return effective, None


def _pin(root: Path) -> tuple[str | None, list[str]]:
    path = root / "rust-toolchain.toml"
    if not path.is_file():
        return None, ["rust-toolchain.toml: missing"]

    text = path.read_text(encoding="utf-8")
    match = re.search(r'^channel\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        return None, ["rust-toolchain.toml: missing channel"]
    pin = match.group(1)
    errors: list[str] = []
    if not EXACT_VERSION.fullmatch(pin):
        errors.append(f"rust-toolchain.toml: channel must be exact, found {pin!r}")

    for required in ("rustfmt", "clippy", "rust-src"):
        if not re.search(rf'components\s*=\s*\[[^\]]*"{required}"', text):
            errors.append(f"rust-toolchain.toml: missing component {required!r}")
    for required in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
        if not re.search(rf'targets\s*=\s*\[[^\]]*"{required}"', text):
            errors.append(f"rust-toolchain.toml: missing target {required!r}")
    return pin, errors


def _runtime_rustc_errors(
    root: Path, command: tuple[str, ...] = ("rustc", "--version")
) -> tuple[str | None, list[str]]:
    """Compare the compiler that actually runs with rust-toolchain.toml.

    This is intentionally a process-boundary check. Static workflow inspection
    cannot detect a base image whose newer compiler is silently overridden by
    rust-toolchain.toml, which is the false-green boundary this assertion owns.
    """
    pin, pin_errors = _pin(root)
    if pin is None:
        return None, pin_errors

    try:
        result = subprocess.run(
            command,
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        return None, [f"runtime rustc --version could not run: {error}"]

    reported = result.stdout.strip()
    if result.returncode != 0:
        detail = result.stderr.strip() or reported or "no output"
        return reported or None, [
            f"runtime rustc --version failed with exit {result.returncode}: {detail}"
        ]

    match = RUSTC_VERSION.match(reported)
    if match is None:
        return reported or None, [
            f"runtime rustc --version returned an unrecognized value: {reported!r}"
        ]

    actual = match.group("version")
    if actual != pin:
        return reported, [
            f"runtime rustc version {actual!r} does not match "
            f"rust-toolchain.toml channel {pin!r}"
        ]
    return reported, []


def _workflow_errors(path: Path, pin: str) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    errors: list[str] = []
    # Structural job ownership: one record per (job, Rust action) occurrence.
    occurrences = _rust_action_owners(path)
    if occurrences is None:
        # Fail CLOSED, LOUDLY, and BEFORE any line-regex fallback (#2419).
        #
        # Silence here was the same fail-open defect one level up: a document
        # PyYAML could not parse produced no structural map, and if its Rust
        # action was quoted the line regex could not see it either, so an
        # unparseable workflow read as clean (review of f772e4301).
        #
        # A checker that cannot understand a file must say so. Refusing to
        # judge it is the fail-closed principle this whole gate exists to
        # establish; degrading to a partial scan is how it became a false
        # green in the first place.
        return [
            "could not be classified: the document does not parse as YAML, or "
            "`jobs:` is not a mapping, or a job key is not a scalar. The "
            "checker will not fall back to line scanning — fix the file so "
            "ownership can be read structurally."
        ]
    # Line marks are a lookup convenience only; several occurrences legitimately
    # share one line when jobs reuse an anchored step list.
    by_line: dict[int, list[_Occurrence]] = {}
    for occurrence in occurrences:
        by_line.setdefault(occurrence.uses_line, []).append(occurrence)
    env_versions = {
        match.group("value")
        for line in lines
        if (match := RUST_VERSION_VALUE.match(line))
    }
    for version in env_versions:
        if version != pin:
            errors.append(f"RUST_VERSION {version!r} does not match {pin!r}")

    handled: set[int] = set()
    for index, line in enumerate(lines):
        action = RUST_ACTION.search(line)
        if action:
            handled.add(index)
        if action and action.group("channel") != "nightly":
            value: str | None = None
            value_line: str = ""
            for following in lines[index + 1 : index + 9]:
                if re.match(r"^\s*-\s+(?:name|uses|run):", following):
                    break
                if match := TOOLCHAIN_VALUE.match(following):
                    value = match.group("value")
                    value_line = following
                    break
            if value is None:
                errors.append(
                    f"line {index + 1}: stable Rust action has no explicit toolchain"
                )
            elif value == "${{":
                expression = TOOLCHAIN_EXPRESSION.match(value_line)
                body = expression.group("body") if expression else ""
                if reference := MATRIX_REFERENCE.match(body):
                    # A matrix-supplied toolchain is legitimate: determinism
                    # matrices deliberately build on a NON-pinned compiler to
                    # prove state roots do not depend on compiler version
                    # (#2104). Pinning that axis to `pin` would make the
                    # toolchain dimension vacuous. What still must hold is that
                    # every leg names an EXACT version — never a floating
                    # `stable`/`nightly` — and that the pinned toolchain is
                    # actually among the legs, so the matrix cannot silently
                    # drift entirely off-pin.
                    key = reference.group("key")
                    matched = by_line.get(index, [])
                    if not matched:
                        # Fail CLOSED. Widening to the whole file here is what
                        # let an unrelated job donate the pin (review of
                        # e147c38b1) — an unattributable action is an error, not
                        # a licence to look everywhere.
                        errors.append(
                            f"line {index + 1}: cannot determine which job owns "
                            f"this action, so matrix {body!r} cannot be resolved"
                        )
                        continue
                    # EVERY occupant of this line, not just one. Under an alias
                    # this single line is several legs across several jobs, and
                    # a pinned one must not excuse an off-pin sibling.
                    for occurrence in matched:
                        errors.extend(
                            _matrix_errors(occurrence, key, pin, index + 1)
                        )
                elif env_versions != {pin}:
                    errors.append(
                        f"line {index + 1}: toolchain environment does not resolve to {pin!r}"
                    )
            elif value != pin:
                errors.append(
                    f"line {index + 1}: toolchain {value!r} does not match {pin!r}"
                )

        stripped = line.strip()
        if (
            CARGO_BUILD_COMMAND.search(line)
            and not stripped.startswith("#")
            and not stripped.startswith("- name:")
            and not stripped.startswith("name:")
            and "--locked" not in line
        ):
            errors.append(
                f"line {index + 1}: Cargo build/check/test/clippy command lacks --locked"
            )

        if re.search(r"\brustup\s+(?:default|toolchain\s+install)\s+stable\b", line):
            errors.append(f"line {index + 1}: unpinned rustup stable toolchain")

    # Actions the LINE regex cannot see at all — a quoted or flow-mapped
    # `uses: 'dtolnay/rust-toolchain@master'` is a Rust action to YAML but not
    # to `RUST_ACTION`. Discovering them only by regex reproduced the ownership
    # bug one level up: the step was invisible, so nothing was checked and the
    # workflow passed silently. The parser sees them, so they are swept here.
    for occurrence in occurrences:
        if occurrence.uses_line in handled:
            continue
        uses, value = occurrence.uses, occurrence.toolchain
        if uses.rsplit("@", 1)[-1] == "nightly":
            continue
        report_line = (
            occurrence.toolchain_line
            if occurrence.toolchain_line is not None
            else occurrence.uses_line
        ) + 1
        if value is None:
            errors.append(
                f"line {report_line}: stable Rust action has no explicit toolchain"
            )
            continue
        expression = re.match(r"^\$\{\{\s*(?P<body>[^}]+?)\s*\}\}$", value.strip())
        if expression is None:
            if value != pin:
                errors.append(
                    f"line {report_line}: toolchain {value!r} does not match {pin!r}"
                )
            continue
        body = expression.group("body")
        reference = MATRIX_REFERENCE.match(body)
        if reference is None:
            if env_versions != {pin}:
                errors.append(
                    f"line {report_line}: toolchain environment does not resolve to {pin!r}"
                )
            continue
        errors.extend(
            _matrix_errors(occurrence, reference.group("key"), pin, report_line)
        )

    return errors


def _dockerfile_errors(path: Path, pin: str) -> list[str]:
    errors: list[str] = []
    for index, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if match := DOCKER_RUST_IMAGE.match(line):
            version = match.group("version")
            if version != pin:
                errors.append(
                    f"line {index}: Rust image {version!r} does not match {pin!r}"
                )
        if re.search(r"\brustup\s+(?:default|toolchain\s+install)\s+stable\b", line):
            errors.append(f"line {index}: unpinned rustup stable toolchain")
        versions = re.findall(r"\brustup\s+(?:default|toolchain\s+install)\s+(\d+\.\d+\.\d+)\b", line)
        for version in versions:
            if version != pin:
                errors.append(
                    f"line {index}: rustup toolchain {version!r} does not match {pin!r}"
                )
    return errors


def check(root: Path) -> list[str]:
    if yaml is None:
        # Fail CLOSED, loudly. Job ownership is now read from the parsed
        # document (#2419); without a parser this checker cannot tell which job
        # owns a matrix-driven action, and the shape it would fall back to —
        # scanning lines — is precisely the defect three review rounds removed.
        # Refusing to run is correct; degrading silently is how the gate became
        # a false green in the first place.
        return [
            "PyYAML is not importable, so job ownership cannot be resolved. "
            "Install it in this job (`pip install pyyaml==6.0.2`) — the checker "
            "will not fall back to line scanning."
        ]

    pin, errors = _pin(root)
    if pin is None:
        return errors

    lockfile = root / "Cargo.lock"
    if not lockfile.is_file():
        errors.append("Cargo.lock: missing")
    elif (root / ".git").exists():
        tracked = subprocess.run(
            ["git", "ls-files", "--error-unmatch", "--", "Cargo.lock"],
            cwd=root,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if tracked.returncode != 0:
            errors.append("Cargo.lock: not tracked by Git")
        ignored = subprocess.run(
            ["git", "check-ignore", "-q", "Cargo.lock"],
            cwd=root,
            check=False,
        )
        if ignored.returncode == 0:
            errors.append("Cargo.lock: ignored by Git")

    workflow_dir = root / ".github" / "workflows"
    for path in sorted([*workflow_dir.glob("*.yml"), *workflow_dir.glob("*.yaml")]):
        errors.extend(
            f"{path.relative_to(root)}: {error}"
            for error in _workflow_errors(path, pin)
        )

    dockerfiles = sorted(
        path
        for path in root.rglob("Dockerfile*")
        if ".git" not in path.parts and "target" not in path.parts
    )
    for path in dockerfiles:
        errors.extend(
            f"{path.relative_to(root)}: {error}"
            for error in _dockerfile_errors(path, pin)
        )
    return errors


def self_test() -> None:
    # These deliberately synthetic 1.88/1.87 fixtures are independent of the
    # repository's current pin; keeping an older pair proves the checker is
    # generic rather than accidentally hard-coded to today's 1.98 policy.
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / ".github" / "workflows").mkdir(parents=True)
        (root / "docker").mkdir()
        (root / "Cargo.lock").write_text("# lock\n", encoding="utf-8")
        (root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.88.0"\n'
            'components = ["rustfmt", "clippy", "rust-src"]\n'
            'targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]\n'
            'profile = "minimal"\n',
            encoding="utf-8",
        )
        workflow = root / ".github" / "workflows" / "ci.yml"
        workflow.write_text(
            "steps:\n"
            "  - uses: dtolnay/rust-toolchain@stable\n"
            "    with:\n"
            "      toolchain: '1.88.0'\n"
            "  - run: cargo test --locked --lib\n",
            encoding="utf-8",
        )
        dockerfile = root / "docker" / "Dockerfile"
        dockerfile.write_text("FROM rust:1.88.0-slim\n", encoding="utf-8")
        assert check(root) == [], check(root)

        good_command = (
            sys.executable,
            "-c",
            "print('rustc 1.88.0 (fixture 2025-06-23)')",
        )
        reported, failures = _runtime_rustc_errors(root, good_command)
        assert reported and reported.startswith("rustc 1.88.0"), reported
        assert failures == [], failures

        bad_command = (
            sys.executable,
            "-c",
            "print('rustc 1.89.0 (fixture 2025-08-07)')",
        )
        _, failures = _runtime_rustc_errors(root, bad_command)
        assert any("runtime rustc version '1.89.0'" in f for f in failures), failures

        workflow.write_text(
            "steps:\n"
            "  - uses: dtolnay/rust-toolchain@stable\n"
            "    with:\n"
            "      toolchain: '1.89.0'\n"
            "  - run: cargo test --lib\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("1.89.0" in failure for failure in failures), failures
        assert any("lacks --locked" in failure for failure in failures), failures

        # A matrix-driven toolchain is accepted when every leg is an exact
        # version AND the pin is among them (#2419).
        matrix_header = (
            "jobs:\n"
            "  roots:\n"
            "    strategy:\n"
            "      matrix:\n"
            "        include:\n"
            "          - leg: pinned\n"
            "            toolchain: '1.88.0'\n"
            "          - leg: previous\n"
            "            toolchain: '{other}'\n"
            "    steps:\n"
            "      - uses: dtolnay/rust-toolchain@master\n"
            "        with:\n"
            "          toolchain: ${{{{ matrix.toolchain }}}}\n"
            "      - run: cargo test --locked --lib\n"
        )
        workflow.write_text(matrix_header.format(other="1.87.0"), encoding="utf-8")
        assert check(root) == [], check(root)

        # A floating leg defeats reproducibility even inside a matrix.
        workflow.write_text(matrix_header.format(other="stable"), encoding="utf-8")
        failures = check(root)
        assert any("not an exact version" in f for f in failures), failures

        # A matrix that never builds the pinned toolchain is drift, not a test.
        workflow.write_text(
            matrix_header.format(other="1.86.0").replace("'1.88.0'", "'1.85.0'"),
            encoding="utf-8",
        )
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), failures

        # `exclude:` REMOVES legs. An axis of ['1.87.0'] with the pin listed only
        # under `exclude:` runs 1.87.0 and nothing else — accepting it would be
        # the same false green in a new shape (review of 1b1f2a2304).
        excluded_pin = (
            "jobs:\n"
            "  roots:\n"
            "    strategy:\n"
            "      matrix:\n"
            "        toolchain:\n"
            "          - '1.87.0'\n"
            "        exclude:\n"
            "          - toolchain: '1.88.0'\n"
            "    steps:\n"
            "      - uses: dtolnay/rust-toolchain@master\n"
            "        with:\n"
            "          toolchain: ${{ matrix.toolchain }}\n"
            "      - run: cargo test --locked --lib\n"
        )
        workflow.write_text(excluded_pin, encoding="utf-8")
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), failures

        # Block-list axis form must be READ, not silently ignored — otherwise the
        # only values seen come from `exclude:` and the check inverts.
        workflow.write_text(
            excluded_pin.replace(
                "        exclude:\n          - toolchain: '1.88.0'\n", ""
            ).replace("          - '1.87.0'\n", "          - '1.87.0'\n          - '1.88.0'\n"),
            encoding="utf-8",
        )
        assert check(root) == [], check(root)

        # A key that appears ONLY under `exclude:` is unresolvable → fail closed.
        workflow.write_text(
            excluded_pin.replace("        toolchain:\n          - '1.87.0'\n", ""),
            encoding="utf-8",
        )
        failures = check(root)
        assert any("only under `exclude:`" in f for f in failures), failures

        # Dan's regression fixture (review of e147c38b1), all three shapes at
        # once: job keys carrying trailing COMMENTS, a job whose `steps:` precede
        # its `strategy:`, and TWO matrices where the unrelated one is pinned and
        # the one the action actually uses is not. Previously the comment hid the
        # job keys, ownership went unresolved, the parser FAILED OPEN to the whole
        # file, and the unrelated job donated 1.88.0 to a job running 1.87.0.
        workflow.write_text(
            "jobs:\n"
            "  pinned-unrelated:   # trailing comment on the job key\n"
            "    strategy:\n"
            "      matrix:   # comment here too\n"
            "        include:\n"
            "          - toolchain: '1.88.0'\n"
            "    steps:\n"
            "      - run: echo does not use the rust-toolchain action\n"
            "  offpin:   # steps deliberately precede strategy\n"
            "    steps:\n"
            "      - uses: dtolnay/rust-toolchain@master\n"
            "        with:\n"
            "          toolchain: ${{ matrix.toolchain }}\n"
            "      - run: cargo test --locked --lib\n"
            "    strategy:\n"
            "      matrix:\n"
            "        include:\n"
            "          - toolchain: '1.87.0'\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), failures
        assert not any("cannot determine which job" in f for f in failures), (
            "job ownership must RESOLVE here — comments and key order are legal "
            f"YAML, not unresolvable shapes: {failures}"
        )

        # Scoping: only `toolchain:` keys INSIDE the matrix block count. A
        # second job's unrelated literal must not leak in and be judged as a
        # matrix leg, and the `${{ matrix.toolchain }}` line under `with:` must
        # not collect itself.
        workflow.write_text(
            matrix_header.format(other="1.87.0")
            + "  unrelated:\n"
            "    steps:\n"
            "      - uses: dtolnay/rust-toolchain@stable\n"
            "        with:\n"
            "          toolchain: '1.88.0'\n"
            "      - run: cargo build --locked\n",
            encoding="utf-8",
        )
        assert check(root) == [], check(root)

        # Job scoping: an UNRELATED job's matrix must not donate the pinned
        # toolchain to a job whose own matrix never builds it. Without resolving
        # the enclosing job the candidate lists merge and this passes vacuously
        # — an unsafe shape the pre-#2419 checker rejected outright (P1, review
        # of 9da1accf8).
        workflow.write_text(
            "jobs:\n"
            "  offpin:\n"
            "    strategy:\n"
            "      matrix:\n"
            "        include:\n"
            "          - leg: only-off-pin\n"
            "            toolchain: '1.87.0'\n"
            "    steps:\n"
            "      - uses: dtolnay/rust-toolchain@master\n"
            "        with:\n"
            "          toolchain: ${{ matrix.toolchain }}\n"
            "      - run: cargo test --locked --lib\n"
            "  unrelated:\n"
            "    strategy:\n"
            "      matrix:\n"
            "        include:\n"
            "          - leg: nothing-to-do-with-rust\n"
            "            toolchain: '1.88.0'\n"
            "    steps:\n"
            "      - run: echo hello\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), failures

        # -- job-key shape probes (#2419) --------------------------------------
        #
        # Every one of these is an off-pin job whose OWN matrix builds only
        # 1.87.0, followed by a decoy job pinned to 1.88.0. If job boundaries
        # are not recognised, the off-pin action is attributed to the decoy and
        # the gate passes vacuously — the exact false green that survived three
        # review rounds.
        #
        # The first two are the reviewer's counterexamples. The rest are shapes
        # nobody tried, included so the self-test encodes the INVARIANT — every
        # valid job key is a boundary — rather than the two known failures. All
        # of them are legal YAML that GitHub accepts as job IDs.
        def _two_job_workflow(offpin_key: str, decoy_key: str = "pinned-decoy") -> str:
            return (
                "jobs:\n"
                f"  {offpin_key}\n"
                "    strategy:\n"
                "      matrix:\n"
                "        toolchain: ['1.87.0']\n"
                "    steps:\n"
                "      - uses: dtolnay/rust-toolchain@master\n"
                "        with:\n"
                "          toolchain: ${{ matrix.toolchain }}\n"
                "      - run: cargo test --locked --lib\n"
                f"  {decoy_key}:\n"
                "    strategy:\n"
                "      matrix:\n"
                "        toolchain: ['1.88.0']\n"
                "    steps:\n"
                "      - run: echo decoy\n"
            )

        shapes = {
            # Reviewer round 3: a double-quoted job key. The old JOB_KEY regex
            # matched only bare `[A-Za-z0-9_-]+:`, so this was not a boundary.
            "double-quoted key": '"offpin":',
            # Reviewer round 2 shape: steps precede strategy, so a scanner that
            # anchors on `strategy:` attributes the action to the wrong job.
            "single-quoted key": "'offpin':",
            # Not tried by the reviewer: a trailing comment on the job key.
            "key with trailing comment": "offpin:  # deliberately off-pin",
            # Not tried by the reviewer: a job key that is not a bare word at
            # all. This is parser hardening, NOT a supported Actions shape —
            # GitHub requires a job ID to start with a letter or `_` and to
            # contain only alphanumerics, `-`, or `_`, so a real workflow may
            # not use this. It is legal YAML, and the traversal should not
            # care either way.
            "dotted key": "off.pin.leg:",
        }
        for label, key in shapes.items():
            workflow.write_text(_two_job_workflow(key), encoding="utf-8")
            failures = check(root)
            assert any("never exercises the pinned" in f for f in failures), (
                f"{label}: off-pin job must be caught, got {failures}"
            )
            assert not any("cannot determine which job" in f for f in failures), (
                f"{label}: ownership must resolve structurally, got {failures}"
            )

        # A flow-mapping job body — the whole job on one line. Nothing about a
        # line-oriented scanner can see the step boundaries here at all.
        workflow.write_text(
            "jobs:\n"
            "  offpin: {strategy: {matrix: {toolchain: ['1.87.0']}}, "
            "steps: [{uses: 'dtolnay/rust-toolchain@master', "
            "with: {toolchain: '${{ matrix.toolchain }}'}}]}\n"
            "  pinned-decoy:\n"
            "    strategy:\n"
            "      matrix:\n"
            "        toolchain: ['1.88.0']\n"
            "    steps:\n"
            "      - run: echo decoy\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), (
            f"flow-mapping job must be caught, got {failures}"
        )

        # A steps list shared between jobs by YAML ANCHOR/ALIAS. GitHub
        # officially supports anchors and aliases for shared workflow
        # configuration, so both jobs really do run this action — `offpin`
        # under 1.87.0 and `pinned-decoy` under 1.88.0.
        #
        # The alias resolves to the anchored node and keeps the ANCHOR's source
        # mark, so both occurrences report the same `start_mark.line`. Keyed by
        # line, the second job overwrote the first and donated its pin, and the
        # off-pin job vanished from the owner map entirely (review of
        # f772e4301). Occurrences are per (job, action), so both survive.
        workflow.write_text(
            "jobs:\n"
            "  offpin:\n"
            "    runs-on: ubuntu-latest\n"
            "    strategy:\n"
            "      matrix:\n"
            "        toolchain: ['1.87.0']\n"
            "    steps: &shared-rust-steps\n"
            "      - uses: dtolnay/rust-toolchain@master\n"
            "        with:\n"
            "          toolchain: ${{ matrix.toolchain }}\n"
            "      - run: cargo test --locked --lib\n"
            "\n"
            "  pinned-decoy:\n"
            "    runs-on: ubuntu-latest\n"
            "    strategy:\n"
            "      matrix:\n"
            "        toolchain: ['1.88.0']\n"
            "    steps: *shared-rust-steps\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("never exercises the pinned" in f for f in failures), (
            f"aliased steps: the off-pin job must be caught, not overwritten by "
            f"the pinned job reusing the same anchored node, got {failures}"
        )
        assert any("'offpin'" in f for f in failures), (
            f"the off-pin JOB must be named — line identity is ambiguous under "
            f"an alias, so the report has to say which job, got {failures}"
        )
        assert not any("cannot determine which job" in f for f in failures), (
            f"ownership must resolve for both aliased occurrences, got {failures}"
        )

        # A document PyYAML cannot parse, carrying a QUOTED action. Structural
        # classification fails, and the line regex cannot see a quoted `uses:`
        # either — so before this fixture the file produced NO finding at all
        # and an unparseable workflow read as clean (review of f772e4301).
        # Inability to understand a file must be reported, never passed over.
        workflow.write_text(
            "jobs:\n"
            "  offpin:\n"
            "    steps:\n"
            "      - uses: 'dtolnay/rust-toolchain@master'\n"
            "        with: {toolchain: '${{ matrix.toolchain }}'\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("could not be classified" in f for f in failures), (
            f"an unparseable workflow must be an ERROR, not silence: {failures}"
        )

        # Ownership that genuinely cannot be resolved must still FAIL CLOSED:
        # a matrix-driven action with no `jobs:` mapping to own it.
        workflow.write_text(
            "steps:\n"
            "  - uses: dtolnay/rust-toolchain@master\n"
            "    with:\n"
            "      toolchain: ${{ matrix.toolchain }}\n",
            encoding="utf-8",
        )
        failures = check(root)
        assert any("cannot determine which job" in f for f in failures), (
            f"unresolved ownership must fail closed, got {failures}"
        )

    print("check_toolchain_pin.py self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--runtime",
        action="store_true",
        help="also require the rustc process that actually runs to match the pin",
    )
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0

    root = Path(__file__).resolve().parents[1]
    errors = check(root)
    reported = None
    if args.runtime:
        reported, runtime_errors = _runtime_rustc_errors(root)
        errors.extend(runtime_errors)
        if reported is not None:
            print(f"runtime rustc reports: {reported}")
    if errors:
        print("Rust toolchain pin check failed:")
        for error in errors:
            print(f"- {error}")
        return 1
    pin, _ = _pin(root)
    suffix = "; runtime compiler matches" if args.runtime else ""
    print(f"Rust toolchain pin check passed ({pin}{suffix})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
