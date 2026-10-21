#!/usr/bin/env python3
"""Check the first-tranche OpenAPI paths against launch_api registration."""

from __future__ import annotations

import argparse
from pathlib import Path

import yaml

from launch_api_routes import parse_routes

CORE_ROUTES = {
    ("GET", "/health"),
    ("GET", "/readyz"),
    ("GET", "/version"),
    ("GET", "/network/info"),
    ("GET", "/chain/blocks"),
    ("GET", "/chain/block/{height}"),
    ("GET", "/addresses/{address}"),
    ("POST", "/transactions/submit"),
    ("POST", "/transactions/station/registrations"),
    ("GET", "/transactions/station/registrations/{station_id}"),
    ("POST", "/transactions/contribution/commit"),
    ("POST", "/transactions/contribution/reveal"),
    ("GET", "/transactions/commit/{commit_id}/status"),
    ("GET", "/transactions/contributions/status"),
    ("GET", "/transactions/oracle/status"),
    ("GET", "/transactions/stake/{address}"),
}
# Every operation key OpenAPI 3.0 allows on a path item. Registered routes only ever use
# get/post/put/delete/patch, so head/options/trace in the spec are reported as phantoms.
HTTP_METHODS = {"get", "put", "post", "delete", "options", "head", "patch", "trace"}


def load_spec_operations(path: str | Path) -> dict[tuple[str, str], dict]:
    document = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
    if document.get("openapi") != "3.0.3" or not isinstance(document.get("paths"), dict):
        raise ValueError("expected an OpenAPI 3.0.3 document with paths")
    return {
        (method.upper(), route_path): operation or {}
        for route_path, item in document["paths"].items()
        for method, operation in item.items()
        if method.lower() in HTTP_METHODS
    }


def load_spec_routes(path: str | Path) -> set[tuple[str, str]]:
    return set(load_spec_operations(path))


def check(spec_path: str | Path, routes_path: str | Path) -> list[str]:
    parsed = parse_routes(routes_path)
    registered = {(str(item["method"]), str(item["path"])) for item in parsed}
    features = {
        (str(item["method"]), str(item["path"])): item["feature"] for item in parsed
    }
    operations = load_spec_operations(spec_path)
    specified = set(operations)
    errors = [
        f"phantom spec operation: {method} {path}"
        for method, path in sorted(specified - registered)
    ]
    # A route compiled only under a cargo feature must say so in the spec, so the spec does not
    # present it as served by every build (the testnet build has privacy off).
    for key, operation in sorted(operations.items()):
        feature = features.get(key)
        if feature and operation.get("x-feature") != feature:
            errors.append(
                f"feature-gated operation not marked x-feature: {feature}: {key[0]} {key[1]}"
            )
    errors.extend(
        f"missing core operation: {method} {path}"
        for method, path in sorted(CORE_ROUTES - specified)
    )
    if not errors:
        uncovered = registered - specified
        print(
            f"launch_api OpenAPI check OK: {len(specified)} checked operations; "
            f"{len(uncovered)} registered operations remain outside the first tranche (INFO)"
        )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", default="docs/api/launch-api.openapi.yaml")
    parser.add_argument(
        "--routes", default="native-chain/src/bin/launch_api/routes/mod.rs"
    )
    args = parser.parse_args()
    errors = check(args.spec, args.routes)
    for error in errors:
        print(f"ERROR: {error}")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
