"""Per-station bias correction, version 0.

A sample is one past hour at one station: what the model said for that hour at a given lead,
and what the station then read. error = reading - model, in integer chain units
(0.01 degC for temperature, Pa for pressure).

Correction for a new forecast:
  1. take this station's errors for the same variable and lead from the last WINDOW_DAYS days;
  2. prefer samples from the same time of day (within +-HOUR_WINDOW hours); use all hours when
     fewer than MIN_SAME_HOUR of those exist;
  3. take the median (one bad reading cannot move it much);
  4. shrink it toward zero when there are few samples: median * n / (n + SHRINK);
  5. clamp it to a sane size.
With no samples the correction is 0, so the forecast is the raw model value.
"""
from statistics import median

WINDOW_DAYS = 30
HOUR_WINDOW = 3
MIN_SAME_HOUR = 3
SHRINK = 5
MAX_CORRECTION = {"temperature_c": 1500, "pressure_hpa": 15000}   # 15 degC, 150 hPa
MODEL_ID = "gfs-station-bias/v0"


def _hour_distance(a: int, b: int) -> int:
    d = abs(a - b) % 24
    return min(d, 24 - d)


def correction(samples, *, station: str, variable: str, lead_hours: int, valid_time: int) -> tuple[int, int]:
    """Return (correction, samples_used). `samples` is an iterable of dicts from the store."""
    oldest = valid_time - WINDOW_DAYS * 86400
    hour = (valid_time // 3600) % 24
    mine = [s for s in samples
            if s["station"] == station and s["variable"] == variable and s["lead"] == lead_hours
            and oldest <= s["valid"] < valid_time]
    same_hour = [s for s in mine if _hour_distance((s["valid"] // 3600) % 24, hour) <= HOUR_WINDOW]
    used = same_hour if len(same_hour) >= MIN_SAME_HOUR else mine
    if not used:
        return 0, 0
    n = len(used)
    mid = median(s["obs"] - s["model"] for s in used)
    shrunk = round(mid * n / (n + SHRINK))
    cap = MAX_CORRECTION[variable]
    return max(-cap, min(cap, shrunk)), n


def corrected(model_value: int, samples, **key) -> tuple[int, int, int]:
    """Return (forecast, correction, samples_used), all integers."""
    delta, n = correction(samples, **key)
    return model_value + delta, delta, n
