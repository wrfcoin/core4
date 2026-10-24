#!/usr/bin/env python3
"""Build the attester's #3299 DEM table (`WRFCOIN_ATTESTER_DEM_ELEVATIONS_FILE`).

Refs wrfcoin/core4-archive#3299. With `reference_sea_level_pressure` active, the attester trusts a station's registered
elevation for the sea-level recompute only when it lies within DEM ground elevation -50 m / +60 m. The DEM value comes from
this file, built once by the operator from the registry's EXACT coordinates (the attester itself only sees 0.01-degree
coarsened ones, too coarse for a DEM lookup on a hillside).

What it does:
  1. Reads the station registry from the node: `GET /transactions/station/registrations` (Operator role, exact coordinates,
     paginated), or from a saved copy of those pages (--registry-file).
  2. For each station WITH a registered elevation, asks USGS 3DEP (EPQS, the same service the backend uses) for the ground
     elevation at the exact coordinates.
  3. Writes the table with the coordinates rounded to 0.01 degree: that is all the attester compares against, so exact
     coordinates never leave this host in the file.
  4. Prints one line per station: id, registered elevation, DEM, and IN/OUT of the band. No coordinates are printed.

Read-only towards the node and the chain: GETs only, nothing signed or submitted. The bearer token is read from the
environment variable named by --token-env and is never printed. A DEM lookup that fails or returns no-data leaves the
station OUT of the table (the attester then does not trust its claim): a guessed DEM would be a guessed bound.
"""
import argparse
import json
import math
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

EPQS_URL = 'https://epqs.nationalmap.gov/v1/json'
BAND_BELOW_M = 50.0
BAND_ABOVE_M = 60.0
# EPQS reports no-data as -1000000; the attester refuses anything outside this plausibility range.
MIN_DEM_M = -500.0
MAX_DEM_M = 9000.0
PAGE_LIMIT = 200
MAX_PAGES = 1000


def within_band(registered_m, dem_m):
    """Mirror of shared-protocol `elevation_within_dem_band`: edges included, non-finite is outside."""
    try:
        registered_m = float(registered_m)
        dem_m = float(dem_m)
    except (TypeError, ValueError):
        return False
    if registered_m != registered_m or dem_m != dem_m or abs(registered_m) == float('inf') or abs(dem_m) == float('inf'):
        return False
    return dem_m - BAND_BELOW_M <= registered_m <= dem_m + BAND_ABOVE_M


def coarse(value):
    """0.01-degree rounding exactly as the attester's `(v * 100.0).round() / 100.0`: Rust rounds half AWAY from zero,
    Python's round() rounds half to even, and a key that differs at a .xx5 boundary would silently drop the station."""
    scaled = abs(float(value) * 100.0)
    whole = math.floor(scaled)
    if scaled - whole >= 0.5:
        whole += 1
    return math.copysign(whole, value) / 100.0


def parse_dem_value(body):
    """EPQS JSON `{"value": "247.9", ...}` -> float, or None for no-data / malformed."""
    try:
        raw = json.loads(body).get('value')
        value = float(raw)
    except (ValueError, TypeError, AttributeError):
        return None
    if value != value or not MIN_DEM_M <= value <= MAX_DEM_M:
        return None
    return value


def build_table(stations, dem_lookup):
    """stations: registry entries; dem_lookup(lat, lon) -> float|None. Returns (table, report_rows)."""
    rows = []
    table = {'version': 1, 'source': 'usgs_3dep_epqs', 'stations': []}
    seen = set()
    for station in sorted(stations, key=lambda s: str(s.get('station_id', ''))):
        station_id = str(station.get('station_id', ''))
        if not station_id or station_id in seen:
            continue
        seen.add(station_id)
        elevation = station.get('elevation_meters')
        if elevation is None:
            rows.append((station_id, None, None, 'NO_ELEVATION (pairs raw; not in table)'))
            continue
        try:
            lat = float(station['latitude'])
            lon = float(station['longitude'])
        except (KeyError, TypeError, ValueError):
            rows.append((station_id, elevation, None, 'NO_LOCATION (not in table)'))
            continue
        dem = dem_lookup(lat, lon)
        if dem is None:
            rows.append((station_id, elevation, None, 'DEM_UNAVAILABLE (not in table: claim will not be trusted)'))
            continue
        table['stations'].append({
            'station_id': station_id,
            'latitude': coarse(lat),
            'longitude': coarse(lon),
            'dem_ground_elevation_m': round(dem, 2),
        })
        rows.append((station_id, elevation, dem, 'IN' if within_band(elevation, dem) else 'OUT (claim will not be trusted)'))
    return table, rows


def http_get_json(url, token=None, timeout=10):
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=timeout) as res:
        return res.read().decode('utf-8')


def fetch_registry(node_url, token):
    stations, after = [], None
    for _ in range(MAX_PAGES):
        query = {'limit': PAGE_LIMIT}
        if after:
            query['after'] = after
        page = json.loads(http_get_json(
            f"{node_url.rstrip('/')}/transactions/station/registrations?{urllib.parse.urlencode(query)}", token))
        page = page.get('data', page)
        stations.extend(page.get('stations', []))
        if not page.get('has_more'):
            return stations
        after = page.get('next_cursor')
        if not after:
            break
    raise SystemExit('registry pagination did not terminate')


def epqs_lookup(lat, lon):
    query = urllib.parse.urlencode({'x': lon, 'y': lat, 'units': 'Meters', 'wkid': 4326, 'includeDate': 'false'})
    try:
        return parse_dem_value(http_get_json(f'{EPQS_URL}?{query}'))
    except (urllib.error.URLError, OSError, ValueError):
        return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--node-url', help='node API base, e.g. http://127.0.0.1:8556 (Operator role required)')
    source.add_argument('--registry-file', help='saved JSON of the registry page(s): {"stations": [...]} or a list')
    parser.add_argument('--token-env', default='WRFCOIN_OPERATOR_TOKEN',
                        help='name of the env var holding the Operator bearer token (value never printed)')
    parser.add_argument('--station', action='append', default=[], help='only these station ids (repeatable)')
    parser.add_argument('--out', required=True, help='table file to write (keep it on the attester host)')
    args = parser.parse_args(argv)

    if args.node_url:
        stations = fetch_registry(args.node_url, os.environ.get(args.token_env))
    else:
        with open(args.registry_file, encoding='utf-8') as f:
            loaded = json.load(f)
        loaded = loaded.get('data', loaded) if isinstance(loaded, dict) else loaded
        stations = loaded.get('stations', []) if isinstance(loaded, dict) else loaded
    if args.station:
        wanted = set(args.station)
        stations = [s for s in stations if s.get('station_id') in wanted]

    table, rows = build_table(stations, epqs_lookup)
    with open(args.out, 'w', encoding='utf-8') as f:
        json.dump(table, f, indent=2, sort_keys=True)
        f.write('\n')
    print(f'#3299 DEM table: {len(table["stations"])} station(s) written to {args.out}; band DEM -{BAND_BELOW_M:g}/+{BAND_ABOVE_M:g} m')
    for station_id, elevation, dem, verdict in rows:
        elevation_s = '-' if elevation is None else f'{float(elevation):.1f}'
        dem_s = '-' if dem is None else f'{dem:.1f}'
        print(f'{station_id} registered={elevation_s} dem={dem_s} {verdict}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
