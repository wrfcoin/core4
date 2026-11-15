#!/usr/bin/env python3
"""Fetch REAL hourly station observations into the trainer's observations format.

Source: the Iowa Environmental Mesonet ASOS/METAR archive
(https://mesonet.agron.iastate.edu/request/download.phtml), a public, read-only
HTTP service that needs no API key. It serves the routine hourly METAR
observations of airport weather stations (temperature, relative humidity,
sea-level pressure) going back decades.

Why this source: the chain's own attested observations are the training set
this model is meant for, but as of 2026-10-19 they are not reachable read-only
from outside a node: the public backend route `/api/stations/<alias>/observations`
serves only the last ~64 blocks (about 1.3 h) and admits no pagination through
the public edge, and the node's full-history route
`/transactions/contributions/<station_id>/observations` requires a Reader bearer.
`tools/forecast-kit fetch-observations` covers those two sources; this script
covers the public archive so a forecaster (or this repository's trainer) can get
months of real hourly history for the climate regions the chain's stations sit in.

These are real observations of the same atmosphere, from airport stations near
the chain's stations; they are NOT the chain's attested readings. Every artifact
produced from them must say so.

Output format (one CSV per station, `<STATION>.csv`, documented in
`tools/forecast-proto/models/README.md`):

    time_utc,temperature_c,pressure_hpa,humidity_pct
    2026-06-01T00:00:00Z,22.78,1011.5,61.54

- `time_utc` is on the hour (ISO-8601, UTC). METAR routine observations are
  issued a few minutes before the hour (e.g. 00:53); each row is assigned to
  its nearest hour and rows that land in the same hour are averaged.
- `pressure_hpa` is sea-level pressure (`mslp`). The chain's stations report
  their own sensor's pressure; the model is trained on per-window anomalies,
  so a constant level offset between the two does not change its forecast.
- Missing values are blank. No coordinates or elevations are requested
  (`latlon=no`, `elev=no`) or stored.

Usage:
    fetch_asos_history.py --stations DLH,HIB,SJC --start 2026-06-01 --end 2026-10-19 \
        --out /path/to/observations [--raw-dir /path/to/keep/raw/csvs]
    fetch_asos_history.py --from-raw /path/to/raw/csvs --out /path/to/observations
"""

import argparse
import csv
import datetime as dt
import os
import sys
import urllib.parse
import urllib.request

IEM_URL = "https://mesonet.agron.iastate.edu/cgi-bin/request/asos.py"
VARIABLES = ["temperature_c", "pressure_hpa", "humidity_pct"]
IEM_FIELDS = {"tmpc": "temperature_c", "mslp": "pressure_hpa", "relh": "humidity_pct"}


def iem_query(station: str, start: dt.date, end: dt.date) -> str:
    params = [
        ("station", station),
        ("data", "tmpc"), ("data", "relh"), ("data", "mslp"),
        ("year1", start.year), ("month1", start.month), ("day1", start.day),
        ("year2", end.year), ("month2", end.month), ("day2", end.day),
        ("tz", "Etc/UTC"), ("format", "onlycomma"),
        ("latlon", "no"), ("elev", "no"),
        ("missing", "M"), ("trace", "T"), ("direct", "no"),
        # 3 = routine hourly METAR; specials (report_type=4) are excluded so
        # the hourly bucketing stays one observation per hour in the normal case.
        ("report_type", "3"),
    ]
    return IEM_URL + "?" + urllib.parse.urlencode(params)


def download(station: str, start: dt.date, end: dt.date, raw_dir: str) -> str:
    os.makedirs(raw_dir, exist_ok=True)
    path = os.path.join(raw_dir, f"{station}.csv")
    url = iem_query(station, start, end)
    with urllib.request.urlopen(url, timeout=300) as resp, open(path, "wb") as fh:
        fh.write(resp.read())
    return path


def nearest_hour(ts: dt.datetime) -> dt.datetime:
    base = ts.replace(minute=0, second=0, microsecond=0)
    return base + dt.timedelta(hours=1) if ts.minute >= 30 else base


def convert(raw_path: str, out_dir: str) -> dict:
    """IEM `onlycomma` CSV -> hourly observations CSV. Returns a coverage summary."""
    buckets: dict[dt.datetime, dict[str, list[float]]] = {}
    station = None
    with open(raw_path, newline="") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            station = station or row.get("station")
            try:
                ts = dt.datetime.strptime(row["valid"], "%Y-%m-%d %H:%M").replace(tzinfo=dt.timezone.utc)
            except (KeyError, ValueError):
                continue
            hour = nearest_hour(ts)
            slot = buckets.setdefault(hour, {v: [] for v in VARIABLES})
            for field, name in IEM_FIELDS.items():
                raw = (row.get(field) or "").strip()
                if raw in ("", "M", "T"):
                    continue
                try:
                    slot[name].append(float(raw))
                except ValueError:
                    continue
    if not buckets:
        return {"station": station or os.path.basename(raw_path), "rows": 0}
    station = station or os.path.splitext(os.path.basename(raw_path))[0]
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, f"{station}.csv")
    hours = sorted(buckets)
    complete = 0
    with open(out_path, "w", newline="") as fh:
        writer = csv.writer(fh)
        writer.writerow(["time_utc"] + VARIABLES)
        for hour in hours:
            values = []
            for name in VARIABLES:
                xs = buckets[hour][name]
                values.append(f"{sum(xs) / len(xs):.2f}" if xs else "")
            complete += all(values)
            writer.writerow([hour.strftime("%Y-%m-%dT%H:%M:%SZ")] + values)
    span_hours = int((hours[-1] - hours[0]).total_seconds() // 3600) + 1
    return {
        "station": station,
        "rows": len(hours),
        "complete_rows": complete,
        "first": hours[0].strftime("%Y-%m-%dT%H:%MZ"),
        "last": hours[-1].strftime("%Y-%m-%dT%H:%MZ"),
        "span_hours": span_hours,
        "missing_hours": span_hours - complete,
        "path": out_path,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="observations directory (one CSV per station)")
    ap.add_argument("--stations", help="comma-separated IEM/ICAO station ids without the K prefix, e.g. DLH,SJC")
    ap.add_argument("--start", help="first day, YYYY-MM-DD (UTC)")
    ap.add_argument("--end", help="last day (exclusive), YYYY-MM-DD (UTC)")
    ap.add_argument("--raw-dir", help="keep the downloaded IEM CSVs here (default: not kept)")
    ap.add_argument("--from-raw", help="convert already-downloaded IEM CSVs from this directory instead of fetching")
    args = ap.parse_args()

    raw_paths = []
    if args.from_raw:
        raw_paths = sorted(
            os.path.join(args.from_raw, f) for f in os.listdir(args.from_raw) if f.lower().endswith(".csv")
        )
    else:
        if not (args.stations and args.start and args.end):
            ap.error("--stations, --start and --end are required unless --from-raw is given")
        start = dt.date.fromisoformat(args.start)
        end = dt.date.fromisoformat(args.end)
        raw_dir = args.raw_dir or os.path.join(args.out, ".raw")
        for station in [s.strip().upper() for s in args.stations.split(",") if s.strip()]:
            raw_paths.append(download(station, start, end, raw_dir))
            print(f"fetched {station}", file=sys.stderr)

    total_rows = total_complete = 0
    for raw in raw_paths:
        summary = convert(raw, args.out)
        total_rows += summary.get("rows", 0)
        total_complete += summary.get("complete_rows", 0)
        print(
            f"{summary['station']}: rows={summary.get('rows', 0)} complete={summary.get('complete_rows', 0)} "
            f"span_hours={summary.get('span_hours', 0)} missing_hours={summary.get('missing_hours', 0)} "
            f"{summary.get('first', '')}..{summary.get('last', '')}"
        )
    print(f"observations: {len(raw_paths)} stations, {total_rows} hourly rows, {total_complete} complete rows -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
