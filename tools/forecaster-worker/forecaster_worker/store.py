"""The worker's memory: one JSON file in the data folder.

It holds station readings (a few days), model-versus-reading samples (the bias history) and the
forecasts already sent. Stations are named by their public alias only. No coordinates are stored.
"""
import json
import os
from pathlib import Path

STATE_FILE = "state.json"
KEEP_OBS_SECONDS = 3 * 86400
KEEP_SAMPLE_SECONDS = 35 * 86400
KEEP_SENT_SECONDS = 40 * 86400


class Store:
    def __init__(self, data_dir):
        self.path = Path(data_dir) / STATE_FILE
        self.obs = {}        # alias -> {str(observed_at): {"temperature_c": centi-degC, "pressure_hpa": Pa}}
        self.samples = []    # {"station", "variable", "lead", "valid", "model", "obs"}
        self.sent = []       # {"station", "lead", "day", "variable", "commit_id", "value", "issued_at"}
        if self.path.exists():
            data = json.loads(self.path.read_text(encoding="utf-8"))
            self.obs = data.get("obs", {})
            self.samples = data.get("samples", [])
            self.sent = data.get("sent", [])

    def save(self, now: int) -> None:
        for alias in list(self.obs):
            self.obs[alias] = {t: v for t, v in self.obs[alias].items() if int(t) >= now - KEEP_OBS_SECONDS}
        self.samples = [s for s in self.samples if s["valid"] >= now - KEEP_SAMPLE_SECONDS]
        self.sent = [s for s in self.sent if s["issued_at"] >= now - KEEP_SENT_SECONDS]
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps({"obs": self.obs, "samples": self.samples, "sent": self.sent}, separators=(",", ":")),
                       encoding="utf-8")
        os.replace(tmp, self.path)

    def add_observations(self, alias: str, rows: list) -> int:
        """Keep readings in integer chain units. Returns how many were new."""
        mine = self.obs.setdefault(alias, {})
        added = 0
        for row in rows:
            at = row.get("observedAt")
            if not isinstance(at, int) or isinstance(at, bool) or at <= 0:
                continue
            values = {}
            t, p = row.get("temperatureC"), row.get("pressureHpa")
            if isinstance(t, (int, float)) and not isinstance(t, bool) and -90 <= t <= 70:
                values["temperature_c"] = round(t * 100)
            if isinstance(p, (int, float)) and not isinstance(p, bool) and 300 <= p <= 1100:
                values["pressure_hpa"] = round(p * 100)
            if values and str(at) not in mine:
                mine[str(at)] = values
                added += 1
        return added

    def reading_near(self, alias: str, variable: str, when: int, within: int = 600):
        """Mean of the station's readings within `within` seconds of `when`, or None."""
        close = [v[variable] for t, v in self.obs.get(alias, {}).items()
                 if abs(int(t) - when) <= within and variable in v]
        return round(sum(close) / len(close)) if close else None

    def newest_reading(self, alias: str):
        times = [int(t) for t in self.obs.get(alias, {})]
        return max(times) if times else None

    def has_sample(self, station: str, variable: str, lead: int, valid: int) -> bool:
        return any(s["station"] == station and s["variable"] == variable and s["lead"] == lead and s["valid"] == valid
                   for s in self.samples)

    def already_sent(self, station: str, lead: int, day: str) -> bool:
        return any(s["station"] == station and s["lead"] == lead and s["day"] == day for s in self.sent)
