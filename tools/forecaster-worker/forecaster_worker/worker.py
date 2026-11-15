"""One worker cycle: read stations, learn each station's model error, forecast, submit."""
import os
import secrets
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from urllib.parse import urlsplit

from . import bias, chain, gfs
from .site import SiteError

# Worker forecasts carry their own model name (owner, 2026-11-10): the worker asks the site to write
# bias.MODEL_ID into the draft. A site from before that change ignores the request and writes its own
# label, so a draft with either name is accepted; any other name is never signed.
RELAY_MODEL_ID = bias.MODEL_ID
SITE_DEFAULT_MODEL_ID = "website-manual/v1"
ACCEPTED_DRAFT_MODEL_IDS = (RELAY_MODEL_ID, SITE_DEFAULT_MODEL_ID)
MAX_FORECASTS_PER_CYCLE = 4          # the site allows 10 draft/submit calls per wallet per 10 minutes
FRESH_READING_SECONDS = 3 * 3600     # a station silent for longer cannot score a forecast
BACKFILL_HOURS = 3
FORECAST_REQUEST_RESERVE = 2 * MAX_FORECASTS_PER_CYCLE   # model files kept back for the forecasts


@dataclass
class Config:
    api_base: str = "https://wrfcoin.com/api"
    chain_id: str = "888"
    network_id: str = "testnet"
    stations: str = "auto"               # "auto" = the stations the site has open, or a comma list of st_ aliases
    variable: str = "temperature_c"      # the site counts one forecast per station, lead and day
    leads: tuple = chain.LEADS
    cycle_seconds: int = 3600
    data_dir: str = "/data"

    @classmethod
    def from_env(cls, env=os.environ):
        cfg = cls()
        cfg.api_base = env.get("WRFCOIN_API", cfg.api_base).rstrip("/")
        cfg.chain_id = env.get("WRFCOIN_CHAIN_ID", cfg.chain_id)
        cfg.network_id = env.get("WRFCOIN_NETWORK_ID", cfg.network_id)
        cfg.stations = env.get("WORKER_STATIONS", cfg.stations).strip() or "auto"
        cfg.variable = env.get("WORKER_VARIABLE", cfg.variable)
        cfg.leads = tuple(int(x) for x in env.get("WORKER_LEADS", "1,6,12,24").split(",") if x.strip())
        cfg.cycle_seconds = int(env.get("WORKER_CYCLE_SECONDS", cfg.cycle_seconds))
        cfg.data_dir = env.get("WORKER_DATA_DIR", cfg.data_dir)
        cfg.check()
        return cfg

    def check(self):
        if self.variable not in chain.VARIABLES:
            raise ValueError(f"WORKER_VARIABLE must be one of {', '.join(chain.VARIABLES)}")
        if not self.leads or any(lead not in chain.LEADS for lead in self.leads):
            raise ValueError("WORKER_LEADS must be a comma list from 1,6,12,24")
        if self.cycle_seconds < 900:
            raise ValueError("WORKER_CYCLE_SECONDS must be at least 900 (15 minutes), to stay polite to the data sources")
        if self.stations != "auto" and any(not chain.ALIAS_RE.match(a.strip()) for a in self.stations.split(",")):
            raise ValueError("WORKER_STATIONS must be 'auto' or a comma list of station aliases like st_0123456789ab")
        parts = urlsplit(self.api_base)
        local = parts.scheme == "http" and parts.hostname in ("localhost", "127.0.0.1")
        if not ((parts.scheme == "https" or local) and parts.hostname and not parts.username and not parts.query):
            raise ValueError("WRFCOIN_API must be an https URL (plain http is allowed only for localhost)")


@dataclass
class CycleReport:
    lines: list = field(default_factory=list)
    stations: list = field(default_factory=list)
    new_readings: int = 0
    new_samples: int = 0
    forecasts: list = field(default_factory=list)   # what was sent, or would be sent in a dry run

    def say(self, text: str):
        self.lines.append(text)


def utc_day(seconds: int) -> str:
    return datetime.fromtimestamp(seconds, timezone.utc).strftime("%Y-%m-%d")


def _coordinate(row: dict, *names):
    for name in names:
        value = row.get(name)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            # The site publishes one decimal. Round anyway, so nothing finer can ever be used.
            return round(float(value), 1)
    return None


def choose_stations(cfg: Config, status, listed: list, report: CycleReport, dry_run: bool) -> dict:
    """alias -> (lat, lon) at 0.1 degree, for the stations this worker forecasts."""
    positions = {}
    for row in listed:
        lat, lon = _coordinate(row, "latitude", "lat"), _coordinate(row, "longitude", "lon", "lng")
        if lat is not None and lon is not None and -90 <= lat <= 90 and -180 <= lon <= 360:
            positions[row["alias"]] = (lat, lon)
    if cfg.stations != "auto":
        wanted = [a.strip() for a in cfg.stations.split(",")]
    elif status and isinstance(status.get("stations"), list):
        wanted = [a for a in status["stations"] if isinstance(a, str)]
    else:
        wanted = sorted(positions) if dry_run else []
        if dry_run:
            report.say("The site does not list open stations; a dry run uses every listed station.")
    chosen = {}
    for alias in wanted:
        if alias in positions:
            chosen[alias] = positions[alias]
        else:
            report.say(f"{alias}: not in the public station list, skipped.")
    return chosen


def learn(store, model, alias: str, position, cfg: Config, now: int, report: CycleReport) -> bool:
    """Add bias samples for recent whole hours that have a station reading. False = model budget used up."""
    last_hour = now - now % 3600
    for hour in range(last_hour - (BACKFILL_HOURS - 1) * 3600, last_hour + 1, 3600):
        for variable in chain.VARIABLES:
            reading = store.reading_near(alias, variable, hour)
            if reading is None:
                continue
            for lead in cfg.leads:
                if store.has_sample(alias, variable, lead, hour):
                    continue
                try:
                    value = model.at_time(hour - lead * 3600, hour, *position)[variable]
                except gfs.GfsBudgetSpent:
                    return False
                except gfs.GfsUnavailable as error:
                    report.say(f"{alias}: no model value for a past hour ({error}).")
                    return True
                store.samples.append({"station": alias, "variable": variable, "lead": lead, "valid": hour,
                                      "model": value, "obs": reading})
                report.new_samples += 1
    return True


def make_forecast(store, model, alias: str, position, cfg: Config, lead: int, now: int) -> dict:
    valid = now + lead * 3600
    raw = model.at_time(now, valid, *position)
    forecast, delta, used = bias.corrected(raw[cfg.variable], store.samples, station=alias, variable=cfg.variable,
                                            lead_hours=lead, valid_time=valid)
    value = chain.site_value(cfg.variable, forecast)
    low, high = chain.VALUE_RANGES[cfg.variable]
    if not low <= value <= high:
        raise gfs.GfsUnavailable("the forecast value is outside the plausible range")
    return {"station": alias, "variable": cfg.variable, "lead": lead, "model_run": raw["cycle"],
            "model_raw": raw[cfg.variable], "correction": delta, "samples": used, "forecast": forecast, "value": value}


def local_draft(forecast: dict, wallet, now: int) -> dict:
    """A draft built here with the site's own rules, for a dry run. The real station id is known only
    to the site, so the public alias stands in for it; a real draft carries the real id."""
    station_id = forecast["station"]
    data = chain.build_data(station_id, forecast["variable"], forecast["value"], forecast["lead"], now, RELAY_MODEL_ID)
    nonce = "dry-" + secrets.token_hex(16)
    return {"forecaster": wallet.address, "stationAlias": forecast["station"], "stationId": station_id,
            "data": data, "nonce": nonce, "timestamp": now, "commitId": chain.commit_id(data, nonce, station_id, now)}


def unsigned_body(draft: dict, wallet) -> dict:
    """What a dry run prints: the submit body without a signature. A dry run signs nothing."""
    return {"data": draft["data"], "nonce": draft["nonce"], "timestamp": draft["timestamp"], "commitId": draft["commitId"],
            "authPubkey": wallet.public_key.hex(), "authSignature": None}


def run_cycle(cfg: Config, site, model, store, wallet, *, dry_run: bool, now: int | None = None,
              clock=None) -> CycleReport:
    """One cycle. State is saved after every accepted forecast and again at the end, whatever happens.
    A busy, broken or unreachable site (429, 5xx, no answer) raises SiteError: the caller waits a full interval."""
    clock = clock or ((lambda: now) if now is not None else time.time)   # tests pin the time; real runs use the clock
    now = int(time.time()) if now is None else now
    report = CycleReport()
    try:
        _cycle(cfg, site, model, store, wallet, dry_run, now, report, clock)
    finally:
        store.save(now)
    return report


def _cycle(cfg, site, model, store, wallet, dry_run, now, report, clock) -> None:
    site.logout()   # a token from an earlier cycle has expired by now
    if getattr(store, "recovered_from", None):
        report.say(f"The history file was damaged. It was kept as {store.recovered_from} and a new one was started.")
    status = site.forecast_status()
    if status is None:
        report.say("Website forecasts are switched off on the site right now.")
    stations = choose_stations(cfg, status, site.stations(), report, dry_run)
    if getattr(site, "more_stations", False):
        report.say("The site has more stations than its first page lists; only the first page is used.")
    report.stations = sorted(stations)
    if not stations:
        report.say("No station to forecast this cycle.")

    for alias in stations:
        try:
            report.new_readings += store.add_observations(alias, site.observations(alias))
        except SiteError as error:
            if error.back_off:
                raise
            report.say(f"{alias}: could not read the station's readings ({error}).")

    # Learn first, so today's forecasts use today's samples, but keep enough model requests back for
    # the forecasts: they are the point of the cycle. The start station rotates so none is always last.
    limit = getattr(model, "max_requests", None)
    if limit is not None:
        model.max_requests = max(0, limit - FORECAST_REQUEST_RESERVE)
    try:
        order = sorted(stations)
        turn = getattr(store, "learn_turn", 0)
        start = turn % len(order) if order else 0
        store.learn_turn = turn + 1     # saved with the state, so the start station moves on every cycle
        for index, alias in enumerate(order[start:] + order[:start]):
            if not learn(store, model, alias, stations[alias], cfg, now, report):
                waiting = len(order) - index
                report.say(f"Model request budget for learning is used up this cycle; {waiting} station(s) learn next cycle.")
                break
    finally:
        if limit is not None:
            model.max_requests = limit
    _send_forecasts(cfg, site, model, store, wallet, dry_run, now, report, stations, clock)


def _send_forecasts(cfg, site, model, store, wallet, dry_run, now, report, stations, clock) -> None:
    day = utc_day(now)
    sent_this_cycle = 0
    for alias, position in stations.items():
        newest = store.newest_reading(alias)
        if newest is None or now - newest > FRESH_READING_SECONDS:
            report.say(f"{alias}: no reading in the last {FRESH_READING_SECONDS // 3600} hours. "
                       "A forecast for a silent station cannot be scored, so none is sent.")
            continue
        for lead in cfg.leads:
            if store.already_sent(alias, lead, day):
                continue
            if sent_this_cycle >= MAX_FORECASTS_PER_CYCLE:
                report.say("Sent the most forecasts allowed in one cycle; the rest go next cycle.")
                return
            try:
                forecast = make_forecast(store, model, alias, position, cfg, lead, now)
            except gfs.GfsUnavailable as error:
                report.say(f"{alias} +{lead}h: no forecast ({error}).")
                continue
            if dry_run:
                draft = local_draft(forecast, wallet, now)
                chain.verify_draft(draft, wallet=wallet.address, alias=chain.station_alias(draft["stationId"]),
                                   variable=cfg.variable, lead_hours=lead, value=forecast["value"], now=now,
                                   model_id=RELAY_MODEL_ID)
                report.forecasts.append(dict(forecast, dry_run=True, submit_body=unsigned_body(draft, wallet)))
                sent_this_cycle += 1
                continue
            try:
                if not site.logged_in:
                    site.login(wallet, cfg.chain_id, cfg.network_id)
                draft = site.draft(alias, cfg.variable, lead, forecast["value"], model_id=RELAY_MODEL_ID)
                station_id = chain.verify_draft(draft, wallet=wallet.address, alias=alias, variable=cfg.variable,
                                                lead_hours=lead, value=forecast["value"], now=int(clock()),
                                                model_id=ACCEPTED_DRAFT_MODEL_IDS)
                answer = site.submit(chain.sign_commit(draft, station_id, cfg.chain_id, cfg.network_id, wallet))
            except SiteError as error:
                if error.code == "ALREADY_FORECAST":
                    store.sent.append({"station": alias, "lead": lead, "day": day, "variable": cfg.variable,
                                       "commit_id": None, "value": None, "issued_at": now})
                    store.save(now)
                    report.say(f"{alias} +{lead}h: this wallet already forecast it today.")
                    continue
                if error.code == "STATION_NOT_OPEN":
                    report.say(f"{alias}: the site does not take forecasts for this station yet.")
                    break
                if error.back_off:
                    raise
                report.say(f"{alias} +{lead}h: the site refused ({error}). No more forecasts this cycle.")
                return
            except (chain.DraftMismatch, chain.LoginChallengeRefused) as error:
                report.say(f"Nothing was signed: {error}. No more forecasts this cycle.")
                return
            sent_this_cycle += 1
            commit = answer.get("commitId") if isinstance(answer.get("commitId"), str) else draft["commitId"]
            store.sent.append({"station": alias, "lead": lead, "day": day, "variable": cfg.variable,
                               "commit_id": commit, "value": forecast["value"], "issued_at": now})
            store.save(now)     # at once: a crash later in the cycle must not forget a sent forecast
            report.forecasts.append(dict(forecast, dry_run=False, commit_id=commit))
