#!/usr/bin/env python3
"""Compare the reviewed record with bounded read-only node projections (stdlib)."""

import argparse
import contextlib
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import signal
import stat
import sys
import urllib.error
import urllib.parse
import urllib.request

MAX_BYTES = 65536
HTTP_SECONDS = 10
U64_MAX = (1 << 64) - 1
DEFAULT_RECORD = Path(__file__).with_name("777-testnet.json")


class CheckError(ValueError):
    pass


def need(condition, label):
    if not condition:
        raise CheckError(label)


def uint(value):
    need(type(value) is int and 0 <= value <= U64_MAX, "invalid unsigned integer")
    return value


def digest(value):
    need(isinstance(value, str), "invalid chain digest")
    value = value.removeprefix("0x").lower()
    need(re.fullmatch(r"[0-9a-f]{64}", value), "invalid chain digest")
    return value


def pairs(items):
    result = {}
    for key, value in items:
        need(key not in result, "duplicate JSON member")
        result[key] = value
    return result


def reject_constant(_):
    raise CheckError("nonfinite JSON")


def decode(data):
    need(len(data) <= MAX_BYTES, "JSON byte limit")
    try:
        obj = json.loads(data.decode("utf-8"), object_pairs_hook=pairs, parse_constant=reject_constant)
        pending = [(obj, 0)]
        while pending:
            value, depth = pending.pop()
            need(depth <= 32, "JSON depth limit")
            if isinstance(value, (dict, list)):
                children = value.values() if isinstance(value, dict) else value
                pending.extend((child, depth + 1) for child in children)
        need(isinstance(obj, dict), "JSON object required")
        return obj
    except (UnicodeError, RecursionError, json.JSONDecodeError) as error:
        raise CheckError("invalid JSON") from error


def read_json(path):
    # Do not block on a FIFO/device or read an unbounded file. No stdin or env.
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        need(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), "regular JSON file required")
        return decode(stream.read(MAX_BYTES + 1))


def expected_rows(record):
    need(type(record["record_version"]) is int and record["record_version"] == 1, "record version")
    chain = record["chain"]
    need(isinstance(chain["chain_id"], str) and isinstance(chain["network_id"], str), "record identity")
    digest(chain["genesis_hash"])
    need(record["genesis"] == dict(activation_schedule=None, signing_version=1, features=[]), "record genesis")
    uint(record["genesis"]["signing_version"])
    result = []
    for entry in record["bootstrap"]:
        result.append((entry["feature"], uint(entry["activation_height"]), "bootstrap", None, None))
    extensions = record["extensions"]
    need(isinstance(extensions, list) and 0 < len(extensions) <= 16, "record extensions")
    for sequence, extension in enumerate(extensions):
        need(uint(extension["sequence"]) == sequence, "record sequence")
        carried = uint(extension["carried_at_height"])
        if sequence:
            need(carried > extensions[sequence - 1]["carried_at_height"], "record carry order")
        need(extension["version_steps"] == [], "record version steps unsupported")
        uint(extension["min_protocol_version"])
        digest(extension["transaction_id"])
        need(re.fullmatch(r"[0-9a-f]{40}", extension["release_commit"]), "record release")
        need(isinstance(extension["entries"], list) and 0 < len(extension["entries"]) <= 8, "record entry count")
        for entry in extension["entries"]:
            height = uint(entry["activation_height"])
            need(height >= carried + 2000, "record carry window")
            result.append((entry["feature"], height, "extension", sequence, carried))
    names = [row[0] for row in result]
    need(all(isinstance(n, str) and re.fullmatch(r"[a-z][a-z0-9_]{0,63}", n) for n in names), "record feature name")
    need(len(set(names)) == len(names), "duplicate record feature")
    binding = record["faucet_binding"]
    need((binding["feature"], uint(binding["activation_height"])) in [(r[0], r[1]) for r in result], "record faucet activation")
    need(re.fullmatch(r"0x[0-9a-f]{40}", binding["address"]), "record public faucet address")
    return result


def compare(record, sample):
    rows = expected_rows(record)
    before, after, upgrade = sample["info_before"], sample["info_after"], sample["upgrade"]
    for info in (before, after):
        need(info["chain_id"] == record["chain"]["chain_id"] and
             info["network_id"] == record["chain"]["network_id"] and
             digest(info["genesis_hash"]) == digest(record["chain"]["genesis_hash"]), "node identity mismatch")
    tip = uint(upgrade["tip_height"])
    need(uint(before["chain_height"]) == uint(after["chain_height"]) == tip, "moving node sample")
    need(tip < U64_MAX and tip >= record["extensions"][-1]["carried_at_height"], "node history height")
    need(upgrade["status"] in ("none", "ready"), "node notice status")
    need(uint(upgrade["local_protocol_version"]) >= max(e["min_protocol_version"] for e in record["extensions"]), "node protocol")
    scheduled = upgrade["scheduled"]
    need(scheduled["status"] == "ready" and scheduled["base_provisional"] is False and
         scheduled["truncated"] is False and scheduled["warnings"] == [] and
         scheduled["halt_deadline"] is None, "node schedule readiness")
    need(uint(scheduled["next_height"]) == tip + 1 and
         uint(scheduled["version_at_next"]) == record["genesis"]["signing_version"], "node next signing height/version")
    entries = scheduled["entries"]
    need(isinstance(entries, list) and len(entries) == uint(scheduled["total_entries"]) == len(rows), "node schedule count")
    actual = []
    for entry in entries:
        height = uint(entry["activation_height"])
        need(entry["known"] is True and entry["active_at_next"] is (tip + 1 >= height) and
             uint(entry["blocks_remaining"]) == max(0, height - tip), "node activation boundary")
        sequence, carried = entry.get("extension_sequence"), entry.get("carried_at_height")
        if entry["source"] == "extension":
            uint(sequence)
            uint(carried)
        else:
            need("extension_sequence" not in entry and "carried_at_height" not in entry, "node base provenance")
        actual.append((entry["feature"], height, entry["source"], sequence, carried))
    need(sorted(actual) == sorted(rows), "node schedule drift")
    binding = record["faucet_binding"]
    active = tip + 1 >= binding["activation_height"]
    expected_sink = dict(address=binding["address"] if active else None,
                         source="binding" if active else "none",
                         active_from=binding["activation_height"] if active else None,
                         at_height=tip + 1)
    sink = upgrade["faucet_sink"]
    uint(sink["at_height"])
    if sink["active_from"] is not None:
        uint(sink["active_from"])
    need(sink == expected_sink, "node faucet sink drift")
    return dict(status="MATCH", chain_id=record["chain"]["chain_id"], tip_height=tip,
                next_height=tip + 1, feature_count=len(rows),
                evidence="node projection comparison; not signature or activation proof")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args):
        return None


@contextlib.contextmanager
def http_deadline():
    need(hasattr(signal, "setitimer") and signal.getitimer(signal.ITIMER_REAL) == (0, 0), "HTTP deadline unavailable")
    old = signal.getsignal(signal.SIGALRM)

    def expired(*args):
        raise CheckError("HTTP deadline")

    signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, HTTP_SECONDS)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, old)


def fetch_sample(base):
    parsed = urllib.parse.urlsplit(base)
    need(parsed.scheme in ("http", "https") and parsed.hostname and parsed.username is None and
         parsed.password is None and parsed.path in ("", "/") and not parsed.query and not parsed.fragment,
         "invalid node base URL")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    outputs = []
    for path in ("/network/info", "/network/upgrade", "/network/info"):
        request = urllib.request.Request(base.rstrip("/") + path, method="GET", headers={"Accept": "application/json"})
        try:
            with http_deadline(), opener.open(request, timeout=5) as response:
                need(response.status == 200, "HTTP status")
                outputs.append(decode(response.read(MAX_BYTES + 1)))
        except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException) as error:
            raise CheckError("HTTP read refused") from error
    return dict(zip(("info_before", "upgrade", "info_after"), outputs))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record", type=Path, default=DEFAULT_RECORD)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--sample", type=Path, help="bounded regular JSON file with info_before, upgrade, info_after")
    source.add_argument("--node-url", help="owner run: exactly three GETs, no redirect, proxy, retry or credentials")
    args = parser.parse_args()
    try:
        record = read_json(args.record)
        expected_rows(record)  # Validate before any network request.
        sample = read_json(args.sample) if args.sample else fetch_sample(args.node_url)
        report = compare(record, sample)
        report["record_sha256"] = hashlib.sha256(json.dumps(record, sort_keys=True).encode()).hexdigest()
        print(json.dumps(report, sort_keys=True))
        return 0
    except (CheckError, KeyError, TypeError, ValueError, OSError) as error:
        label = str(error) if isinstance(error, CheckError) else "invalid input or unavailable read"
        print("STOP: " + label, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
