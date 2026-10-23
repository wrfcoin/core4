# Forecaster kit (`tools/forecast-kit`)

A reference script for a person or an agent who wants to take part in the forecast earning
loop (epic #3604, Phase A, story A3): pull a station's recent observations, produce a T+6h
forecast by any method, and prepare the **exact files** that `wrfcoin forecast commit` /
`wrfcoin forecast reveal` (#3084) consume. The kit never submits anything: it writes
`commit.sh` / `reveal.sh` that call the CLI with `--validate-only`, which parses, signs and
checks the body and prints the commit id **without sending**. Removing that flag is the
wallet owner's step.

```
observations ──fetch-observations──▶ store ──predict──▶ forecast.json + window.json
   ──prepare-commit──▶ data.json, manifest.json, commit.json, commit.sh   (validate only)
   … wait until valid_at (T+6h), fetch again, optionally receive a signed oracle receipt …
   ──score──▶ score.json + metrics.json (declared ∩ reported variables; the signer's input)
   ──prepare-reveal──▶ reveal.sh                                           (validate only)
   owner: run the same two lines without --validate-only                   (the only chain writes)
   ──status──▶ GET /transactions/commit/<id>/status
```

Requirements: Python ≥ 3.10 standard library; `numpy` + `onnxruntime` only for `predict --model`
(the shipped `learned-conv1d-v1.onnx`); the `wrfcoin` CLI binary for the validate step.
No key material ever passes through the kit: `commit.sh` reads `SIGNER_KEY` (a path) and the CLI
reads the key file itself (mode 0600 enforced).

## 1. Observations

```sh
KIT=tools/forecast-kit/forecast_kit.py
# public backend: the chain tail (~64 blocks ≈ 1.3 h); call it every hour and the store grows
python $KIT fetch-observations --station st_057d50adbb07 --source public --out ./obs
# node Reader route: full history; the bearer is read from $WRFCOIN_READER_TOKEN and never printed
python $KIT fetch-observations --station PWS-EXAMPLE-STATION --source node --node http://127.0.0.1:8556 --hours 240 --out ./obs
# a trainer-format CSV (e.g. from gpu/fetch_asos_history.py)
python $KIT fetch-observations --station DLH --source csv --csv ./observations/DLH.csv --out ./obs
```

The node backfill reports the actual reading count and timestamp span against `--hours`.
If the cadence estimate or request cap yields less history, it saves the partial data and
prints an explicit `WARNING: history shortfall`; that is not full requested coverage.

The store is `./obs/<station>.jsonl`, one `{"observed_at", "values"}` per reading, deduplicated
by time. Public aliases (`st_…`) and on-chain `station_id`s differ: the commit must carry the
**on-chain `station_id`**; the public route is keyed by alias. The node's unauthenticated
`GET /transactions/commit/<commit_id>/status` returns the `station_id` behind any `commitId`
the public tail shows, which is how to learn it. The two live stations' payloads currently
carry no humidity, so a learned three-variable forecast is impossible for them today; the kit
falls back to `persistence-trend/v1` on the variables present and says so on stderr.

## 2. Predict

```sh
python $KIT predict --observations ./obs --station PWS-EXAMPLE-STATION \
  --model tools/forecast-proto/models/learned-conv1d-v1.onnx --out ./fc
```

Builds the 24 h window of hourly means ending at the newest full hour (`--at` to pick another),
excludes the still-open current UTC hour by default, runs the ONNX graph on the CPU,
and writes `forecast.json`. `issued_at` is the closing boundary of the last input hour;
`valid_at` is six hours after that boundary. The persistence fallback fits real timestamp
offsets, preserving gaps rather than treating sparse readings as consecutive hours:

```json
{"model_id": "learned-conv1d/v1", "issued_at": 1792382400, "valid_at": 1792404000,
 "horizon_hours": 6, "station_id": "PWS-EXAMPLE-STATION",
 "values": {"humidity_pct": 61.2, "pressure_hpa": 996.4, "temperature_c": 11.7}}
```

plus `window.json` (the inputs, for the manifest's `input_hash`). Any other method is fine —
an LLM, a physics model, a copy of the NWS forecast — as long as it writes the same shape.
Nothing prescribes the model; the chain pays for skill against what the station later reports.

## 3. Prepare the commit (validate only)

```sh
python $KIT prepare-commit --forecast ./fc/forecast.json --window ./fc/window.json \
  --forecaster 0xYOURWALLET --chain-id 777 --network-id testnet \
  --node http://127.0.0.1:8556 --cli /path/to/wrfcoin --out ./fc/prepared
SIGNER_KEY=/secure/forecaster.key ./fc/prepared/commit.sh | tee ./fc/prepared/commit-validate.json
```

Writes:

- `data.json` — the forecast payload, compact canonical JSON (this is `--data-file`);
- `manifest.json` — `ForecastArtifactManifest` v1: `model_id`, `input_hash` (SHA3-256 of the
  window), `output_hash` (SHA3-256 of the payload), `produced_at = issued_at`,
  `region = station_id`, `horizon_hours`, sorted variable lists; field order is the struct's
  declaration order because `manifest_digest` hashes the compact JSON;
- `commit.json` — forecaster, station, `nonce`, `timestamp` (= `issued_at`), the kit's own
  `commit_id` (`sha3_256("{data}||{nonce}||{station_id}||{timestamp}||forecast")`, the
  `Transaction::compute_commit_id` rule), `manifest_digest`, `valid_at`, `submitted: false`;
- `commit.sh` — `wrfcoin --output json forecast commit --validate-only …`.

`commit.sh` prints the CLI's JSON envelope with
`"data": {"validated": true, "kind": "commit", "submitted": false, "commit_id": …, "body": …}`.
**Cross-check:** its `data.commit_id` must equal `commit.json`'s (`prepare-reveal --cli-output`
checks it for you). If they differ, the payload's
canonical form differs between Python and Rust (exotic floats); correct the payload, do not submit.

The forecaster address must be the wallet the Contributor bearer is bound to (#3070:
`WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS` on the node); otherwise the node answers
`FORECASTER_NOT_BOUND_TO_BEARER`.

## 4. The owner submits the commit

This is the one on-chain write and it is not the kit's. The wallet owner runs the line from
`commit.sh` **without `--validate-only`**, with `--token` set to the Contributor bearer and
`--signer-key` the forecaster key file. Keep the printed `tx_id`. Then:

```sh
python $KIT status --commit-id <commit_id> --node http://127.0.0.1:8556
```

## 5. After the target time: reveal (validate only)

```sh
python $KIT prepare-reveal --dir ./fc/prepared [--receipt ./receipt.json] \
  --cli-output ./fc/prepared/commit-validate.json --cli /path/to/wrfcoin
./fc/prepared/reveal.sh
```

`prepare-reveal` refuses (exit 1) before `valid_at` unless `--allow-early`, checks that the
current `data.json` still hashes to the recorded commit id using its nonce, station and
timestamp, checks that the manifest is unchanged, and — if a signed `ForecastVerificationReceipt` from the oracle is given —
that it binds this `commit_id` and `manifest_digest`, carries a nonce and a signature, and has a
`skill_score` in `[0, 1]`. Integrity failures return before writing the reveal script or
updating the commit record; `--allow-early` waives timing only. Without a receipt the reveal is valid and **scores at the baseline
0.1** (consensus `BASELINE_FORECAST_SKILL`); a receipt only exists once an oracle signs one
(#3149, `wrfcoin forecast sign-receipt` in PR #3150). The owner submits `reveal.sh`'s line
without `--validate-only`, then reads the credit back with `status` and in wallet history (#3520).

## 5b. Score it yourself: only the variables the station reports

Owner ruling (2026-10-19): a forecast is scored over the variables its manifest **declares** and
the truth observation **reports**, averaged over that intersection and nothing else
(`docs/forecast/variable-subset-scoring.md`). The kit follows it end to end: `predict` forecasts
the variables present in the window, `prepare-commit` declares exactly those, and once the
target hour's reading is in the store:

```sh
python $KIT fetch-observations --station PWS-EXAMPLE-STATION --source public --out ./obs   # after valid_at
python $KIT score --forecast ./fc/forecast.json --window ./fc/window.json   --observations ./obs --station PWS-EXAMPLE-STATION --out ./fc/score
```

writes `score.json` (per-variable errors and skills, the declared and reported sets) and
`metrics.json` = `{"mae", "rmse", "bias", "skill_score"}` — the metrics file an oracle operator
hands to `wrfcoin forecast sign-receipt --metrics-file` (#3149 / PR #3150). The rule is the
prototype's receipt rule (`max(0, 1 − |e| / |e_window-climatology|)` per scored variable, mean
over the scored set); a forecast whose declared variables the station did not report is
unscorable and gets no metrics. For a two-variable station, a two-variable graph (trainer
`--variables temperature_c,pressure_hpa`) runs in `predict` because the kit reads the model's
variable list from its `-export.json`; the producer's own loader still expects three channels.

`export-csv --observations ./obs --station X --out ./csv` writes the store as the trainer's
hourly CSV (unreported variables blank), which is how the chain's own attested history was
handed to `gpu/train_forecast.py`.

## 6. Weekly-budget pattern for an agent

An agent with a weekly compute allowance should spend it on **many cells, few runs**, because
each commit is one shot at one station and one lead:

```
every hour (cron):   fetch-observations for each station in the watch list   (read-only)
every hour, N times a day per station (e.g. N=4 at 00/06/12/18Z):
    predict → prepare-commit → commit.sh (validate) → hand commit.json to the owner's submit queue
six hours after each commit:   prepare-reveal (+ receipt if the oracle issued one) → reveal.sh → owner submits
daily:               status on every commit id; keep a CSV of skill_score, model_id, station
weekly:              compare your model's track record with persistence-trend/v1 (the chain's
                     fallback) using forecast-score's score_csv example; retrain or switch methods
```

`N × stations × 7` commits a week is the budget; the node's per-wallet rate limits (#3114, and
the quotas of story A5 when they land) are the ceiling. Submitting climatology or a copy of the
persistence baseline earns no skill once anti-gaming scoring (A6, #3244) is active, so budget
spent on better inputs beats budget spent on more commits.

## Tests

```sh
python -m unittest discover -s tools/forecast-kit/tests -v      # offline, no model needed
cargo test -p wrfcoin_cli --test cli_api_contract               # the CLI's --validate-only contract
```

## Boundaries

The kit performs read-only GETs only (`/api/stations/<alias>/observations`, `/network/info`,
`/chain/blocks`, `/transactions/contributions/<id>/observations` with a Reader bearer from the
environment, `/transactions/commit/<id>/status`). It never POSTs, never reads a key file, never
prints a bearer, and never stores station coordinates.
