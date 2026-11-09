"""The wrfcoin.com public API, as a headless wallet uses it.

Reads (no login): station list, a station's recent readings, forecast status.
Writes (wallet login): forecast draft and signed submit; the site relays the commit and reveals it.
The login token lives in memory only and is never printed.
"""
import json
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit

from . import chain

USER_AGENT = "wrfcoin-forecaster-worker/0"


class SiteError(Exception):
    def __init__(self, status, code=None, message=None):
        super().__init__(f"HTTP {status}" + (f" {code}" if code else "") + (f": {message}" if message else ""))
        self.status, self.code, self.message = status, code, message


def default_transport(method: str, url: str, headers: dict, body: bytes | None):
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, response.read(2_000_000)
    except urllib.error.HTTPError as error:
        return error.code, error.read(200_000)
    except (urllib.error.URLError, OSError, TimeoutError):
        return None, b""


class Site:
    def __init__(self, api_base: str, transport=default_transport, sleep=time.sleep, clock=time.time,
                 min_interval: float = 2.0):
        self.api_base = api_base.rstrip("/")
        parts = urlsplit(self.api_base)
        self.origin = f"{parts.scheme}://{parts.netloc}"
        self._transport, self._sleep, self._clock = transport, sleep, clock
        self._min_interval = min_interval
        self._last = None
        self._token = None
        self.requests = 0

    def _call(self, method: str, path: str, body=None, auth: bool = False):
        if self._last is not None:
            wait = self._min_interval - (self._clock() - self._last)
            if wait > 0:
                self._sleep(wait)
        headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
        raw = None
        if method == "POST":
            headers["Content-Type"] = "application/json"
            raw = json.dumps(body if body is not None else {}).encode()
        if auth:
            if not self._token:
                raise SiteError(401, "SIGNED_OUT", "not logged in")
            headers["Authorization"] = f"Bearer {self._token}"
        self.requests += 1
        status, payload = self._transport(method, self.api_base + path, headers, raw)
        self._last = self._clock()
        if status is None:
            raise SiteError(None, "UNREACHABLE", "the site could not be reached")
        try:
            parsed = json.loads(payload.decode("utf-8")) if payload else None
        except ValueError:
            parsed = None
        if 200 <= status < 300:
            if isinstance(parsed, dict) and "success" in parsed and "data" in parsed:
                return parsed["data"]
            return parsed
        error = parsed.get("error") if isinstance(parsed, dict) else None
        code = error.get("code") if isinstance(error, dict) else None
        message = error.get("message") if isinstance(error, dict) else None
        raise SiteError(status, code if isinstance(code, str) else None, message if isinstance(message, str) else None)

    # Public reads
    def forecast_status(self) -> dict | None:
        """None when website forecasts are switched off (the site answers 404)."""
        try:
            return self._call("GET", "/forecasts/status")
        except SiteError as error:
            if error.status == 404:
                return None
            raise

    def stations(self) -> list:
        data = self._call("GET", "/weather/stations") or {}
        return [s for s in data.get("stations", []) if isinstance(s, dict) and chain.ALIAS_RE.match(str(s.get("alias", "")))]

    def observations(self, alias: str) -> list:
        data = self._call("GET", f"/stations/{alias}/observations") or {}
        return [o for o in data.get("observations", []) if isinstance(o, dict)]

    # Wallet calls
    def login(self, wallet, chain_id: str, network_id: str) -> None:
        challenge = self._call("GET", f"/auth/nonce/{wallet.address}?scheme=ed25519") or {}
        if challenge.get("scheme") != "ed25519":
            raise chain.LoginChallengeRefused("the site did not send an ed25519 challenge")
        nonce = challenge.get("nonce")
        message = chain.check_login_challenge(
            challenge.get("message"), address=wallet.address, origin=self.origin, chain_id=chain_id,
            network_id=network_id, nonce=nonce if isinstance(nonce, str) else "", now=self._clock())
        result = self._call("POST", "/auth/wallet-login", {
            "scheme": "ed25519", "address": wallet.address, "pubkey": wallet.public_key.hex(),
            "signature": wallet.sign(message).hex(), "nonce": nonce,
        }) or {}
        token = result.get("jwt")
        if not isinstance(token, str) or not token:
            raise SiteError(502, "NO_TOKEN", "the login answer held no token")
        self._token = token

    @property
    def logged_in(self) -> bool:
        return self._token is not None

    def logout(self) -> None:
        """Forget the login token. Tokens are short-lived, so each cycle logs in afresh when it has work."""
        self._token = None

    def draft(self, alias: str, variable: str, lead_hours: int, value: float) -> dict:
        return self._call("POST", "/forecasts/draft",
                          {"stationAlias": alias, "variable": variable, "leadHours": lead_hours, "value": value}, auth=True)

    def submit(self, body: dict) -> dict:
        return self._call("POST", "/forecasts/submit", body, auth=True)

    def mine(self) -> list:
        data = self._call("GET", "/forecasts/mine", auth=True) or {}
        return data.get("forecasts", [])
