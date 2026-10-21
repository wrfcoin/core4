# Attester shadow comparison report (offline)

Refs wrfcoin/infra#1255 (epic 3/4 item), SOL-R story 3, infra#1161. Written by loop-build r21 lane CB-1.

With `WRFCOIN_ATTESTER_REFERENCE_MODE=shadow` the attester submits exactly the `fixed` verdict and logs one
`SHADOW reference:` line per commit and one `SHADOW summary:` per cycle (see `docs/oracle-attester-deployment.md`).
`shadow_report.py` turns those lines into a short, bounded report for the owner's shadow review. It reads text only: no
node, key or network, and it changes nothing the attester signs. Default (`fixed`) reward behaviour is untouched.

```bash
python3 shadow_report.py --max-reference-km <the configured WRFCOIN_ATTESTER_MAX_REFERENCE_KM> attester.log
# exit 0 CONSISTENT, 1 VIOLATIONS, 2 bad input (e.g. unknown log format, over the bound)
python3 -m unittest -q test_shadow_report
```

## What the report keeps apart
* **Configured radius**: the owner's reward-policy number, given on the command line and printed on its own line.
* **Coarse-location uncertainty**: the registry location is rounded to 0.01 degree, so the logged distance can differ from the
  true one by up to ~0.8 km (`--uncertainty-km`). It is a separate line and is never added to or subtracted from the radius.
  A station whose distance is within it of the radius is marked `BORDERLINE`, never silently moved across the line.
* **Physics-only cap**: a commit with no reference, or a KNOWN distance beyond the radius, must be `PHYSICS-ONLY` at the 0.5 cap
  (`UNCHECKED_CONFIDENCE_CAP`, the shared-protocol constant); anything else is a violation. The cap is deliberately not an
  option, so the check cannot be loosened from the command line. The radius must be finite and positive, as the daemon requires.
* **Unknown distance**: the daemon's missing-registry-location fallback selects the configured station with `distance_km=n/a`
  and may score the commit. The report shows it as `UNKNOWN_DIST` and counts it under "unverified distance"; the radius is NOT
  checked for it and it is not a violation.
It also lists the fixed reference, how many distinct per-station references were chosen, and how many verdicts would change.
Only the two record kinds the attester emits (`SHADOW reference:` and `SHADOW summary:`) are read; the daemon's other log lines,
including its startup warning, are ignored, while a malformed record is refused. Bounds: the whole input is read up to
16 MiB (more is refused), at most 5000 records, 50 rows printed; no coordinates appear in the log or the report.

## Not evidenced here
Byte-identity of the submitted fixed-mode verdict with shadow on/off cannot be read from these logs (the attester logs only
the shadow side). That needs a test inside the attester, which edits `native-chain/src/bin/oracle_attester.rs`; it is left
until core4#3395 (which rewrites that loop) has merged. Reward activation (`per_station`) remains owner-gated.
