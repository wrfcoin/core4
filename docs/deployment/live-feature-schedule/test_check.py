"""Offline and loopback controls for the actual deployment-record checker."""

import contextlib
import copy
import http.server
import importlib.util
import json
import os
import pathlib
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).parent
RECORD = HERE / "777-testnet.json"
GENESIS = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"  # pragma: allowlist secret -- public block-1 hash from compiled bootstrap
FAUCET = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"
ROWS = [
    ("schedule_extension", 29800, "bootstrap", None, None),
    ("reward_gross_outlier_guard", 32500, "extension", 0, 29802),
    ("weather_skew_block_time_allowance", 32500, "extension", 0, 29802),
    ("faucet_sink_binding", 33000, "extension", 1, 30656),
]


def checker():
    spec = importlib.util.spec_from_file_location("schedule_check", HERE / "check.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sample(tip=30660):
    info = dict(chain_id="777", network_id="testnet", genesis_hash=GENESIS, chain_height=tip)
    rows = []
    for feature, height, source, sequence, carried in ROWS:
        row = dict(feature=feature, activation_height=height, source=source,
                   known=True, active_at_next=tip + 1 >= height,
                   blocks_remaining=max(0, height - tip))
        if sequence is not None:
            row.update(extension_sequence=sequence, carried_at_height=carried)
        rows.append(row)
    active = tip + 1 >= 33000
    upgrade = dict(status="none", local_protocol_version=3, tip_height=tip,
                   scheduled=dict(status="ready", next_height=tip + 1, version_at_next=1,
                                  base_provisional=False, entries=rows, total_entries=4,
                                  truncated=False, warnings=[], halt_deadline=None),
                   faucet_sink=dict(address=FAUCET if active else None,
                                    source="binding" if active else "none",
                                    active_from=33000 if active else None, at_height=tip + 1))
    return dict(info_before=info, upgrade=upgrade, info_after=copy.deepcopy(info))


@contextlib.contextmanager
def server(responses):
    calls = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            calls.append((self.command, self.path))
            if self.headers.get("Authorization") or self.headers.get("Cookie"):
                calls.append(("CREDENTIAL_HEADER", "present"))
            status, body, delay = responses[self.path]
            time.sleep(delay)
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def log_message(self, *args):
            pass

    node = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=node.serve_forever, daemon=True)
    thread.start()
    try:
        yield "http://127.0.0.1:" + str(node.server_port), calls
    finally:
        node.shutdown()
        node.server_close()
        thread.join()


class ScheduleRecordTests(unittest.TestCase):
    def test_committed_deployment_record_contains_carried_schedule(self):
        record = json.loads(RECORD.read_text())
        self.assertEqual(record["chain"]["genesis_hash"], GENESIS)
        self.assertEqual(record["genesis"], dict(activation_schedule=None, signing_version=1, features=[]))
        self.assertEqual(record["bootstrap"], [dict(feature="schedule_extension", activation_height=29800)])
        self.assertEqual([(e["sequence"], e["carried_at_height"], e["entries"])
                          for e in record["extensions"]], [
            (0, 29802, [dict(feature="reward_gross_outlier_guard", activation_height=32500),
                        dict(feature="weather_skew_block_time_allowance", activation_height=32500)]),
            (1, 30656, [dict(feature="faucet_sink_binding", activation_height=33000)])])
        self.assertEqual([e["transaction_id"] for e in record["extensions"]], [
            "7122087aea65f4cc702e4c510b2647ec5ab32fa0c670e9249a2e433db5e53b76",  # pragma: allowlist secret -- public E0 transaction id from inclusion record
            "01701cb0735df620960d53e37cafd77db2022f0222dac8b4683548a567c2cabf"])  # pragma: allowlist secret -- public E0f transaction id from inclusion record
        for extension in record["extensions"]:
            self.assertEqual(extension["version_steps"], [])
            self.assertEqual(extension["min_protocol_version"], 3)
            self.assertEqual(extension["release_commit"], "65134a64492135b6ac81344dd0d2edd8dfaf1309")  # pragma: allowlist secret -- public B3 Git revision

    def test_record_matches_compiled_bootstrap_and_public_binding(self):
        root = HERE.parents[2]
        source = (root / "shared-protocol/src/crypto/chain_binding.rs").read_text()
        bootstrap = source.split("pub const LEGACY_CHAIN_BOOTSTRAP:", 1)[1].split("];", 1)[0]
        self.assertIn('chain_id: "777"', bootstrap)
        self.assertIn('network_id: "testnet"', bootstrap)
        self.assertIn('genesis_hash: "' + GENESIS + '"', bootstrap)
        self.assertIn('feature: ConsensusFeature::ScheduleExtension', bootstrap)
        self.assertIn('height: H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE', bootstrap)
        self.assertIn('pub const H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE: u64 = 29_800;', source)
        binding = source.split("pub const LEGACY_FAUCET_BINDINGS:", 1)[1].split("];", 1)[0]
        self.assertIn('address: Some("' + FAUCET[2:] + '")', binding)

    def test_activation_boundaries_use_next_height(self):
        m = checker()
        for tip in (30656, 32498, 32499, 32500, 32998, 32999, 33000, 33001):
            with self.subTest(tip=tip):
                result = m.compare(m.read_json(RECORD), sample(tip))
                self.assertEqual(result["status"], "MATCH")
                self.assertEqual(result["tip_height"], tip)

    def test_identity_schedule_and_health_drift_refuse(self):
        m, record = checker(), json.loads(RECORD.read_text())
        changes = [
            ("identity", lambda s: s["info_before"].update(chain_id="778")),
            ("genesis", lambda s: s["info_before"].update(genesis_hash="00" * 32)),
            ("moving tip", lambda s: s["info_after"].update(chain_height=30661)),
            ("missing row", lambda s: s["upgrade"]["scheduled"]["entries"].pop()),
            ("E1", lambda s: s["upgrade"]["scheduled"]["entries"].append(dict(feature="station_owner_slash_identity"))),
            ("height", lambda s: s["upgrade"]["scheduled"]["entries"][1].update(activation_height=32501)),
            ("sequence", lambda s: s["upgrade"]["scheduled"]["entries"][1].update(extension_sequence=1)),
            ("carry", lambda s: s["upgrade"]["scheduled"]["entries"][1].update(carried_at_height=29803)),
            ("source", lambda s: s["upgrade"]["scheduled"]["entries"][0].update(source="genesis")),
            ("duplicate", lambda s: s["upgrade"]["scheduled"]["entries"].append(copy.deepcopy(s["upgrade"]["scheduled"]["entries"][0]))),
            ("active", lambda s: s["upgrade"]["scheduled"]["entries"][0].update(active_at_next=False)),
            ("bool height", lambda s: s["upgrade"]["scheduled"].update(version_at_next=True)),
            ("counter", lambda s: s["upgrade"]["scheduled"]["entries"][0].update(blocks_remaining=1)),
            ("unknown", lambda s: s["upgrade"]["scheduled"]["entries"][0].update(known=False)),
            ("truncated", lambda s: s["upgrade"]["scheduled"].update(truncated=True)),
            ("provisional", lambda s: s["upgrade"]["scheduled"].update(base_provisional=True)),
            ("warning", lambda s: s["upgrade"]["scheduled"].update(warnings=[{}])),
            ("halt", lambda s: s["upgrade"]["scheduled"].update(halt_deadline={})),
            ("version", lambda s: s["upgrade"]["scheduled"].update(version_at_next=2)),
            ("total", lambda s: s["upgrade"]["scheduled"].update(total_entries=5)),
            ("protocol", lambda s: s["upgrade"].update(local_protocol_version=2)),
            ("sink", lambda s: s["upgrade"]["faucet_sink"].update(address=FAUCET)),
        ]
        for label, mutate in changes:
            with self.subTest(label=label):
                s = sample()
                mutate(s)
                with self.assertRaises(m.CheckError):
                    m.compare(record, s)

    def test_record_ambiguity_refuses(self):
        m = checker()
        for update in (lambda r: r["extensions"][1].update(sequence=0),
                       lambda r: r["extensions"][0]["entries"][0].update(activation_height=True),
                       lambda r: r["extensions"][1].update(carried_at_height=33001),
                       lambda r: r["bootstrap"].append(copy.deepcopy(r["bootstrap"][0]))):
            r = json.loads(RECORD.read_text())
            update(r)
            with self.assertRaises(m.CheckError):
                m.compare(r, sample())

    def test_bounded_strict_json(self):
        m = checker()
        with tempfile.TemporaryDirectory() as directory:
            p = pathlib.Path(directory) / "input.json"
            for content in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{}' + b' ' * m.MAX_BYTES,
                            b'[' * 100 + b']' * 100, b'\xff', '{}'.encode('utf-16')):
                p.write_bytes(content)
                with self.assertRaises(m.CheckError):
                    m.read_json(p)

    def test_actual_http_three_gets_and_no_credentials(self):
        m, s = checker(), sample()
        responses = {"/network/info": (200, json.dumps(s["info_before"]).encode(), 0),
                     "/network/upgrade": (200, json.dumps(s["upgrade"]).encode(), 0)}
        with server(responses) as (base, calls), mock.patch.dict(os.environ, {
            "HTTP_PROXY": "http://127.0.0.1:1", "http_proxy": "http://127.0.0.1:1",
            "NO_PROXY": "", "no_proxy": "", "WRFCOIN_READER_TOKEN": "synthetic-canary",
        }):
            result = m.compare(m.read_json(RECORD), m.fetch_sample(base))
        self.assertEqual(result["status"], "MATCH")
        self.assertEqual(calls, [("GET", "/network/info"), ("GET", "/network/upgrade"),
                                 ("GET", "/network/info")])

    def test_http_error_redirect_size_timeout_no_retry(self):
        m = checker()
        m.HTTP_SECONDS = 0.15
        s = sample()
        valid_info = json.dumps(s["info_before"]).encode()
        for status, body, delay in ((302, b"{}", 0), (500, b"private-response", 0),
                                    (200, b'{"x":"' + b' ' * m.MAX_BYTES + b'"}', 0),
                                    (200, valid_info, 0.4)):
            with self.subTest(status=status, delay=delay):
                with server({"/network/info": (status, body, delay),
                             "/network/upgrade": (200, json.dumps(s["upgrade"]).encode(), 0)}) as (base, calls):
                    with self.assertRaises(m.CheckError):
                        m.fetch_sample(base)
                self.assertEqual(calls, [("GET", "/network/info")])
        for base in ("http://user:secret@localhost", "http://localhost/x", "file:///tmp/a",  # pragma: allowlist secret -- synthetic rejected URL userinfo, no real credential
                     "http://localhost?token=x"):
            with self.assertRaises(m.CheckError):
                m.fetch_sample(base)

    def test_actual_cli_offline_and_sanitized_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "view.json"
            path.write_text(json.dumps(sample()))
            args = [sys.executable, str(HERE / "check.py"), "--sample", str(path)]
            good = subprocess.run(args, capture_output=True, text=True)
            self.assertEqual(good.returncode, 0, good.stderr)
            self.assertEqual(json.loads(good.stdout)["status"], "MATCH")
            path.write_text('{"private-marker": "never-print-me"}')
            bad = subprocess.run(args, capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1)
            self.assertNotIn("never-print-me", bad.stdout + bad.stderr)

    def test_actual_cli_malformed_http_sanitized_refusal(self):
        for wire in (b"wire-private-marker\r\n\r\n", b"",
                     b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ninvalid-chunk\r\n"):
            calls = []

            class Handler(socketserver.BaseRequestHandler):
                def handle(self):
                    self.request.settimeout(2)
                    calls.append(self.request.recv(4096))
                    self.request.sendall(wire)

            with self.subTest(wire=wire), socketserver.TCPServer(("127.0.0.1", 0), Handler) as node:
                thread = threading.Thread(target=node.serve_forever, daemon=True)
                thread.start()
                try:
                    base = "http://127.0.0.1:" + str(node.server_address[1])
                    result = subprocess.run([sys.executable, str(HERE / "check.py"), "--node-url", base],
                                            capture_output=True, text=True, timeout=5)
                    self.assertEqual(result.returncode, 1)
                    self.assertEqual(result.stdout, "")
                    self.assertEqual(result.stderr, "STOP: HTTP read refused\n")
                    self.assertEqual(len(calls), 1)
                    self.assertTrue(calls[0].startswith(b"GET /network/info HTTP/1.1\r\n"))
                finally:
                    node.shutdown()
                    thread.join()


if __name__ == "__main__":
    unittest.main()
