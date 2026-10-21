import copy
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

spec = importlib.util.spec_from_file_location("audit_owners", Path(__file__).with_name("audit_owners.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
OWNER = "0x" + "31" * 20


def page():
    return {"stations": [
        {"station_id": "station-a", "is_active": True, "owner_address": "wrf" + "31" * 20,
         "latitude": 12.123456, "longitude": 34.654321, "elevation_meters": 42},
        {"station_id": "station-b", "is_active": False, "owner_address": "invalid"}],
        "has_more": False, "next_cursor": None, "total": 2,
        "observed_at_height": 34000, "settled_height": 33700, "state_root_hex": "ab" * 32}


class AuditTests(unittest.TestCase):
    def run_audit(self, pages=None, account=None, **kwargs):
        pages = iter(pages or [page(), page()])
        account = account or (200, {"ok": True, "data": {"address": OWNER, "balance": "100"}})
        calls = []
        def get(path):
            calls.append(path)
            return account if path.startswith("/addresses/") else (200, next(pages))
        kwargs.setdefault("sleep", lambda _: None)
        result = audit.audit(get, **kwargs)
        return result, calls

    def test_complete_canonical_ids_only_and_inactive_owner_excluded(self):
        result, calls = self.run_audit()
        self.assertEqual(result["status"], "AUDIT-PASS")
        self.assertEqual(result["active_count"], 1)
        self.assertEqual(result["owner_account_ids"], [OWNER])
        self.assertEqual(result["station_owner_ids"], [{"station_id": "station-a", "owner_account_id": OWNER}])
        self.assertEqual(len(calls), 3)
        text = json.dumps(result)
        for private in ("latitude", "longitude", "elevation", "12.123456", "34.654321"):
            self.assertNotIn(private, text)

    def test_missing_invalid_and_zero_are_no_go(self):
        for account in [(404, {"ok": False, "error": {"code": "NOT_FOUND"}}),
                        (200, {"ok": True, "data": {"address": OWNER, "balance": "0"}})]:
            self.assertEqual(self.run_audit(account=account)[0]["status"], "NO-GO")
        bad = page()
        bad["stations"][0]["owner_address"] = "not-account"
        self.assertEqual(self.run_audit([bad, bad])[0]["invalid_owner_station_ids"], ["station-a"])

    def test_multi_page_no_prefix_success(self):
        first, second = page(), page()
        first["stations"] = first["stations"][:1]
        first.update(has_more=True, next_cursor="station-a")
        second["stations"] = second["stations"][1:]
        result, calls = self.run_audit([first, second, page()], limit=1)
        self.assertEqual(result["registry_count"], 2)
        self.assertIn("after=station-a", calls[1])
        with self.assertRaisesRegex(audit.Stop, "PAGE_BOUND"):
            self.run_audit([first], max_pages=1)

    def test_state_height_root_and_count_drift_stop(self):
        for key, value in [("observed_at_height", 34001), ("state_root_hex", "cd" * 32), ("total", 3)]:
            changed = page()
            changed[key] = value
            with self.assertRaisesRegex(audit.Stop, "CANONICAL_STATE_MOVED"):
                self.run_audit([page(), changed])

    def test_shape_cursor_order_incomplete_and_request_bounds(self):
        for mutate, code in [
            (lambda p: p.update(total=3), "REGISTRY_INCOMPLETE"),
            (lambda p: p.update(has_more=True, next_cursor="wrong"), "CURSOR_INVALID"),
            (lambda p: p["stations"].reverse(), "PAGE_ORDER_INVALID"),
            (lambda p: p.update(observed_at_height=True), "SHAPE_INVALID"),
        ]:
            bad = page()
            mutate(bad)
            with self.assertRaisesRegex(audit.Stop, code):
                self.run_audit([bad])
        with self.assertRaisesRegex(audit.Stop, "READ_BUDGET"):
            self.run_audit(max_reads=1)

    def test_account_mismatch_and_error_are_not_missing(self):
        for account in [(200, {"ok": True, "data": {"address": "0x" + "32" * 20, "balance": "1"}}),
                        (503, {"private": "server-text"}),
                        (404, {"ok": False, "error": {"code": "OTHER"}})]:
            with self.assertRaises(audit.Stop):
                self.run_audit(account=account)

    def test_url_and_token_refusals(self):
        for base in ["http://example.test", "https://user@example.test", "https://example.test/path"]:
            with self.assertRaisesRegex(audit.Stop, "BASE_INVALID"):
                audit.reader(base, "synthetic")
        with self.assertRaisesRegex(audit.Stop, "TOKEN_INVALID"):
            audit.reader("http://127.0.0.1:12345", "synthetic\rINJECT")

    def test_real_cli_transport_is_get_only_and_sanitized(self):
        seen = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                seen.append((self.command, self.path, self.headers.get("Authorization")))
                data = ({"ok": True, "data": {"address": OWNER, "balance": "100"}}
                        if self.path.startswith("/addresses/") else page())
                payload = json.dumps(data).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
        server = HTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            proc = subprocess.run([sys.executable, str(Path(audit.__file__)), "--base",
                                   f"http://127.0.0.1:{server.server_port}", "--delay", "1"],
                                  input="synthetic-audit-token\n", text=True, capture_output=True, timeout=10)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertEqual(json.loads(proc.stdout)["status"], "AUDIT-PASS")
            self.assertEqual([x[0] for x in seen], ["GET"] * 3)
            self.assertTrue(all(x[2] == "Bearer synthetic-audit-token" for x in seen))
            for private in ("latitude", "longitude", "12.123456", "synthetic-audit-token"):
                self.assertNotIn(private, proc.stdout + proc.stderr)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


class Fleet:
    """Fake transport for a registry of `n` stations, each with its own funded owner."""
    def __init__(self, n, per_page=100):
        self.n, self.per_page, self.reads = n, per_page, []
        self.owners = ["0x%040x" % (i + 1) for i in range(n)]

    def get(self, path):
        self.reads.append(path)
        if path.startswith("/addresses/"):
            owner = path.split("/")[-1]
            return 200, {"ok": True, "data": {"address": owner, "balance": "5"}}
        after = path.split("after=")[1] if "after=" in path else ""
        ids = ["s%04d" % i for i in range(self.n)]
        rest = [i for i in ids if i > after][:self.per_page]
        more = bool(rest) and rest[-1] != ids[-1]
        return 200, {"stations": [{"station_id": i, "is_active": True,
                                   "owner_address": self.owners[int(i[1:])]} for i in rest],
                     "has_more": more, "next_cursor": rest[-1] if more else None, "total": self.n,
                     "observed_at_height": 100, "settled_height": 90, "state_root_hex": "ab" * 32}


class PacedBudgetTests(unittest.TestCase):
    def test_single_run_never_exceeds_30_reads_and_refuses_larger_budget(self):
        fleet = Fleet(100)  # 1 page + 100 owners + bracket = 102 reads unpaced
        with self.assertRaisesRegex(audit.Stop, "READ_BUDGET"):
            audit.audit(fleet.get, sleep=lambda _: None)
        self.assertLessEqual(len(fleet.reads), 30)
        for bad in (31, 128, 0, -1, True):
            with self.assertRaisesRegex(audit.Stop, "READ_BOUND_EXCEEDS_30"):
                audit.audit(Fleet(1).get, max_reads=bad, sleep=lambda _: None)

    def test_reads_are_paced_with_a_fixed_delay_and_never_retried(self):
        fleet, naps = Fleet(3), []
        result = audit.audit(fleet.get, sleep=naps.append, delay=2.5)
        self.assertEqual(result["status"], "AUDIT-PASS")
        self.assertEqual(len(fleet.reads), 5)
        self.assertEqual(naps, [2.5] * 4)  # between reads, not before the first
        self.assertEqual(len(set(fleet.reads[1:4])), 3)  # no path read twice
        with self.assertRaisesRegex(audit.Stop, "DELAY_TOO_SHORT"):
            audit.audit(fleet.get, delay=0.1, sleep=naps.append)

    def test_resumable_audit_spans_runs_with_at_most_30_reads_each(self):
        fleet, saved, state = Fleet(100), [], audit.new_state("node-a")
        runs = []
        while True:
            before = len(fleet.reads)
            result = audit.audit(fleet.get, sleep=lambda _: None, state=state,
                                 save=lambda s: saved.append(json.dumps(s)), identity="node-a")
            runs.append(len(fleet.reads) - before)
            if result["status"] != "PARTIAL":
                break
            self.assertEqual(result["reads_this_run"], 30)
            state = json.loads(saved[-1])  # as if reloaded from the state file
            self.assertLess(len(runs), 10)
        self.assertTrue(all(n <= 30 for n in runs), runs)
        self.assertGreater(len(runs), 1)
        self.assertEqual(result["status"], "AUDIT-PASS")
        self.assertEqual(result["owner_count"], 100)
        self.assertEqual(len(set(r for r in fleet.reads if r.startswith("/addresses/"))), 100)
        self.assertEqual(result["request_count"], len(fleet.reads))

    def test_resume_stops_if_the_registry_moved_between_runs(self):
        fleet, state, saved = Fleet(60), audit.new_state("n"), []
        first = audit.audit(fleet.get, sleep=lambda _: None, state=state, identity="n",
                            save=lambda s: saved.append(json.dumps(s)))
        self.assertEqual(first["status"], "PARTIAL")
        fleet.get_orig = fleet.get
        def moved(path):
            status, obj = fleet.get_orig(path)
            if "state_root_hex" in obj:
                obj["observed_at_height"] = 101
            return status, obj
        with self.assertRaisesRegex(audit.Stop, "CANONICAL_STATE_MOVED"):
            audit.audit(moved, sleep=lambda _: None, state=json.loads(saved[-1]), identity="n")

    def test_state_is_validated_and_bound_to_one_node(self):
        state = audit.new_state("node-a")
        with self.assertRaisesRegex(audit.Stop, "STATE_IDENTITY_MISMATCH"):
            audit.audit(Fleet(1).get, sleep=lambda _: None, state=state, identity="node-b")
        for mutate in (lambda s: s.update(version=2), lambda s: s.update(stations="x"),
                       lambda s: s.update(owner_results={"nope": "ok"}),
                       lambda s: s.update(expected=[1, 2, "zz", 3])):
            bad = audit.new_state("node-a")
            mutate(bad)
            with self.assertRaisesRegex(audit.Stop, "STATE_"):
                audit.audit(Fleet(1).get, sleep=lambda _: None, state=bad, identity="node-a")

    def test_corrupted_checkpoint_can_never_pass_or_nogo_on_a_partial_registry(self):
        """SOL-R3 P1 on #3470: removing a saved row (zero-balance owner) must not yield AUDIT-PASS."""
        fleet = Fleet(2)
        fleet.zero = {fleet.owners[1]}
        base_get = fleet.get
        def get(path):
            status, obj = base_get(path)
            if path.endswith(fleet.owners[1]):
                obj["data"]["balance"] = "0"
            return status, obj
        saved = []
        first = audit.audit(get, max_reads=2, sleep=lambda _: None, state=audit.new_state("n"),
                            identity="n", save=lambda s: saved.append(json.dumps(s)))
        self.assertEqual(first["status"], "PARTIAL")
        intact = json.loads(saved[-1])
        self.assertTrue(intact["registry_done"])
        self.assertEqual(audit.audit(get, sleep=lambda _: None, state=json.loads(saved[-1]),
                                     identity="n")["status"], "NO-GO")

        def drop_row(s):
            del s["stations"][1]
        def reorder(s):
            s["stations"].reverse()
        def duplicate(s):
            s["stations"][1] = list(s["stations"][0])
        def wrong_previous(s):
            s["previous"] = "s0000"
        def cursor_when_done(s):
            s["cursor"] = "s0001"
        def not_done_but_full(s):
            s["registry_done"] = False
        def no_context(s):
            s["expected"] = None
        def results_for_unknown_owner(s):
            s["owner_results"]["0x" + "99" * 20] = "ok"
        def results_before_registry_done(s):
            s["registry_done"] = False
            s["cursor"], s["previous"] = "s0001", "s0001"
            s["stations"] = s["stations"][:1]
            s["previous"] = "s0000"
            s["cursor"] = "s0000"
            s["owner_results"] = {fleet.owners[0]: "ok"}
        for mutate in (drop_row, reorder, duplicate, wrong_previous, cursor_when_done,
                       not_done_but_full, no_context, results_for_unknown_owner,
                       results_before_registry_done):
            bad = json.loads(saved[-1])
            mutate(bad)
            with self.assertRaisesRegex(audit.Stop, "STATE_INVALID|REGISTRY_INCOMPLETE", msg=mutate.__name__):
                audit.audit(get, sleep=lambda _: None, state=bad, identity="n")
        # A partially collected registry resumes only if its cursor and order are coherent.
        part = audit.new_state("n")
        audit.audit(Fleet(250).get, max_reads=1, sleep=lambda _: None, state=part, identity="n")
        self.assertFalse(part["registry_done"])
        bad = json.loads(json.dumps(part))
        bad["cursor"] = "s0000"
        with self.assertRaisesRegex(audit.Stop, "STATE_INVALID"):
            audit.audit(Fleet(250).get, sleep=lambda _: None, state=bad, identity="n")
        bad = json.loads(json.dumps(part))
        bad["stations"] = bad["stations"][:-1]
        with self.assertRaisesRegex(audit.Stop, "STATE_INVALID"):
            audit.audit(Fleet(250).get, sleep=lambda _: None, state=bad, identity="n")

    def test_cli_partial_then_resume_writes_state_0600_and_clears_it_when_done(self):
        fleet = Fleet(40)
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_GET(self):
                payload = json.dumps(fleet.get(self.path)[1]).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
        server = HTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                path = str(Path(tmp, "state.json"))
                argv = [sys.executable, str(Path(audit.__file__)), "--base",
                        f"http://127.0.0.1:{server.server_port}", "--delay", "1",
                        "--max-reads", "15", "--state", path]
                codes = []
                for _ in range(6):
                    before = len(fleet.reads)
                    proc = subprocess.run(argv, input="synthetic-audit-token\n", text=True,
                                          capture_output=True, timeout=60)
                    self.assertLessEqual(len(fleet.reads) - before, 15)
                    codes.append(proc.returncode)
                    if proc.returncode != 3:
                        break
                    self.assertEqual(json.loads(proc.stdout)["status"], "PARTIAL")
                    self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
                    self.assertNotIn("synthetic-audit-token", Path(path).read_text())
                self.assertEqual(codes[-1], 0, (codes, proc.stdout, proc.stderr))
                self.assertGreater(len(codes), 1)
                self.assertFalse(os.path.exists(path))
                proc = subprocess.run(argv[:-4] + ["--max-reads", "31"], input="t\n", text=True,
                                      capture_output=True, timeout=10)
                self.assertEqual(json.loads(proc.stdout), {"status": "STOP", "code": "READ_BOUND_EXCEEDS_30"})
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    unittest.main()
