# Attester DEM table and sea-level shadow preview (#3299)

Refs wrfcoin/core4-archive#3299, #3285.

With `reference_sea_level_pressure` active, the oracle attester trusts a station's registered elevation for the sea-level
pressure recompute only when the elevation is within **DEM ground elevation −50 m / +60 m** (owner ruling 2026-10-24). The
band is asymmetric so that sensor height above ground is allowed. Honest negative offsets also pass: the reference station's
barometer-confirmed 227.7 m is 20.2 m below its USGS DEM value of 247.9 m.

Outside the band, the claim is not trusted. Pressure is then scored as an unverified Outlier, and the tier floor applies.

The same happens when the attester has no DEM value for the station. This fails closed on purpose: otherwise a station
could escape the bound simply by not appearing in the table.

The feature is **unscheduled** on chain 888. Until it activates, none of this changes any verdict. The release that removes
the `SCHEDULING_HELD` entry for `reference_sea_level_pressure` (`tools/cli/WrfcoinCLI/src/commands/governance.rs`) is a
follow-up, and it should land only once the preview below looks right.

Everything here is an **owner / host action**. Both tools are read-only: nothing is signed or submitted.

## 1. Build the DEM table

```bash
# On a host that can reach the node API. Needs the Operator role: the route serves EXACT coordinates.
# The token is read from the env var named by --token-env and is never printed.
python3 build_dem_table.py --node-url http://127.0.0.1:8556 --token-env WRFCOIN_OPERATOR_TOKEN \
    --out /path/on/attester/host/dem-elevations.json
# or, from saved registry page(s):
python3 build_dem_table.py --registry-file registry.json --out dem-elevations.json
```

The script:
- reads the registry from `GET /transactions/station/registrations`, following every page;
- asks USGS 3DEP (EPQS, the same service the backend uses) for the ground elevation at each station's **exact** coordinates;
- writes the table with coordinates rounded to 0.01°. The attester compares only coarse locations, so exact ones never land in the file.

It prints one line per station:

```
<station_id> registered=<m> dem=<m> IN | OUT (claim will not be trusted) | DEM_UNAVAILABLE | NO_ELEVATION
```

What the statuses mean:
- **`DEM_UNAVAILABLE`**: the station is left out of the table. A guessed DEM would be a guessed bound, so its claim will not be trusted.
- **`NO_ELEVATION`**: the station has no registered elevation. It is paired raw, exactly as today, and needs no entry.

To run the script's tests: `python3 -m unittest -q test_build_dem_table`.

Keep the file on the attester hosts; do not commit it. Station ids and locations of private stations stay private.

## 2. Shadow preview on live data

The preview runs the attester's own evaluation offline. For each station it prints the tier counts under three modes:
1. **live**: the features you pass, sea-level OFF. This is what chain 888 does today.
2. **3285 unbounded**: sea-level ON, with the registered elevation trusted unconditionally (the rule before #3299).
3. **3285+3299 bounded**: sea-level ON, with the elevation bounded by the DEM table.

```bash
# Readings: save snapshots of the pending surface (Reader role; coarse locations only) over a period, then merge:
#   curl -s -H "Authorization: Bearer $READER_TOKEN" http://127.0.0.1:8556/transactions/oracle/pending > pending-$(date +%s).json
jq -s '[.[] | (.data // .) | .observations[]]' pending-*.json > readings.json   # repeated commits are counted once
# References: the same NWS station the attester uses (api.weather.gov keeps about 7 days):
curl -s -H 'User-Agent: wrfcoin-preview' \
  'https://api.weather.gov/stations/KDLH/observations?start=2026-10-24T00:00:00Z&end=2026-10-25T00:00:00Z' > kdlh.json

oracle_attester --sea-level-preview --readings readings.json --nws-observations kdlh.json \
  --dem dem-elevations.json --features reward_gross_outlier_guard --per-reading
```

- `--features` takes the features active on the chain now, for example `active_features` from the pending response.
  `reference_sea_level_pressure` is ignored if you include it: the tool adds it itself for modes 2 and 3.
- `--per-reading` adds one line per reading, with the pairing and the audit reason.
- The preview needs no node, key or signer variables. Freshness is evaluated at each reading's own timestamp.
- The `WRFCOIN_ATTESTER_MAX_AGE_SECS` / `WRFCOIN_ATTESTER_MAX_FUTURE_SKEW_SECS` policy variables apply if they are set,
  as they do in the daemon.

Output:

```
station readings live(sea-level off) | 3285 unbounded | 3285+3299 bounded | changed | outcomes(bounded)
<station_id> 288 degraded:288 | good:288 | good:288 | 0 | Verified:288
```

`changed` counts the readings whose tier differs between modes 2 and 3. An honest station should show `0`.

For any station whose bounded tier is worse than the unbounded one, check:
- its `OUT` / `DEM_UNAVAILABLE` line from step 1;
- the `ElevationOutsideDemBand` / `NoDemElevation` outcomes in the preview.

## 3. Enable on the attesters

Set `WRFCOIN_ATTESTER_DEM_ELEVATIONS_FILE=/path/to/dem-elevations.json` on **every** attester, so they all hold the same
bytes, and restart them.

At startup the attester logs how many stations it loaded. An unreadable or malformed file refuses to start.

An entry is used only while the station's served coarse location matches the one in the file. After a station moves,
rebuild the table.
