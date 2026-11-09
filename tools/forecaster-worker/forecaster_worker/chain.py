"""The byte rules a forecast commit must follow. Each one mirrors existing code:

- commit id: core4 `Transaction::compute_commit_id` (tools/forecast-kit `compute_commit_id`)
- commit signature message: core4 `commit_authorization_message_with_domain`
  (frontend walletCrypto/forecastCommit.ts `forecastCommitMessage`)
- canonical data text and the draft shape: backend services/website-forecast/website-forecast.ts
- station alias: backend utils/station-alias.js
- login challenge: backend routes/wallet-auth-service.ts `buildEd25519WalletLoginMessage`
"""
import hashlib
import json
import re
import struct
from datetime import datetime, timezone

COMMIT_AUTH_DOMAIN = "wrfcoin_commit_auth_v2"
CONTRIBUTION_TYPE = "forecast"
LOGIN_DOMAIN = "wrfcoin_wallet_login_ed25519_v1"
LEADS = (1, 6, 12, 24)
VARIABLES = ("temperature_c", "pressure_hpa")
# The same plausible ranges the site applies (station pressure, not sea level).
VALUE_RANGES = {"temperature_c": (-60.0, 60.0), "pressure_hpa": (500.0, 1100.0)}
ALIAS_RE = re.compile(r"^st_[0-9a-f]{12}$")
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
ADDRESS_RE = re.compile(r"^0x[0-9a-f]{40}$")
_ISO_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z$")


def canonical_json(value) -> str:
    """Compact JSON with sorted keys: what serde_json writes for a `Value`."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def commit_id(data: str, nonce: str, station_id: str, timestamp: int) -> str:
    return hashlib.sha3_256(f"{data}||{nonce}||{station_id}||{timestamp}||{CONTRIBUTION_TYPE}".encode()).hexdigest()


def _framed(text: str) -> bytes:
    raw = text.encode()
    return struct.pack("<I", len(raw)) + raw


def commit_auth_message(chain_id: str, network_id: str, station_id: str, commit: str) -> bytes:
    return (COMMIT_AUTH_DOMAIN.encode() + _framed(chain_id) + _framed(network_id) + _framed(station_id)
            + _framed(commit) + _framed(CONTRIBUTION_TYPE))


def station_alias(station_id: str) -> str:
    return "st_" + hashlib.sha256(f"wrfcoin-station-alias-v1:{station_id}".encode()).hexdigest()[:12]


def round_tenth(value: float) -> float:
    """One decimal, the precision the site relay takes. Integers in, so no float surprises."""
    return round(value * 10) / 10


def site_value(variable: str, scaled: int) -> float:
    """Chain integer units (0.01 degC, Pa) to the site's one-decimal degC or hPa."""
    tenths = _round_half_away(scaled, 10)  # 0.01 degC -> 0.1 degC, Pa -> 0.1 hPa: both divide by 10
    return tenths / 10


def _round_half_away(value: int, divisor: int) -> int:
    q, r = divmod(abs(value), divisor)
    if 2 * r >= divisor:
        q += 1
    return q if value >= 0 else -q


def build_data(station_id: str, variable: str, value: float, lead_hours: int, issued_at: int, model_id: str) -> str:
    number = int(value) if float(value).is_integer() else value
    return canonical_json({
        "horizon_hours": lead_hours, "issued_at": issued_at, "model_id": model_id, "station_id": station_id,
        "valid_at": issued_at + lead_hours * 3600, "values": {variable: number},
    })


class DraftMismatch(Exception):
    """The site's draft is not the forecast this worker asked for. Nothing is signed."""


def verify_draft(draft: dict, *, wallet: str, alias: str, variable: str, lead_hours: int, value: float) -> str:
    """Check a draft before signing it. Returns the station id to sign over."""
    def fail(why):
        raise DraftMismatch(why)

    if not isinstance(draft, dict):
        fail("no draft")
    data, nonce, timestamp, commit = draft.get("data"), draft.get("nonce"), draft.get("timestamp"), draft.get("commitId")
    if not isinstance(data, str) or not isinstance(nonce, str) or not isinstance(timestamp, int) or isinstance(timestamp, bool):
        fail("draft is incomplete")
    if not isinstance(commit, str) or not HEX64_RE.match(commit):
        fail("commit id is not 64 hex characters")
    try:
        parsed = json.loads(data)
    except ValueError:
        fail("data is not JSON")
    if not isinstance(parsed, dict):
        fail("data is not an object")
    station_id = parsed.get("station_id")
    if not isinstance(station_id, str) or not station_id:
        fail("data names no station")
    if station_id != draft.get("stationId"):
        fail("station id differs from the draft")
    if station_alias(station_id) != alias:
        fail("the draft is for a different station")
    if parsed.get("horizon_hours") != lead_hours:
        fail("the draft has a different lead time")
    values = parsed.get("values")
    if not isinstance(values, dict) or list(values) != [variable]:
        fail("the draft forecasts a different variable")
    got = values[variable]
    if isinstance(got, bool) or not isinstance(got, (int, float)) or float(got) != round_tenth(value):
        fail("the draft value differs from the forecast")
    if parsed.get("issued_at") != timestamp:
        fail("the commit time differs from the forecast time")
    if parsed.get("valid_at") != timestamp + lead_hours * 3600:
        fail("the target time does not match the lead")
    if str(draft.get("forecaster", "")).lower() != wallet.lower() or not ADDRESS_RE.match(wallet.lower()):
        fail("the draft names a different wallet")
    if commit_id(data, nonce, station_id, timestamp) != commit:
        fail("the commit id does not match the forecast")
    return station_id


def sign_commit(draft: dict, station_id: str, chain_id: str, network_id: str, wallet) -> dict:
    """The body of POST /api/forecasts/submit."""
    message = commit_auth_message(chain_id, network_id, station_id, draft["commitId"])
    return {
        "data": draft["data"], "nonce": draft["nonce"], "timestamp": draft["timestamp"], "commitId": draft["commitId"],
        "authPubkey": wallet.public_key.hex(), "authSignature": wallet.sign(message).hex(),
    }


class LoginChallengeRefused(Exception):
    """The login challenge is not exactly a login for this wallet, site and chain. Nothing is signed."""


def check_login_challenge(message, *, address: str, origin: str, chain_id: str, network_id: str, nonce: str,
                          now: float) -> bytes:
    """Return the bytes to sign, only if `message` is line for line the expected login challenge.

    The wallet key also moves funds, so the worker never signs server text it has not checked.
    """
    def refuse(why):
        raise LoginChallengeRefused(why)

    if not isinstance(message, str):
        refuse("challenge is not text")
    host = re.sub(r"^https?://", "", origin)
    if not re.match(r"^https?://[A-Za-z0-9.-]+(:\d+)?$", origin):
        refuse("the configured site origin is not a plain http(s) origin")
    lines = message.split("\n")
    if len(lines) != 14:
        refuse("challenge does not have the expected shape")
    expected = [
        LOGIN_DOMAIN,
        f"{host} wants you to sign in with your WRFCoin ed25519 account:",
        address.lower(), "", "Sign in to WRFCoin Testnet.", "",
        f"URI: {origin}", "Version: 1", "Scheme: ed25519", f"Chain ID: {chain_id}", f"Network: {network_id}",
        f"Nonce: {nonce}",
    ]
    if not re.match(r"^[A-Za-z0-9_-]{8,128}$", nonce or ""):
        refuse("challenge nonce is malformed")
    for index, want in enumerate(expected):
        if lines[index] != want:
            refuse(f"challenge line {index + 1} is not what a login for this wallet and site looks like")
    issued, expiry = lines[12].removeprefix("Issued At: "), lines[13].removeprefix("Expiration Time: ")
    if not lines[12].startswith("Issued At: ") or not lines[13].startswith("Expiration Time: ") \
            or not _ISO_RE.match(issued) or not _ISO_RE.match(expiry):
        refuse("challenge times are malformed")
    if _iso_seconds(expiry) <= max(now, _iso_seconds(issued)):
        refuse("challenge has expired")
    return message.encode()


def _iso_seconds(text: str) -> float:
    return datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc).timestamp()
