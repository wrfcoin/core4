"""NOAA GFS point values through the NOMADS grib filter.

Data credit: NOAA/NCEP Global Forecast System (GFS). US government work, public domain
(https://www.weather.gov/disclaimer). The worker asks the filter for a 1-degree box and two
fields, so one file is well under a kilobyte.
"""
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone

from . import grib2

ATTRIBUTION = ("Weather model data: NOAA/NCEP Global Forecast System (GFS), public domain. "
               "Not an official NOAA product; NOAA does not endorse this software.")
NOMADS_FILTER = "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25_1hr.pl"
USER_AGENT = "wrfcoin-forecaster-worker/0 (+https://wrfcoin.com)"
# A run is on NOMADS roughly 4 to 5 hours after its start time.
AVAILABILITY_LAG_HOURS = 5
MAX_HOURLY_STEP = 120
BOX_HALF_DEGREES = 0.5
MIN_SECONDS_BETWEEN_REQUESTS = 1.0


class GfsUnavailable(Exception):
    pass


def pick_cycle(issue_time: int, lag_hours: int = AVAILABILITY_LAG_HOURS) -> datetime:
    """The newest 6-hourly run that is at least `lag_hours` old at `issue_time` (unix seconds)."""
    t = datetime.fromtimestamp(issue_time, timezone.utc) - timedelta(hours=lag_hours)
    return t.replace(hour=t.hour - t.hour % 6, minute=0, second=0, microsecond=0)


def forecast_hour(cycle: datetime, valid_hour: int) -> int:
    """Whole hours from the run start to `valid_hour` (unix seconds on the hour)."""
    seconds = valid_hour - int(cycle.timestamp())
    if seconds % 3600 or not 0 <= seconds // 3600 <= MAX_HOURLY_STEP:
        raise GfsUnavailable("the valid time is outside this run's hourly steps")
    return seconds // 3600


def box_url(cycle: datetime, step: int, lat: float, lon: float) -> str:
    hh = f"{cycle.hour:02d}"
    query = [
        ("dir", f"/gfs.{cycle:%Y%m%d}/{hh}/atmos"), ("file", f"gfs.t{hh}z.pgrb2.0p25.f{step:03d}"),
        ("var_TMP", "on"), ("var_PRES", "on"), ("lev_2_m_above_ground", "on"), ("lev_surface", "on"),
        ("subregion", ""),
        ("toplat", f"{lat + BOX_HALF_DEGREES:.2f}"), ("leftlon", f"{lon - BOX_HALF_DEGREES:.2f}"),
        ("rightlon", f"{lon + BOX_HALF_DEGREES:.2f}"), ("bottomlat", f"{lat - BOX_HALF_DEGREES:.2f}"),
    ]
    return NOMADS_FILTER + "?" + urllib.parse.urlencode(query)


def point_from_grib(blob: bytes, lat: float, lon: float, cycle: datetime | None = None, step: int | None = None) -> dict:
    """Integer chain units at a point: {"temperature_c": centi-degC, "pressure_hpa": Pa}."""
    fields = grib2.read_fields(blob)
    tmp = grib2.pick(fields, category=0, number=0, level_type=103, level_value=2.0)   # 2 m temperature, K
    pres = grib2.pick(fields, category=3, number=0, level_type=1)                      # surface pressure, Pa
    for f in (tmp, pres):
        if cycle is not None and f.reference_time != (cycle.year, cycle.month, cycle.day, cycle.hour):
            raise grib2.GribError("the file is from a different model run")
        if step is not None and f.forecast_hours != step:
            raise grib2.GribError("the file is for a different forecast hour")
    kelvin, pascal = tmp.at(lat, lon), pres.at(lat, lon)
    if not (150.0 < kelvin < 350.0 and 30000.0 < pascal < 110000.0):
        raise grib2.GribError("model values are not plausible")
    return {"temperature_c": round((kelvin - 273.15) * 100), "pressure_hpa": round(pascal)}


class GfsClient:
    """Fetches point boxes politely: one at a time, spaced out, cached for the life of a cycle."""

    def __init__(self, opener=None, sleep=time.sleep, clock=time.monotonic, max_requests: int = 40):
        self._open = opener or self._default_open
        self._sleep, self._clock = sleep, clock
        self._last = None
        self._cache = {}
        self.requests = 0
        self.max_requests = max_requests

    @staticmethod
    def _default_open(url: str) -> bytes:
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=40) as response:
            return response.read(200_000)

    def point(self, cycle: datetime, step: int, lat: float, lon: float) -> dict:
        key = (cycle, step, round(lat, 1), round(lon, 1))
        if key in self._cache:
            return self._cache[key]
        if self.requests >= self.max_requests:
            raise GfsUnavailable("request budget for this cycle is used up")
        if self._last is not None:
            wait = MIN_SECONDS_BETWEEN_REQUESTS - (self._clock() - self._last)
            if wait > 0:
                self._sleep(wait)
        self.requests += 1
        try:
            blob = self._open(box_url(cycle, step, lat, lon))
        except (urllib.error.URLError, OSError, TimeoutError) as error:
            code = getattr(error, "code", None)
            raise GfsUnavailable(f"NOMADS did not return the file (HTTP {code})" if code else "NOMADS could not be reached") from None
        finally:
            self._last = self._clock()
        try:
            value = point_from_grib(blob, lat, lon, cycle, step)
        except grib2.GribError as error:
            raise GfsUnavailable(f"could not read the model file: {error}") from None
        self._cache[key] = value
        return value

    def at_time(self, issue_time: int, valid_time: int, lat: float, lon: float) -> dict:
        """Model values at `valid_time`, from the run a worker would have had at `issue_time`.

        Linear in time between the two hourly steps around `valid_time`.
        """
        cycle = pick_cycle(issue_time)
        lower = valid_time - valid_time % 3600
        first = self.point(cycle, forecast_hour(cycle, lower), lat, lon)
        if lower == valid_time:
            return dict(first, cycle=f"{cycle:%Y%m%d}T{cycle.hour:02d}Z")
        second = self.point(cycle, forecast_hour(cycle, lower + 3600), lat, lon)
        weight = (valid_time - lower) / 3600
        mixed = {k: round(first[k] * (1 - weight) + second[k] * weight) for k in first}
        return dict(mixed, cycle=f"{cycle:%Y%m%d}T{cycle.hour:02d}Z")
