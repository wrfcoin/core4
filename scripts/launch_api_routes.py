#!/usr/bin/env python3
"""Extract the launch_api method/path ledger from its Actix registration."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROUTE_RE = re.compile(
    r'\.route\(\s*"(?P<path>[^"]+)"\s*,\s*web::(?P<method>get|post|put|delete|patch)\(\)',
    re.DOTALL,
)


CFG_RE = re.compile(r"#\[cfg\((?P<expr>[^\n]*)\)\]\s*\n(?P<stmt>\s*let\b)")
FEATURE_RE = re.compile(r'feature\s*=\s*"([^"]+)"')
_MASK_RE = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*')


def _mask(source: str) -> str:
    """Blank out string literals and line comments, keeping offsets."""
    return _MASK_RE.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), source)


def _cfg_spans(source: str) -> list[tuple[int, int, str]]:
    """(start, end, feature) for each `#[cfg(feature = ..)] let ...;` statement."""
    masked = _mask(source)
    spans = []
    for match in CFG_RE.finditer(masked):
        features = FEATURE_RE.findall(source[match.start() : match.end()])
        if not features:
            continue
        depth = 0
        index = match.start("stmt")
        while index < len(masked):
            char = masked[index]
            if char in "({[":
                depth += 1
            elif char in ")}]":
                depth -= 1
            elif char == ";" and depth == 0:
                break
            index += 1
        spans.append((match.end(), index, "+".join(features)))
    return spans


def _feature_for(route_path: str) -> str | None:
    if route_path.startswith("/debug/"):
        return "dev-endpoints"
    if route_path.startswith("/zk/"):
        return "zk"
    if route_path.startswith("/execution/"):
        return "execution"
    if route_path in {"/network/stats"} or route_path.startswith(
        "/network/peer"
    ) or route_path.startswith("/network/dht/"):
        return "p2p"
    return None


def parse_routes(path: str | Path) -> list[dict[str, str | None]]:
    source = Path(path).read_text(encoding="utf-8")
    governance_start = source.find('web::scope("/governance")')
    governance_end = source.find('.route("/enhanced_security/status"', governance_start)
    spans = _cfg_spans(source)
    routes: list[dict[str, str | None]] = []
    for match in ROUTE_RE.finditer(source):
        route_path = match.group("path")
        if governance_start <= match.start() < governance_end:
            route_path = "/governance" + route_path
        routes.append(
            {
                "method": match.group("method").upper(),
                "path": route_path,
                "feature": next(
                    (name for lo, hi, name in spans if lo <= match.start() < hi),
                    _feature_for(route_path),
                ),
            }
        )
    return sorted(routes, key=lambda item: (str(item["path"]), str(item["method"])))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "path",
        nargs="?",
        default="native-chain/src/bin/launch_api/routes/mod.rs",
    )
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    routes = parse_routes(args.path)
    if args.json:
        print(json.dumps(routes, indent=2))
    else:
        for route in routes:
            suffix = f" [{route['feature']}]" if route["feature"] else ""
            print(f"{route['method']} {route['path']}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
