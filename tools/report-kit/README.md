# Report kit (`tools/report-kit`)

Reproducible report jobs for the agents-earning epic (Phase B, story B3). Each job emits the **work unit**
of the draft ADR `docs/adr/2026-10-28-3604-report-work-unit.md`: an input manifest, a job description and an
output, each digested over a domain-separated, length-prefixed body, so a second party can rerun it and compare.

**Nothing here is paid.** Reports are paid only through the ratified work-unit ADR and a verifier pool (owner
ruling 2026-10-24). The kit only makes a report reproducible.

## `obs_quality_report.py` — daily regional observation quality (`obs_quality_daily/v1`)

Inputs are the node's `GET /transactions/contributions/<station_id>/observations` items (one JSON file per
station; the route's `{"data": {"items": [...]}}` envelope or a bare list), filtered to a block-height range.

```sh
R=tools/report-kit/obs_quality_report.py
python $R run --inputs ./obs-items --from-height 3000 --to-height 3511 --chain-id 888 --network-id testnet --out ./unit
python $R verify --unit ./unit --inputs ./obs-items   # a verifier's rerun: agree (0) | disagree (1) | inputs_mismatch (3)
```

The summary has regional aggregates only, every value a fixed-point integer:

- station, reading and attested-reading counts;
- hourly coverage (mean and minimum, and the number of stations below one half);
- per variable:
  - out-of-physical-bounds readings;
  - spikes (more than 5 scaled MADs from the station's own median);
  - drift: stations whose median residual against the regional hourly median (at least three stations that hour) exceeds a per-variable threshold.

Per-station findings are not published; that is a separate, owner-approved step.

Tolerance is `exact`: the job is CPU-deterministic, so the output digest itself is the agreement test.

Tests: `python -m unittest discover -s tools/report-kit/tests -v`.
