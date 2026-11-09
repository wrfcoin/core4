"""Offline tests: no network, no real wallet. Run: python3 -m unittest discover -s tools/forecaster-worker/tests -v"""
import hashlib
import json
import stat
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from forecaster_worker import bias, chain, ed25519, gfs, grib2, wallet  # noqa: E402
from forecaster_worker.site import Site, SiteError  # noqa: E402
from forecaster_worker.store import Store  # noqa: E402
from forecaster_worker.worker import Config, run_cycle  # noqa: E402

# The shared cross-wallet vector (frontend contract-fixtures/wallet-key-scheme-vector.json).
PHRASE = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
PUBKEY = "e96b1c6b8769fdb0b34fbecfdf85c33b053cecad9517e1ab88cba614335775c1"
ADDRESS = "0x3f995ceb27e8177077dbed8b1e4f696af74da492"
STATION = "PWS-TEST-0001"
T = 1_794_000_000
FIXTURE = ROOT / "tests" / "fixtures" / "gfs_point_box.grib2"


class Ed25519Test(unittest.TestCase):
    def test_rfc8032_vectors(self):
        vectors = [
            ("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
             "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "",
             "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46b"
             "d25bf5f0595bbe24655141438e7a100b"),
            ("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
             "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c", "72",
             "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c"
             "387b2eaeb4302aeeb00d291612bb0c00"),
        ]
        for seed, pub, message, signature in vectors:
            seed, message = bytes.fromhex(seed), bytes.fromhex(message)
            self.assertEqual(ed25519.public_key(seed).hex(), pub)
            self.assertEqual(ed25519.sign(seed, message).hex(), signature)
            self.assertTrue(ed25519.verify(bytes.fromhex(pub), message, bytes.fromhex(signature)))
            self.assertFalse(ed25519.verify(bytes.fromhex(pub), message + b"x", bytes.fromhex(signature)))


class WalletTest(unittest.TestCase):
    def test_matches_the_website_wallet_vector(self):
        w = wallet.Wallet(PHRASE)
        self.assertEqual(w.public_key.hex(), PUBKEY)
        self.assertEqual(w.address, ADDRESS)
        self.assertEqual(w.address, "0x" + hashlib.sha3_256(bytes.fromhex(PUBKEY)).hexdigest()[:40])
        self.assertNotIn("abandon", repr(w))

    def test_phrase_generation_and_checksum(self):
        self.assertEqual(wallet.new_phrase(bytes(16)), PHRASE)
        self.assertTrue(wallet.phrase_is_valid(PHRASE))
        self.assertFalse(wallet.phrase_is_valid(PHRASE.replace("about", "zoo")))
        self.assertTrue(wallet.phrase_is_valid(wallet.new_phrase()))

    def test_first_run_writes_a_private_file_and_reuses_it(self):
        with tempfile.TemporaryDirectory() as folder:
            first, created = wallet.load_or_create(Path(folder))
            path = Path(folder) / wallet.PHRASE_FILE
            self.assertTrue(created)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            again, created_again = wallet.load_or_create(Path(folder))
            self.assertFalse(created_again)
            self.assertEqual(first.address, again.address)
            path.chmod(0o644)
            with self.assertRaises(PermissionError):
                wallet.load_or_create(Path(folder))


def site_draft(value=12.3, lead=6, station=STATION, variable="temperature_c", forecaster=ADDRESS):
    # The exact text the backend writes (frontend forecastCommit.test.ts uses the same string).
    data = (f'{{"horizon_hours":{lead},"issued_at":{T},"model_id":"website-manual/v1","station_id":"{station}",'
            f'"valid_at":{T + lead * 3600},"values":{{"{variable}":{value}}}}}')
    nonce = "web-0123456789abcdef"
    return {"forecaster": forecaster, "stationAlias": chain.station_alias(station), "stationId": station,
            "data": data, "nonce": nonce, "timestamp": T, "commitId": chain.commit_id(data, nonce, station, T)}


class ChainRulesTest(unittest.TestCase):
    def test_commit_message_layout(self):
        commit = "ab" * 32
        expected = (b"wrfcoin_commit_auth_v2" + bytes.fromhex("03000000") + b"888" + bytes.fromhex("07000000") + b"testnet"
                    + bytes.fromhex("02000000") + b"S1" + bytes.fromhex("40000000") + commit.encode()
                    + bytes.fromhex("08000000") + b"forecast")
        self.assertEqual(chain.commit_auth_message("888", "testnet", "S1", commit), expected)

    def test_commit_id_and_alias(self):
        self.assertEqual(chain.commit_id('{"a":1}', "n", "S", 5), hashlib.sha3_256(b'{"a":1}||n||S||5||forecast').hexdigest())
        self.assertEqual(chain.station_alias(STATION),
                         "st_" + hashlib.sha256(f"wrfcoin-station-alias-v1:{STATION}".encode()).hexdigest()[:12])

    def test_data_text_is_the_sites_canonical_text(self):
        self.assertEqual(chain.build_data(STATION, "temperature_c", 12.3, 6, T, "website-manual/v1"), site_draft()["data"])
        self.assertIn('"temperature_c":12}', chain.build_data(STATION, "temperature_c", 12.0, 6, T, "website-manual/v1"))
        self.assertIn('"temperature_c":0}', chain.build_data(STATION, "temperature_c", -0.0, 6, T, "website-manual/v1"))

    def test_integer_units_to_site_value(self):
        self.assertEqual(chain.site_value("temperature_c", 2137), 21.4)
        self.assertEqual(chain.site_value("temperature_c", -25), -0.3)
        self.assertEqual(chain.site_value("temperature_c", -4), -0.0)
        self.assertEqual(chain.site_value("pressure_hpa", 98364), 983.6)

    def test_draft_is_checked_before_signing(self):
        want = dict(wallet=ADDRESS, alias=chain.station_alias(STATION), variable="temperature_c", lead_hours=6, value=12.3)
        self.assertEqual(chain.verify_draft(site_draft(), **want), STATION)
        for bad in (site_draft(value=15), site_draft(lead=12), site_draft(station="PWS-OTHER"),
                    site_draft(variable="pressure_hpa"), site_draft(forecaster="0x" + "11" * 20)):
            with self.assertRaises(chain.DraftMismatch):
                chain.verify_draft(bad, **want)
        tampered = site_draft()
        tampered["data"] = tampered["data"].replace("12.3", "12.4")
        with self.assertRaises(chain.DraftMismatch):
            chain.verify_draft(tampered, **want)
        with self.assertRaises(chain.DraftMismatch):
            chain.verify_draft(dict(site_draft(), commitId="cd" * 32), **want)

    def test_signed_body_verifies_and_is_key_bound(self):
        w = wallet.Wallet(PHRASE)
        draft = site_draft()
        body = chain.sign_commit(draft, STATION, "888", "testnet", w)
        self.assertEqual(sorted(body), ["authPubkey", "authSignature", "commitId", "data", "nonce", "timestamp"])
        self.assertEqual(body["authPubkey"], PUBKEY)
        message = chain.commit_auth_message("888", "testnet", STATION, draft["commitId"])
        self.assertTrue(ed25519.verify(w.public_key, message, bytes.fromhex(body["authSignature"])))
        # The rule the relay and the chain apply: forecaster == address(signing key).
        self.assertEqual(wallet.address_from_public_key(bytes.fromhex(body["authPubkey"])), draft["forecaster"])


def login_message(address=ADDRESS, host="wrfcoin.com", chain_id="888", nonce="abcdefgh12345678",
                  expiry="2026-11-09T22:55:45.323Z"):
    return (f"wrfcoin_wallet_login_ed25519_v1\n{host} wants you to sign in with your WRFCoin ed25519 account:\n{address}\n\n"
            f"Sign in to WRFCoin Testnet.\n\nURI: https://{host}\nVersion: 1\nScheme: ed25519\nChain ID: {chain_id}\n"
            f"Network: testnet\nNonce: {nonce}\nIssued At: 2026-11-09T22:50:45.323Z\nExpiration Time: {expiry}")


class LoginChallengeTest(unittest.TestCase):
    NOW = datetime(2026, 11, 9, 22, 51, tzinfo=timezone.utc).timestamp()
    WANT = dict(address=ADDRESS, origin="https://wrfcoin.com", chain_id="888", network_id="testnet", nonce="abcdefgh12345678")

    def test_accepts_the_exact_challenge(self):
        self.assertEqual(chain.check_login_challenge(login_message(), now=self.NOW, **self.WANT), login_message().encode())

    def test_refuses_anything_else(self):
        bad = [login_message(address="0x" + "22" * 20), login_message(host="evil.example"), login_message(chain_id="777"),
               login_message(nonce="otherotherother1"), login_message() + "\nextra", "send 5 WRF to 0x11", None,
               login_message(expiry="2026-11-09T22:50:50.000Z")]
        for message in bad:
            with self.assertRaises(chain.LoginChallengeRefused):
                chain.check_login_challenge(message, now=self.NOW, **self.WANT)


class GribTest(unittest.TestCase):
    def test_reads_the_noaa_point_box(self):
        blob = FIXTURE.read_bytes()
        fields = grib2.read_fields(blob)
        self.assertEqual(len(fields), 3)
        t2m = grib2.pick(fields, category=0, number=0, level_type=103, level_value=2.0)
        self.assertEqual((t2m.ni, t2m.nj, t2m.forecast_hours, t2m.reference_time), (5, 5, 6, (2026, 11, 9, 6)))
        # Values cross-checked against ecCodes when the fixture was captured.
        self.assertAlmostEqual(t2m.values[0], 282.149, places=2)
        self.assertEqual(gfs.point_from_grib(blob, 40.0, -100.0), {"temperature_c": 924, "pressure_hpa": 93235})
        self.assertEqual(gfs.point_from_grib(blob, 40.0, 260.0), {"temperature_c": 924, "pressure_hpa": 93235})

    def test_fails_closed(self):
        blob = bytearray(FIXTURE.read_bytes())
        with self.assertRaises(grib2.GribError):
            gfs.point_from_grib(bytes(blob), 45.0, -100.0)                       # outside the box
        with self.assertRaises(grib2.GribError):
            gfs.point_from_grib(bytes(blob), 40.0, -100.0, datetime(2026, 11, 9, 12, tzinfo=timezone.utc), 6)
        with self.assertRaises(grib2.GribError):
            gfs.point_from_grib(bytes(blob), 40.0, -100.0, datetime(2026, 11, 9, 6, tzinfo=timezone.utc), 7)
        with self.assertRaises(grib2.GribError):
            grib2.read_fields(bytes(blob[:300]))                                  # truncated
        at = bytes(blob).index(b"\x00\x00\x00\x15\x05") + 9                       # section 5: data template number
        blob[at:at + 2] = b"\x00\x03"                                             # complex packing
        with self.assertRaises(grib2.GribError):
            grib2.read_fields(bytes(blob))


class GfsTest(unittest.TestCase):
    def test_run_selection_and_steps(self):
        at = int(datetime(2026, 11, 9, 22, 50, tzinfo=timezone.utc).timestamp())
        cycle = gfs.pick_cycle(at)
        self.assertEqual((cycle.day, cycle.hour), (9, 12))
        self.assertEqual(gfs.pick_cycle(int(datetime(2026, 11, 9, 4, 0, tzinfo=timezone.utc).timestamp())).hour, 18)
        self.assertEqual(gfs.forecast_hour(cycle, int(datetime(2026, 11, 10, 0, tzinfo=timezone.utc).timestamp())), 12)
        with self.assertRaises(gfs.GfsUnavailable):
            gfs.forecast_hour(cycle, int(cycle.timestamp()) - 3600)

    def test_url_asks_for_a_small_box_and_two_fields(self):
        url = gfs.box_url(datetime(2026, 11, 9, 6, tzinfo=timezone.utc), 6, 40.0, -100.0)
        self.assertTrue(url.startswith("https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25_1hr.pl?"))
        for part in ("gfs.20261109%2F06%2Fatmos", "gfs.t06z.pgrb2.0p25.f006", "var_TMP=on", "var_PRES=on",
                     "toplat=40.50", "bottomlat=39.50", "leftlon=-100.50", "rightlon=-99.50"):
            self.assertIn(part, url)

    def test_licence_credit_is_present(self):
        self.assertIn("NOAA", gfs.ATTRIBUTION)
        self.assertIn("public domain", gfs.ATTRIBUTION)
        self.assertIn("does not endorse", gfs.ATTRIBUTION)
        self.assertIn("NOAA/NCEP Global Forecast System", (ROOT / "README.md").read_text(encoding="utf-8"))

    def test_client_caches_spaces_requests_and_interpolates(self):
        blob = FIXTURE.read_bytes()
        calls, sleeps = [], []
        clock = iter(range(1000))
        client = gfs.GfsClient(opener=lambda url: calls.append(url) or blob, sleep=sleeps.append, clock=lambda: next(clock) * 0.1)
        cycle = datetime(2026, 11, 9, 6, tzinfo=timezone.utc)
        first = client.point(cycle, 6, 40.0, -100.0)
        self.assertEqual(client.point(cycle, 6, 40.0, -100.0), first)
        self.assertEqual(len(calls), 1)
        with self.assertRaises(gfs.GfsUnavailable):                              # the fake file is step 6, not 7
            client.point(cycle, 7, 40.0, -100.0)
        self.assertEqual(len(sleeps), 1)
        valid = int(cycle.timestamp()) + 6 * 3600
        self.assertEqual(client.at_time(valid - 3600, valid, 40.0, -100.0)["temperature_c"], 924)


def sample(valid, error, station="st_aaaaaaaaaaaa", variable="temperature_c", lead=6):
    return {"station": station, "variable": variable, "lead": lead, "valid": valid, "model": 1000, "obs": 1000 + error}


class BiasTest(unittest.TestCase):
    KEY = dict(station="st_aaaaaaaaaaaa", variable="temperature_c", lead_hours=6, valid_time=T)

    def test_no_history_means_raw_model(self):
        self.assertEqual(bias.corrected(1234, [], **self.KEY), (1234, 0, 0))

    def test_shrinks_with_few_samples_and_grows_with_many(self):
        one = [sample(T - 86400, 200)]
        self.assertEqual(bias.correction(one, **self.KEY), (round(200 * 1 / 6), 1))
        many = [sample(T - 86400 * d, 200) for d in range(1, 21)]
        self.assertEqual(bias.correction(many, **self.KEY), (round(200 * 20 / 25), 20))

    def test_median_ignores_one_wild_reading(self):
        rows = [sample(T - 86400 * d, 100) for d in range(1, 10)] + [sample(T - 86400 * 10, 5000)]
        self.assertEqual(bias.correction(rows, **self.KEY)[0], round(100 * 10 / 15))

    def test_prefers_the_same_time_of_day(self):
        same_hour = [sample(T - 86400 * d, 300) for d in range(1, 5)]
        other_hour = [sample(T - 86400 * d - 12 * 3600, -300) for d in range(1, 30)]
        self.assertEqual(bias.correction(same_hour + other_hour, **self.KEY), (round(300 * 4 / 9), 4))
        # With fewer than three same-hour samples, all hours are used.
        self.assertEqual(bias.correction(same_hour[:2] + other_hour, **self.KEY)[1], 31)

    def test_only_this_station_lead_variable_and_window(self):
        rows = [sample(T - 86400, 900, station="st_bbbbbbbbbbbb"), sample(T - 86400, 900, lead=24),
                sample(T - 86400, 900, variable="pressure_hpa"), sample(T - 40 * 86400, 900), sample(T + 3600, 900)]
        self.assertEqual(bias.correction(rows, **self.KEY), (0, 0))

    def test_clamped(self):
        rows = [sample(T - 3600 * h, 99999) for h in range(1, 200)]
        self.assertEqual(bias.correction(rows, **self.KEY)[0], bias.MAX_CORRECTION["temperature_c"])


class FakeModel:
    """Model says 10.00 degC and 950.00 hPa everywhere."""
    def __init__(self):
        self.calls = []

    def at_time(self, issue_time, valid_time, lat, lon):
        self.calls.append((issue_time, valid_time, lat, lon))
        return {"temperature_c": 1000, "pressure_hpa": 95000, "cycle": "20261109T12Z"}


class FakeSite:
    origin = "https://wrfcoin.example"

    def __init__(self, now, open_stations, station_id_by_alias):
        self.now, self.open, self.ids = now, open_stations, station_id_by_alias
        self.logged_in = False
        self.logins = 0
        self.submitted = []
        self.refuse_station = None

    def forecast_status(self):
        return {"enabled": True, "leads": [1, 6, 12, 24], "variables": ["temperature_c", "pressure_hpa"], "stations": self.open}

    def stations(self):
        return [{"alias": a, "latitude": 44.123456, "longitude": -93.654321, "status": "live"} for a in self.ids]

    def observations(self, alias):
        # The station reads 1.50 degC warmer than the model, every 10 minutes for the last 3 hours.
        return [{"observedAt": self.now - self.now % 600 - 600 * k, "temperatureC": 11.5, "pressureHpa": 940.0} for k in range(18)]

    def login(self, wallet_, chain_id, network_id):
        self.logins += 1
        self.logged_in = True

    def draft(self, alias, variable, lead, value):
        if alias == self.refuse_station:
            raise SiteError(403, "STATION_NOT_OPEN", "not open")
        station = self.ids[alias]
        data = chain.build_data(station, variable, value, lead, self.now, "website-manual/v1")
        return {"forecaster": ADDRESS, "stationAlias": alias, "stationId": station, "data": data, "nonce": "web-" + "0" * 32,
                "timestamp": self.now, "commitId": chain.commit_id(data, "web-" + "0" * 32, station, self.now)}

    def submit(self, body):
        self.submitted.append(body)
        return {"commitId": body["commitId"], "status": "committed"}


class CycleTest(unittest.TestCase):
    NOW = int(datetime(2026, 11, 9, 22, 50, tzinfo=timezone.utc).timestamp())

    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.wallet = wallet.Wallet(PHRASE)
        self.ids = {chain.station_alias("PWS-A"): "PWS-A", chain.station_alias("PWS-B"): "PWS-B"}
        self.alias_a = chain.station_alias("PWS-A")
        self.cfg = Config(data_dir=self.folder.name)

    def run_once(self, site, dry_run, store=None):
        store = store or Store(self.folder.name)
        return run_cycle(self.cfg, site, FakeModel(), store, self.wallet, dry_run=dry_run, now=self.NOW), store

    def test_dry_run_learns_forecasts_and_sends_nothing(self):
        site = FakeSite(self.NOW, [self.alias_a], self.ids)
        report, store = self.run_once(site, dry_run=True)
        self.assertEqual(report.stations, [self.alias_a])
        self.assertEqual(site.logins, 0)
        self.assertEqual(site.submitted, [])
        self.assertEqual(store.sent, [])
        self.assertGreater(report.new_samples, 0)
        self.assertEqual([f["lead"] for f in report.forecasts], [1, 6, 12, 24])
        first = report.forecasts[0]
        # Three past hours each say the station is 1.50 degC warmer: 150 * 3 / (3 + 5) = 56.
        self.assertEqual((first["model_raw"], first["correction"], first["samples"], first["forecast"]), (1000, 56, 3, 1056))
        self.assertEqual(first["value"], 10.6)
        body = first["submit_body"]
        message = chain.commit_auth_message("888", "testnet", self.alias_a, body["commitId"])
        self.assertTrue(ed25519.verify(self.wallet.public_key, message, bytes.fromhex(body["authSignature"])))

    def test_live_cycle_submits_once_per_station_lead_and_day(self):
        site = FakeSite(self.NOW, [self.alias_a], self.ids)
        report, store = self.run_once(site, dry_run=False)
        self.assertEqual(site.logins, 1)
        self.assertEqual(len(site.submitted), 4)
        self.assertEqual(len(report.forecasts), 4)
        for body in site.submitted:
            data = json.loads(body["data"])
            self.assertEqual(data["station_id"], "PWS-A")
            self.assertEqual(chain.commit_id(body["data"], body["nonce"], "PWS-A", body["timestamp"]), body["commitId"])
            message = chain.commit_auth_message("888", "testnet", "PWS-A", body["commitId"])
            self.assertTrue(ed25519.verify(self.wallet.public_key, message, bytes.fromhex(body["authSignature"])))
        again, _ = self.run_once(site, dry_run=False, store=Store(self.folder.name))
        self.assertEqual(len(site.submitted), 4)
        self.assertEqual(again.forecasts, [])

    def test_at_most_four_forecasts_a_cycle_and_closed_stations_are_skipped(self):
        site = FakeSite(self.NOW, sorted(self.ids), self.ids)
        report, _ = self.run_once(site, dry_run=False)
        self.assertEqual(len(site.submitted), 4)
        site2 = FakeSite(self.NOW, sorted(self.ids), self.ids)
        site2.refuse_station = sorted(self.ids)[0]
        with tempfile.TemporaryDirectory() as other:
            self.cfg = Config(data_dir=other)
            run_cycle(self.cfg, site2, FakeModel(), Store(other), self.wallet, dry_run=False, now=self.NOW)
        self.assertEqual({json.loads(b["data"])["station_id"] for b in site2.submitted}, {self.ids[sorted(self.ids)[1]]})

    def test_a_draft_for_another_value_is_never_signed(self):
        site = FakeSite(self.NOW, [self.alias_a], self.ids)
        honest = site.draft
        site.draft = lambda alias, variable, lead, value: honest(alias, variable, lead, value + 5)
        report, store = self.run_once(site, dry_run=False)
        self.assertEqual(site.submitted, [])
        self.assertTrue(any("Nothing was signed" in line for line in report.lines))

    def test_silent_station_gets_no_forecast(self):
        site = FakeSite(self.NOW, [self.alias_a], self.ids)
        site.observations = lambda alias: []
        report, _ = self.run_once(site, dry_run=True)
        self.assertEqual(report.forecasts, [])
        self.assertTrue(any("cannot be scored" in line for line in report.lines))

    def test_nothing_finer_than_a_tenth_of_a_degree_is_used_or_stored(self):
        site = FakeSite(self.NOW, [self.alias_a], self.ids)
        model = FakeModel()
        store = Store(self.folder.name)
        report = run_cycle(self.cfg, site, model, store, self.wallet, dry_run=True, now=self.NOW)
        self.assertTrue(model.calls)
        for _, _, lat, lon in model.calls:
            self.assertEqual((lat, lon), (44.1, -93.7))
        text = (Path(self.folder.name) / "state.json").read_text() + json.dumps(report.forecasts) + "\n".join(report.lines)
        for fragment in ("44.1", "93.6", "93.7", "latitude", "longitude"):
            self.assertNotIn(fragment, text)


class ConfigTest(unittest.TestCase):
    def test_defaults_and_refusals(self):
        cfg = Config.from_env({})
        self.assertEqual((cfg.api_base, cfg.chain_id, cfg.stations, cfg.leads), ("https://wrfcoin.com/api", "888", "auto", (1, 6, 12, 24)))
        for bad in ({"WORKER_VARIABLE": "humidity_pct"}, {"WORKER_LEADS": "3"}, {"WORKER_CYCLE_SECONDS": "60"},
                    {"WORKER_STATIONS": "PWS-RAW-ID"}, {"WRFCOIN_API": "http://wrfcoin.com/api"}):
            with self.assertRaises(ValueError):
                Config.from_env(bad)


class SiteClientTest(unittest.TestCase):
    def test_login_signs_only_a_checked_challenge_and_keeps_the_token_private(self):
        w = wallet.Wallet(PHRASE)
        seen = []
        now = LoginChallengeTest.NOW

        def transport(method, url, headers, body):
            seen.append((method, url, dict(headers), body))
            if "/auth/nonce/" in url:
                return 200, json.dumps({"success": True, "data": {"nonce": "abcdefgh12345678", "scheme": "ed25519",
                                                                    "message": login_message()}}).encode()
            if url.endswith("/auth/wallet-login"):
                sent = json.loads(body)
                assert ed25519.verify(w.public_key, login_message().encode(), bytes.fromhex(sent["signature"]))
                return 200, json.dumps({"success": True, "data": {"jwt": "token-value", "expiresIn": 900}}).encode()
            if url.endswith("/forecasts/mine"):
                return 200, json.dumps({"forecasts": []}).encode()
            return 404, b"{}"

        site = Site("https://wrfcoin.com/api", transport=transport, sleep=lambda s: None, clock=lambda: now)
        site.login(w, "888", "testnet")
        self.assertEqual(site.mine(), [])
        self.assertEqual(seen[-1][2]["Authorization"], "Bearer token-value")
        self.assertNotIn("token-value", repr(site.__dict__.keys()))
        evil = Site("https://wrfcoin.com/api", sleep=lambda s: None, clock=lambda: now, transport=lambda m, u, h, b: (
            200, json.dumps({"success": True, "data": {"nonce": "abcdefgh12345678", "scheme": "ed25519",
                                                         "message": login_message(host="evil.example")}}).encode()))
        with self.assertRaises(chain.LoginChallengeRefused):
            evil.login(w, "888", "testnet")
        self.assertFalse(evil.logged_in)

    def test_switched_off_site_and_error_codes(self):
        off = Site("https://wrfcoin.com/api", transport=lambda m, u, h, b: (404, b'{"error":"Not found"}'), sleep=lambda s: None)
        self.assertIsNone(off.forecast_status())
        busy = Site("https://wrfcoin.com/api", sleep=lambda s: None, transport=lambda m, u, h, b: (
            409, b'{"success":false,"error":{"code":"ALREADY_FORECAST","message":"x"}}'))
        busy._token = "t"
        with self.assertRaises(SiteError) as caught:
            busy.draft("st_aaaaaaaaaaaa", "temperature_c", 6, 1.0)
        self.assertEqual(caught.exception.code, "ALREADY_FORECAST")


if __name__ == "__main__":
    unittest.main()
