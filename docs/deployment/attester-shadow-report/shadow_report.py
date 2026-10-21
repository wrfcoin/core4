#!/usr/bin/env python3
"""Bounded comparison report over the attester's `SHADOW reference:` log lines (Refs wrfcoin/infra#1255, infra#1161).

`WRFCOIN_ATTESTER_REFERENCE_MODE=shadow` submits exactly the `fixed` verdict and logs, per commit, what the per-station
reference would have done. This tool turns those log lines into a short report for the owner's shadow review. It is
offline and read-only: it parses text, touches no node, key or network, and never changes what the attester signs.

The report keeps three things apart that are easy to confuse:
  * the CONFIGURED RADIUS (`WRFCOIN_ATTESTER_MAX_REFERENCE_KM`, an owner reward-policy choice), given with --max-reference-km;
  * the COARSE-LOCATION UNCERTAINTY: the registry location is rounded to 0.01 deg, so the distance in the log can differ
    from the true one by up to ~0.8 km. It is reported as its own number and is NEVER added to or subtracted from the radius;
    a station whose distance is within that uncertainty of the radius is marked BORDERLINE instead;
  * the PHYSICS-ONLY CAP (`UNCHECKED_CONFIDENCE_CAP`, 0.5, the shared-protocol constant; deliberately NOT an option here, so
    the audit can never be loosened from the command line): a commit with no reference, or a KNOWN distance beyond the
    radius, must be PHYSICS-ONLY at that cap. A selected reference whose distance is unknown (`distance_km=n/a`, the
    daemon's missing-registry-location fallback to the configured station) is reported as UNKNOWN_DISTANCE, never as
    out of radius.
"""
import argparse
import math
import re
import sys

SHADOW_LINE = re.compile(
    r'SHADOW reference: commit_id=(?P<commit>\S+) station_id=(?P<station>\S+) fixed_ref=(?P<fixed_ref>\S+) '
    r'fixed_tier=(?P<fixed_tier>\S+) fixed_confidence=(?P<fixed_conf>[0-9]+(?:\.[0-9]+)?) '
    r'resolved_ref=(?P<resolved>\S+) distance_km=(?P<distance>n/a|[0-9]+(?:\.[0-9]+)?) '
    r'per_station_tier=(?P<ps_tier>\S+) per_station_confidence=(?P<ps_conf>[0-9]+(?:\.[0-9]+)?) '
    r'would_change=(?P<would>true|false)(?: notes=(?P<notes>\[.*\]))?\s*$')
SUMMARY_LINE = re.compile(
    r'SHADOW summary: would_change=(?P<would>[0-9]+) distinct_reference_stations=(?P<distinct>[0-9]+) '
    r'nws_lookups_this_cycle=(?P<lookups>[0-9]+)')
PHYSICS_ONLY = 'PHYSICS-ONLY'
DEFAULT_CAP = 0.5            # shared-protocol oracle::mechanical::UNCHECKED_CONFIDENCE_CAP
DEFAULT_UNCERTAINTY_KM = 0.8  # docs/oracle-attester-deployment.md: 0.01 deg rounding, "at most ~0.8 km"
CONF_TOLERANCE = 0.0005       # the log prints three decimals
MAX_LINES = 5000
MAX_LINE_CHARS = 4096
MAX_ROWS_PRINTED = 50


class ReportError(Exception):
    pass


RECORD_MARKERS = ('SHADOW reference:', 'SHADOW summary:')
MAX_INPUT_BYTES = 16 * 1024 * 1024


def parse_log(text, max_lines=MAX_LINES):
    """Return (records, summaries). Only the two record kinds the attester emits are read: lines containing
    `SHADOW reference:` or `SHADOW summary:`. Every other daemon line (including the startup warning that
    mentions `SHADOW:`) is ignored; a record line that does not parse is refused. Bounded."""
    records, summaries = [], []
    count = 0
    for raw in text.splitlines():
        if not any(marker in raw for marker in RECORD_MARKERS):
            continue
        count += 1
        if count > max_lines:
            raise ReportError(f'more than {max_lines} SHADOW records: refusing an unbounded report')
        if len(raw) > MAX_LINE_CHARS:
            raise ReportError('a SHADOW record exceeds the line bound')
        m = SHADOW_LINE.search(raw)
        if m:
            d = m.groupdict()
            records.append(dict(
                commit=d['commit'], station=d['station'], fixed_ref=d['fixed_ref'], fixed_tier=d['fixed_tier'],
                fixed_conf=float(d['fixed_conf']), resolved=d['resolved'],
                distance=None if d['distance'] == 'n/a' else float(d['distance']),
                ps_tier=d['ps_tier'], ps_conf=float(d['ps_conf']), would=d['would'] == 'true', notes=d['notes'] or '[]'))
            continue
        s = SUMMARY_LINE.search(raw)
        if s:
            summaries.append({k: int(v) for k, v in s.groupdict().items()})
            continue
        raise ReportError('a SHADOW record is not in the expected format (the attester log format changed?)')
    return records, summaries


def classify(rec, radius_km, uncertainty_km):
    """Return (label, problem). problem is None or a short refusal string. The cap is the source constant."""
    if rec['resolved'] != 'none' and rec['distance'] is None:
        # The daemon's own fallback to the configured station when the registry has no location: the distance is
        # unknown, not proven outside the radius, and the verdict may legitimately be scored.
        return 'UNKNOWN_DIST', None
    out_of_radius = rec['resolved'] == 'none' or rec['distance'] > radius_km
    if out_of_radius:
        if rec['ps_tier'] != PHYSICS_ONLY or abs(rec['ps_conf'] - DEFAULT_CAP) > CONF_TOLERANCE:
            return 'OUT_OF_RADIUS', f'out of radius but not PHYSICS-ONLY at the {DEFAULT_CAP} cap'
        return 'OUT_OF_RADIUS', None
    if abs(rec['distance'] - radius_km) <= uncertainty_km:
        return 'BORDERLINE', None
    return 'IN_RADIUS', None


def build_report(records, summaries, radius_km, uncertainty_km=DEFAULT_UNCERTAINTY_KM):
    if isinstance(radius_km, bool) or not isinstance(radius_km, (int, float)) or not math.isfinite(radius_km) or radius_km <= 0:
        raise ReportError('--max-reference-km must be a finite positive number (the daemon refuses anything else)')
    if isinstance(uncertainty_km, bool) or not isinstance(uncertainty_km, (int, float)) or not 0 <= uncertainty_km < 10:
        raise ReportError('--uncertainty-km must be a number in [0, 10)')
    if not records:
        raise ReportError('no SHADOW reference lines to compare')
    rows, problems = [], []
    for rec in records:
        label, problem = classify(rec, radius_km, uncertainty_km)
        rows.append((rec, label))
        if problem:
            problems.append(f"{rec['station']} (commit {rec['commit']}): {problem}")
    stations = sorted({r['station'] for r in records})
    resolved = sorted({r['resolved'] for r in records if r['resolved'] != 'none'})
    fixed_refs = sorted({r['fixed_ref'] for r in records})
    lines = [
        'ATTESTER SHADOW COMPARISON (synthetic or live log; read-only; the submitted verdict is the FIXED one)',
        f'configured radius        : {radius_km:g} km  (WRFCOIN_ATTESTER_MAX_REFERENCE_KM, owner reward-policy choice)',
        f'coarse-location uncertainty: up to {uncertainty_km:g} km  (registry location rounded to 0.01 deg; separate from the radius)',
        f'physics-only cap         : {DEFAULT_CAP:g}  (source constant, not adjustable; no-reference and known-out-of-radius commits must be PHYSICS-ONLY at this confidence)',
        f'commits compared         : {len(records)} across {len(stations)} station(s)',
        f'fixed reference(s)       : {", ".join(fixed_refs)}',
        f'per-station reference(s) : {len(resolved)} distinct ({", ".join(resolved) or "none"})',
    ]
    if summaries:
        s = summaries[-1]
        lines.append(f"attester summary (last)  : would_change={s['would']} distinct_reference_stations={s['distinct']} "
                     f"nws_lookups_this_cycle={s['lookups']}")
    lines.append('')
    lines.append('station            class         dist_km  fixed(ref/tier/conf)            per-station(ref/tier/conf)       would_change')
    for rec, label in rows[:MAX_ROWS_PRINTED]:
        dist = 'n/a' if rec['distance'] is None else f"{rec['distance']:.1f}"
        lines.append(f"{rec['station'][:18]:<18} {label:<13} {dist:>7}  "
                     f"{rec['fixed_ref']}/{rec['fixed_tier']}/{rec['fixed_conf']:.3f}".ljust(80)
                     + f"{rec['resolved']}/{rec['ps_tier']}/{rec['ps_conf']:.3f}".ljust(34) + str(rec['would']).lower())
    if len(rows) > MAX_ROWS_PRINTED:
        lines.append(f'... {len(rows) - MAX_ROWS_PRINTED} more row(s) not printed (report is bounded)')
    counts = {k: sum(1 for _, l in rows if l == k) for k in ('IN_RADIUS', 'BORDERLINE', 'OUT_OF_RADIUS', 'UNKNOWN_DIST')}
    lines.append('')
    lines.append('classes                  : ' + ', '.join(f'{k}={v}' for k, v in counts.items()))
    lines.append(f"would change a verdict   : {sum(1 for r in records if r['would'])} of {len(records)} (shadow only; nothing was signed from it)")
    unknown = sum(1 for _, l in rows if l == 'UNKNOWN_DIST')
    if unknown:
        lines.append(f'unverified distance      : {unknown} commit(s) used a selected reference with distance_km=n/a (registry has no location; '
                     'configured-station fallback); radius NOT checked for them')
    lines.append('NOT EVIDENCED HERE       : byte-identity of the submitted fixed-mode verdict with shadow on/off '
                 '(needs the attester test or a fixed-mode run); reward activation (per_station) stays owner-gated')
    lines.append('RESULT                   : ' + ('VIOLATIONS' if problems else 'CONSISTENT'))
    lines.extend(f'  violation: {p}' for p in problems[:MAX_ROWS_PRINTED])
    return '\n'.join(lines) + '\n', problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--max-reference-km', type=float, required=True)
    ap.add_argument('--uncertainty-km', type=float, default=DEFAULT_UNCERTAINTY_KM)
    ap.add_argument('--max-lines', type=int, default=MAX_LINES)
    ap.add_argument('log', nargs='?', help='attester log file (default: stdin)')
    args = ap.parse_args(argv)
    try:
        if args.log:
            with open(args.log, 'rb') as handle:
                data = handle.read(MAX_INPUT_BYTES + 1)
        else:
            data = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
        if len(data) > MAX_INPUT_BYTES:
            raise ReportError(f'input exceeds {MAX_INPUT_BYTES} bytes: refusing an unbounded read')
        records, summaries = parse_log(data.decode('utf-8'), args.max_lines)
        report, problems = build_report(records, summaries, args.max_reference_km, args.uncertainty_km)
    except (ReportError, OSError, UnicodeError) as exc:
        print(f'STOP: {exc}')
        return 2
    sys.stdout.write(report)
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
