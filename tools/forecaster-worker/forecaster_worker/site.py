"""The wrfcoin.com public API, as a headless wallet uses it.

Reads (no login): station list, a station's recent readings, forecast status.
Writes (wallet login): forecast draft and signed submit; the site relays the commit and reveals it.
The login token lives in memory only and is never printed. Redirects are never followed, so the
token cannot be carried to another host. Every answer is shape-checked before use.
"""
import json
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit

from . import chain

USER_AGENT = "wrfcoin-forecaster-worker/0"
MAX_RETRY_AFTER_SECONDS = 6 * 3600


class SiteError(Exception):
    def __init__(self, status, code=None, message=None, retry_after=None):
        super().__init__(f"HTTP {status}" + (f" {code}" if code else "") + (f": {message}" if message else ""))
        self.status, self.code, self.message, self.retry_after = status, code, message, retry_after

    @property
    def back_off(self) -> bool:
        """The site is busy, broken or unreachable: stop the cycle instead of trying the next thing."""
        return self.status is None or self.status == 429 or self.status >= 500


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


_OPENER = urllib.request.build_opener(_NoRedirect)


def default_transport(method: str, url: str, headers: dict, body: bytes | None):
    """Returns (status, body, retry_after_text). A redirect comes back as its 3xx status, unfollowed."""
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with _OPENER.open(request, timeout=30) as response:
            return response.status, response.read(2_000_000), response.headers.get("Retry-After")
    except urllib.error.HTTPError as error:
        with error:
            return error.code, error.read(200_000), error.headers.get("Retry-After") if error.headers else None
    except (urllib.error.URLError, OSError, TimeoutError, ValueError):
        return None, b"", None


def _retry_seconds(text) -> int | None:
    try:
        seconds = int(str(text).strip())
    except (TypeError, ValueError):
        return None
    return min(seconds, MAX_RETRY_AFTER_SECONDS) if seconds > 0 else None


def _dict(value) -> dict:
    return value if isinstance(value, dict) else {}


def _list(value) -> list:
    return value if isinstance(value, list) else []


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
        self.more_stations = False

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
        answer = self._transport(method, self.api_base + path, headers, raw)
        self._last = self._clock()
        status, payload = answer[0], answer[1]
        retry_after = _retry_seconds(answer[2]) if len(answer) > 2 else None
        if status is None:
            raise SiteError(None, "UNREACHABLE", "the site could not be reached")
        try:
            parsed = json.loads(payload.decode("utf-8")) if payload else None
        except (ValueError, AttributeError):
            parsed = None
        if 200 <= status < 300:
            if isinstance(parsed, dict) and "success" in parsed and "data" in parsed:
                return parsed["data"]
            return parsed
        if 300 <= status < 400:
            raise SiteError(status, "REDIRECT", "the site answered with a redirect, which the worker does not follow")
        error = _dict(_dict(parsed).get("error"))
        code, message = error.get("code"), error.get("message")
        raise SiteError(status, code if isinstance(code, str) else None, message if isinstance(message, str) else None,
                        retry_after)

    # Public reads
    def forecast_status(self) -> dict | None:
        """None when website forecasts are switched off (the site answers 404)."""
        try:
            data = self._call("GET", "/forecasts/status")
        except SiteError as error:
            if error.status == 404:
                return None
            raise
        return data if isinstance(data, dict) else None

    def stations(self) -> list:
        """The first page of the public station list. The public edge takes no paging query, so when the
        site says there are more, `more_stations` is set and the caller says so."""
        data = _dict(self._call("GET", "/weather/stations"))
        self.more_stations = data.get("hasMore") is True
        return [s for s in _list(data.get("stations"))
                if isinstance(s, dict) and isinstance(s.get("alias"), str) and chain.ALIAS_RE.match(s["alias"])]

    def observations(self, alias: str) -> list:
        data = _dict(self._call("GET", f"/stations/{alias}/observations"))
        return [o for o in _list(data.get("observations")) if isinstance(o, dict)]

    # Wallet calls
    def login(self, wallet, chain_id: str, network_id: str) -> None:
        challenge = _dict(self._call("GET", f"/auth/nonce/{wallet.address}?scheme=ed25519"))
        if challenge.get("scheme") != "ed25519":
            raise chain.LoginChallengeRefused("the site did not send an ed25519 challenge")
        nonce = challenge.get("nonce")
        message = chain.check_login_challenge(
            challenge.get("message"), address=wallet.address, origin=self.origin, chain_id=chain_id,
            network_id=network_id, nonce=nonce if isinstance(nonce, str) else "", now=self._clock())
        result = _dict(self._call("POST", "/auth/wallet-login", {
            "scheme": "ed25519", "address": wallet.address, "pubkey": wallet.public_key.hex(),
            "signature": wallet.sign(message).hex(), "nonce": nonce,
        }))
        token = result.get("jwt")
        if not isinstance(token, str) or not token or any(c in token for c in "\r\n "):
            raise SiteError(502, "NO_TOKEN", "the login answer held no token")
        self._token = token

    @property
    def logged_in(self) -> bool:
        return self._token is not None

    def logout(self) -> None:
        """Forget the login token. Tokens are short-lived, so each cycle logs in afresh when it has work."""
        self._token = None

    def draft(self, alias: str, variable: str, lead_hours: int, value: float, model_id: str | None = None) -> dict:
        body = {"stationAlias": alias, "variable": variable, "leadHours": lead_hours, "value": value}
        if model_id is not None:
            body["modelId"] = model_id   # a site that does not know modelId yet ignores it
        return _dict(self._call("POST", "/forecasts/draft", body, auth=True))

    def submit(self, body: dict) -> dict:
        return _dict(self._call("POST", "/forecasts/submit", body, auth=True))

    def mine(self) -> list:
        data = _dict(self._call("GET", "/forecasts/mine", auth=True))
        return [row for row in _list(data.get("forecasts")) if isinstance(row, dict)]
