#!/usr/bin/env python3
"""Fail when the launch_api registration sites and RPC_SURFACE.md drift."""

from __future__ import annotations

import argparse
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path


ROUTE_PATTERN = re.compile(
    r'\.route\(\s*"([^"]+)"\s*,\s*'
    r"web::(get|post|put|delete|patch)\(\)\.to\(([^)]+)\)\s*,?\s*\)",
    re.DOTALL,
)
WS_ACTION_PATTERN = re.compile(
    r'#\[serde\(rename\s*=\s*"([^"]+)"\)\]\s*(\w+)', re.DOTALL
)
JSONRPC_PATTERN = re.compile(
    r'\.register_(?:async_)?method\(\s*"([^"]+)"', re.DOTALL
)
DOC_ROW_PATTERN = re.compile(
    r"^\| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \|"
)
VALID_CLASSIFICATIONS = {
    "public",
    "authenticated",
    "admin",
    "disabled-in-release",
}


@dataclass(frozen=True, order=True)
class SurfaceMethod:
    method: str
    transport: str
    handler: str


def _handler_path(root: Path, handler: str) -> str:
    module, _name = handler.rsplit("::", 1)
    if module == "crate::websocket":
        path = Path("native-chain/src/bin/launch_api/websocket.rs")
    elif module == "zk_handlers":
        path = Path("native-chain/src/bin/launch_api/routes/zk.rs")
    elif module == "privacy_handlers":
        path = Path("native-chain/src/bin/launch_api/routes/privacy.rs")
    else:
        path = Path("native-chain/src/bin/launch_api/routes") / f"{module}.rs"
    if not (root / path).is_file():
        raise ValueError(f"handler source is missing: {path}")
    return f"{path}:{handler}"


def extract_registered(root: Path) -> set[SurfaceMethod]:
    route_file = root / "native-chain/src/bin/launch_api/routes/mod.rs"
    websocket_file = root / "native-chain/src/bin/launch_api/websocket.rs"
    route_source = route_file.read_text(encoding="utf-8")
    websocket_source = websocket_file.read_text(encoding="utf-8")
    methods: set[SurfaceMethod] = set()

    for match in ROUTE_PATTERN.finditer(route_source):
        route, verb, handler = match.groups()
        handler = " ".join(handler.split())
        if handler.startswith("governance::"):
            route = f"/governance{route}"
        method = f"{verb.upper()} {route}"
        transport = "ws" if route == "/ws" else "http"
        item = SurfaceMethod(method, transport, _handler_path(root, handler))
        if item in methods:
            raise ValueError(f"duplicate registration: {method}")
        methods.add(item)

    client_message = re.search(
        r"enum\s+ClientMessage\s*\{(?P<body>.*?)\n\}",
        websocket_source,
        re.DOTALL,
    )
    if client_message is None:
        raise ValueError("websocket ClientMessage enum is missing")
    for action, variant in WS_ACTION_PATTERN.findall(client_message.group("body")):
        methods.add(
            SurfaceMethod(
                f"{action.upper()} /ws",
                "ws",
                f"native-chain/src/bin/launch_api/websocket.rs:ClientMessage::{variant}",
            )
        )

    launch_root = root / "native-chain/src/bin/launch_api"
    for path in sorted(launch_root.rglob("*.rs")):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(root)
        for rpc_name in JSONRPC_PATTERN.findall(source):
            methods.add(
                SurfaceMethod(
                    f"RPC {rpc_name}",
                    "jsonrpc",
                    f"{relative}:register_method({rpc_name})",
                )
            )
    return methods


def extract_documented(path: Path) -> tuple[set[SurfaceMethod], list[str]]:
    methods: set[SurfaceMethod] = set()
    errors: list[str] = []
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        match = DOC_ROW_PATTERN.match(line)
        if match is None:
            continue
        method, transport, handler, classification = match.groups()
        handler = re.sub(r" \(feature: [^)]+\)$", "", handler)
        item = SurfaceMethod(method, transport, handler)
        if item in methods:
            errors.append(f"line {line_number}: duplicate documented method {method}")
        methods.add(item)
        if classification not in VALID_CLASSIFICATIONS:
            errors.append(
                f"line {line_number}: invalid classification {classification!r}"
            )
    if not methods:
        errors.append("no inventory rows found")
    return methods, errors


def check(root: Path) -> list[str]:
    document = root / "docs/security/RPC_SURFACE.md"
    if not document.is_file():
        return ["docs/security/RPC_SURFACE.md is missing"]
    try:
        registered = extract_registered(root)
    except (OSError, ValueError) as error:
        return [f"registration extraction failed: {error}"]
    documented, errors = extract_documented(document)

    for item in sorted(registered - documented):
        errors.append(
            f"registered but undocumented: {item.method} [{item.transport}] -> {item.handler}"
        )
    for item in sorted(documented - registered):
        errors.append(
            f"documented but unregistered: {item.method} [{item.transport}] -> {item.handler}"
        )
    return errors


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        routes = root / "native-chain/src/bin/launch_api/routes"
        routes.mkdir(parents=True)
        docs = root / "docs/security"
        docs.mkdir(parents=True)
        (routes / "chain.rs").write_text(
            "pub async fn health() {}\n", encoding="utf-8"
        )
        (routes / "mod.rs").write_text(
            'cfg.route("/health", web::get().to(chain::health));\n',
            encoding="utf-8",
        )
        websocket = root / "native-chain/src/bin/launch_api/websocket.rs"
        websocket.write_text(
            '#[derive(Deserialize)]\nenum ClientMessage {\n'
            '    #[serde(rename = "subscribe")]\n'
            '    Subscribe { channel: String },\n}\n',
            encoding="utf-8",
        )
        document = docs / "RPC_SURFACE.md"
        document.write_text(
            "| Method | Transport | Handler path:symbol | Classification | Mutates state? | Unbounded work risk | Rate-limit class |\n"
            "| --- | --- | --- | --- | --- | --- | --- |\n"
            "| `GET /health` | `http` | `native-chain/src/bin/launch_api/routes/chain.rs:chain::health` | `public` | no | no | global-read |\n"
            "| `SUBSCRIBE /ws` | `ws` | `native-chain/src/bin/launch_api/websocket.rs:ClientMessage::Subscribe` | `authenticated` | no | no | none |\n",
            encoding="utf-8",
        )
        assert check(root) == [], check(root)
        document.write_text(
            document.read_text(encoding="utf-8").replace("GET /health", "GET /stale"),
            encoding="utf-8",
        )
        failures = check(root)
        assert any("registered but undocumented" in failure for failure in failures)
        assert any("documented but unregistered" in failure for failure in failures)
    print("check_rpc_surface.py self-test passed")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0

    root = Path(__file__).resolve().parents[1]
    errors = check(root)
    if errors:
        print("RPC surface check failed:")
        for error in errors:
            print(f"- {error}")
        return 1
    print("RPC surface check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
