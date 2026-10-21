#!/usr/bin/env python3
"""Statically enforce that every exposed workflow retains an executable guard."""

from __future__ import annotations

import json
import sys
from pathlib import Path

try:
    import yaml
except ImportError as error:  # pragma: no cover - exercised in CI installation
    raise SystemExit(f"FAIL-CLOSED: PyYAML is required to inspect workflow structure: {error}")


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "scripts/terminal_skip_guard_manifest.json"


class WorkflowLoader(yaml.SafeLoader):
    """Keep GitHub Actions' `on` key as a string under YAML 1.1 parsers."""


for character, resolvers in list(WorkflowLoader.yaml_implicit_resolvers.items()):
    WorkflowLoader.yaml_implicit_resolvers[character] = [
        (tag, regex) for tag, regex in resolvers if tag != "tag:yaml.org,2002:bool"
    ]


EXECUTED_EVENTS = {"pull_request", "push", "release"}


def triggers_executed_event(document: dict) -> bool:
    triggers = document.get("on", {})
    if isinstance(triggers, str):
        triggers = {triggers: {}}
    if isinstance(triggers, list):
        triggers = {trigger: {} for trigger in triggers if isinstance(trigger, str)}
    return isinstance(triggers, dict) and bool(set(triggers) & EXECUTED_EVENTS)


def has_needs_edge(document: dict) -> bool:
    return any(
        isinstance(job, dict) and bool(job.get("needs"))
        for job in document.get("jobs", {}).values()
    )


def rendered_env(events: dict[str, dict[str, list[str]]], classification: str) -> str:
    """Render the workflow expression from the manifest's event partition."""
    default = " ".join(events["default"][classification])
    variants = [(event, values) for event, values in events.items() if event != "default"]
    if not variants:
        return default
    if len(variants) != 1:
        raise ValueError("only one event-specific classification is supported")
    event, values = variants[0]
    return "${{ github.event_name == '%s' && '%s' || '%s' }}" % (
        event,
        " ".join(values[classification]),
        default,
    )


def main() -> int:
    manifest = json.loads(MANIFEST.read_text())
    errors: list[str] = []
    guard_contexts: dict[str, str] = {}
    candidate_workflows: set[str] = set()
    workflow_paths = list((ROOT / ".github/workflows").glob("*.yml"))
    workflow_paths.extend((ROOT / ".github/workflows").glob("*.yaml"))
    for path in workflow_paths:
        try:
            document = yaml.load(path.read_text(), Loader=WorkflowLoader) or {}
        except (OSError, yaml.YAMLError) as error:
            errors.append(f"{path.name}: cannot inspect workflow trigger/needs shape: {error}")
            continue
        if not isinstance(document, dict):
            errors.append(f"{path.name}: workflow document must be a mapping")
        elif triggers_executed_event(document) and has_needs_edge(document):
            candidate_workflows.add(path.name)
    uncovered = candidate_workflows - set(manifest)
    if uncovered:
        errors.append(
            "workflow(s) have needs edges on pull_request/push/release but no manifest guard: "
            + ", ".join(sorted(uncovered))
        )
    for filename, expected in manifest.items():
        path = ROOT / ".github/workflows" / filename
        try:
            document = yaml.load(path.read_text(), Loader=WorkflowLoader)
            job = document["jobs"][expected["job"]]
        except (OSError, KeyError, TypeError, yaml.YAMLError) as error:
            errors.append(f"{filename}: cannot load terminal guard {expected['job']!r}: {error}")
            continue
        expected_context = expected.get("context")
        if expected_context is not None:
            actual_context = job.get("name")
            if actual_context != expected_context:
                errors.append(
                    f"{filename}: check context {actual_context!r}, expected {expected_context!r}"
                )
            if "${{" in str(actual_context):
                errors.append(f"{filename}: check context must not be templated")
            prior_filename = guard_contexts.get(str(actual_context))
            if prior_filename is not None:
                errors.append(
                    f"{filename}: check context {actual_context!r} collides with {prior_filename}"
                )
            else:
                guard_contexts[str(actual_context)] = filename
        for additional_expected in expected.get("additional_guards", []):
            additional_job = additional_expected["job"]
            additional = document.get("jobs", {}).get(additional_job)
            if not isinstance(additional, dict):
                errors.append(f"{filename}: missing additional terminal guard {additional_job!r}")
                continue
            if "always()" not in str(additional.get("if", "")):
                errors.append(f"{filename}: {additional_job} must use always()")
            if additional_expected["if_contains"] not in str(additional.get("if", "")):
                errors.append(f"{filename}: {additional_job} has an unpinned event predicate")
            actual_additional_needs = additional.get("needs", [])
            if isinstance(actual_additional_needs, str):
                actual_additional_needs = [actual_additional_needs]
            if set(actual_additional_needs) != set(additional_expected["needs"]):
                errors.append(f"{filename}: {additional_job} needs differ from manifest")
            if not any("python3 scripts/assert_gate_results.py" in str(step.get("run", "")) for step in additional.get("steps", []) if isinstance(step, dict)):
                errors.append(f"{filename}: {additional_job} does not execute assert_gate_results.py")
            for step in additional.get("steps", []):
                if isinstance(step, dict) and "python3 scripts/assert_gate_results.py" in str(step.get("run", "")):
                    env = step.get("env", {})
                    if str(env.get("GATE_REQUIRED", "")) != " ".join(additional_expected["required"]):
                        errors.append(f"{filename}: {additional_job} required classification differs from manifest")
                    if str(env.get("GATE_CONDITIONAL", "")) != " ".join(additional_expected["conditional"]):
                        errors.append(f"{filename}: {additional_job} conditional classification differs from manifest")
        condition = str(job.get("if", ""))
        if "always()" not in condition:
            errors.append(f"{filename}: {expected['job']} must use always()")
        actual_needs = job.get("needs", [])
        if isinstance(actual_needs, str):
            actual_needs = [actual_needs]
        if set(actual_needs) != set(expected["needs"]):
            errors.append(f"{filename}: {expected['job']} needs {actual_needs!r}, expected {expected['needs']!r}")
        events = expected.get("events")
        if not isinstance(events, dict) or "default" not in events:
            errors.append(f"{filename}: manifest lacks a default event classification")
        else:
            for event, classification in events.items():
                if set(classification) != {"required", "conditional"}:
                    errors.append(f"{filename}: {event} must classify required and conditional jobs")
                    continue
                if set(classification["required"]) & set(classification["conditional"]):
                    errors.append(f"{filename}: {event} overlaps required and conditional jobs")
                if set(classification["required"]) | set(classification["conditional"]) != set(expected["needs"]):
                    errors.append(f"{filename}: {event} does not classify every needs dependency")
        steps = job.get("steps", [])
        gate_steps = [step for step in steps if isinstance(step, dict) and "python3 scripts/assert_gate_results.py" in str(step.get("run", ""))]
        if not gate_steps:
            errors.append(f"{filename}: {expected['job']} does not execute assert_gate_results.py")
        elif isinstance(events, dict) and "default" in events:
            environment = gate_steps[0].get("env", {})
            for classification, env_name in (("required", "GATE_REQUIRED"), ("conditional", "GATE_CONDITIONAL")):
                try:
                    actual = str(environment.get(env_name, ""))
                    expected_value = rendered_env(events, classification)
                except (KeyError, TypeError, ValueError) as error:
                    errors.append(f"{filename}: cannot render {env_name} from manifest: {error}")
                    continue
                if actual != expected_value:
                    errors.append(f"{filename}: {env_name}={actual!r}, expected manifest-pinned {expected_value!r}")
    if errors:
        for error in errors:
            print(f"FAIL-CLOSED: {error}")
        return 1
    print("all exposed workflows retain an always-running terminal skip guard")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
