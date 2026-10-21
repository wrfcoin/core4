#!/usr/bin/env python3
"""Pre-flight gate: no operator interface is exposed off-loopback (#2120).

The deployment half of the interface-separation rule. `P2pConfig::validate` enforces what
the p2p layer can see (node role, sentry allowlist, the addresses it is handed); this
checks what actually gets published by Compose, which Rust never sees.

    python3 scripts/check_interface_binding.py --compose-ps "$(docker compose ... ps --format ...)"
    python3 scripts/check_interface_binding.py --self-test

Usable from the testnet go/no-go gate. Reads published bindings on stdin or via
--compose-ps so it needs no Docker access of its own, and so the same input can be replayed
in review.

The rule: a published binding on 0.0.0.0 (or ::) is a finding UNLESS its container port is
in ALLOWED_PUBLIC_PORTS. P2P is the only interface that is legitimately reachable off-box;
everything else -- node API, backend, faucet, explorer, Prometheus, Grafana, Alertmanager,
exporters -- is an operator surface and belongs on loopback or the tailnet.
"""

from __future__ import annotations

import argparse
import re
import sys

# Container ports that may legitimately be published off-loopback.
#
# 9000 is the reference node's P2P listener; 9001 is the seed container's. Both were
# measured on the live box -- the guide previously said "only 9000", which would have made
# an operator either flag 9001 as a hole or close it and remove the seed from the network.
ALLOWED_PUBLIC_PORTS: set[int] = {9000, 9001}

_BINDING = re.compile(r"(?P<host>0\.0\.0\.0|::|\[::\]):(?P<hostport>\d+)->(?P<cport>\d+)/(?P<proto>tcp|udp)")


def findings(text: str) -> list[tuple[str, int, int]]:
    """(raw binding, host port, container port) for every off-loopback publish."""
    out: list[tuple[str, int, int]] = []
    for m in _BINDING.finditer(text):
        cport = int(m.group("cport"))
        if cport in ALLOWED_PUBLIC_PORTS:
            continue
        out.append((m.group(0), int(m.group("hostport")), cport))
    return out


def self_test() -> None:
    # A loopback publish is never a finding.
    assert not findings("127.0.0.1:8556->8556/tcp"), "loopback must pass"
    # The sanctioned P2P ports are not findings even on 0.0.0.0.
    assert not findings("0.0.0.0:9000->9000/tcp"), "9000 is the P2P listener"
    assert not findings("0.0.0.0:9001->9001/tcp"), "9001 is the seed P2P listener"
    # Anything else on 0.0.0.0 is.
    assert findings("0.0.0.0:9090->9090/tcp"), "Prometheus off-loopback must be a finding"
    assert findings("[::]:3100->3100/tcp"), "IPv6 any-address must be caught too"
    # RED control on the matcher itself: it must not fire on text that merely CONTAINS a
    # port number. A regex that matched loosely here would report the whole file.
    assert not findings("image: prometheus:9090"), "must not match arbitrary text"
    assert not findings("0.0.0.0:9090"), "must require the ->container/proto form"
    # And the allow-list must discriminate rather than accept everything.
    assert findings("0.0.0.0:9002->9002/tcp"), "an unlisted port must not be allowed"
    print("check_interface_binding.py self-test passed")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--compose-ps", default=None,
                    help="output of `docker compose ps` (otherwise read stdin)")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        self_test()
        return 0

    text = args.compose_ps if args.compose_ps is not None else sys.stdin.read()
    if not text.strip():
        print("FAIL: no input. Pipe `docker compose ps` in, or pass --compose-ps. "
              "Empty input is a refusal, not a pass: a gate that reports success on no "
              "data is the vacuous-green failure this exists to prevent.", file=sys.stderr)
        return 1

    bad = findings(text)
    print(f"[interface-binding] allowed public ports: {sorted(ALLOWED_PUBLIC_PORTS)}")
    if bad:
        print(f"\nFAIL: {len(bad)} operator interface(s) published off-loopback:", file=sys.stderr)
        for raw, hostport, cport in bad:
            print(f"  {raw}  (host {hostport} -> container {cport})", file=sys.stderr)
        print("\nOnly P2P may be reachable off-box. Bind these to 127.0.0.1 and reach them "
              "over an SSH tunnel or the tailnet (#2120).", file=sys.stderr)
        return 1

    print("check_interface_binding.py: every operator interface is loopback-bound")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
