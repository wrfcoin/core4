# Forecast models shipped with the producer

Two ONNX graphs live here. Both have the same contract, which is what the Rust
producer (`src/learned.rs`, model id `learned-conv1d/v1`, tract-onnx 0.23.7) loads:

- Input: float32 `[1,24,3]`, 24 consecutive hourly samples, in
  `temperature_c, pressure_hpa, humidity_pct` order, oldest first, **raw physical units**.
- Output: float32 `[1,3]`, the same variable order, valid six hours after the last input.
- Normalization and unit handling are inside the graph. Opset 17.
- A missing, invalid or incompatible model, non-finite output, or an irregular window falls
  back to `persistence-trend/v1` and logs the fallback once per process. Forecast, manifest
  and run-summary model IDs name the model that actually ran.

To opt into inference, add `--model learned-conv1d/v1 --onnx-path <model.onnx>` to
`forecast-proto run`, or set `WRFCOIN_FORECAST_MODEL` and `WRFCOIN_FORECAST_ONNX`.
The default remains `persistence-trend/v1`.

| file | trained on | size | parity record | status |
|---|---|---|---|---|
| `synthetic-placeholder.onnx` | the trainer's synthetic diurnal+synoptic fixture | 87,640 B | `placeholder-export.json` | runtime integration fixture only |
| `learned-conv1d-v1.onnx` | **real hourly station observations** (below) | 744,480 B | `learned-conv1d-v1-export.json` | beats the shipped baseline on a held-out window; **not validated for payouts** |

**Neither model is validated for payouts.** "Validated for payouts" would mean skill measured
against the chain's own attested observations by the oracle that signs receipts
(`docs/forecast/scorer-truth-source.md`), and no such receipt exists yet (#3149).

## `learned-conv1d-v1.onnx` — provenance

Trained 2026-10-19 by `gpu/train_forecast.py --observations` on the workstation GPU
(AMD Radeon RX 6700 XT via torch-directml; `trained_on_device: directml`, exported on CPU).

- **Data:** public hourly METAR observations of 11 ASOS airport stations in the two US regions
  where the chain's stations sit (Upper Midwest: DLH HIB INL BRD STC MSP; California: SJC OAK
  SFO LVK SCK), 2026-06-01 → 2026-10-19 UTC, from the Iowa
  Environmental Mesonet archive (`gpu/fetch_asos_history.py`, read-only, no key, no
  coordinates stored). 36,960 station-hours, 36,795 complete. These are **real observations of
  the same atmosphere, from stations near the chain's stations; they are not the chain's
  attested readings** — those are not reachable read-only beyond a ~1.3 h tail
  (`/api/stations/<alias>/observations`) without a Reader bearer, and the live WS-5000 payloads
  carry no humidity. The chain's stations report sensor pressure while this archive reports
  sea-level pressure; the graph centres each window on its own mean, so a constant level
  offset does not change the forecast (`observed_data_onnx_is_level_invariant` pins this).
- **Split:** chronological. Targets from 2026-10-09T01Z onward (10 days) are held out and were
  never trained or tuned on; the 5 days before them are the inner validation slice used for
  early stopping and for choosing the channel width; everything earlier is the fit set.
  Windows: 32,403 fit / 1,286 inner-validation / 2,557 held-out. Gaps ≤ 2 h are linearly
  interpolated; longer gaps drop every window touching them.
- **Model:** dilated causal Conv1d ×4 (dilations 1,2,4,8) + MLP head, residual on the last
  observation, per-window mean centring and constant scale `[10,10,20]` inside the graph;
  128 channels, 182,915 parameters. Seed 20260909. Lerp-free Adam (lr 1e-3, weight decay
  1e-5; see "DirectML notes"), batch 512, SmoothL1 on normalized error, early stop
  patience 12 → 29 epochs, best epoch 17, inner-validation normalized MAE 0.2641.
  Channel width was selected on the inner-validation slice among {64, 128, 256}
  (0.2692 / **0.2641** / 0.2688), not on the held-out window.
- **Held-out results (2,557 forecasts, 11 stations), scored two ways:**

  The receipt rule the prototype signs today (per forecast and variable,
  `max(0, 1 − |e| / |e_window-climatology|)`, averaged — `tools/forecast-proto skill::score`):

  | model | temperature_c | pressure_hpa | humidity_pct | mean `skill_score` | ratio vs persistence-trend |
  |---|---|---|---|---|---|
  | `persistence-trend/v1` (shipped fallback) | 0.200 | 0.438 | 0.200 | 0.2793 | 1.00 |
  | `learned-conv1d/v1` (this file) | 0.411 | 0.551 | 0.359 | **0.4401** | **1.58** |

  MAE (model / persistence-trend / window climatology): temperature 2.51 / 5.23 / 4.10 °C,
  pressure 1.45 / 1.95 / 3.95 hPa, humidity 10.53 / 19.03 / 14.43 %. Aggregate skill vs
  climatology (`1 − MAE/MAE_clim`): model 0.388 / 0.634 / 0.270, persistence-trend
  0.000 / 0.508 / 0.000 — the baseline's zero on the diurnal variables is the #3067 finding,
  reproduced on real data. The model beats the baseline on every variable and at every one of
  the 11 stations (per-station mean skill 0.355–0.569 vs 0.228–0.343).

  The `forecast-score` crate's own metrics over the same predictions
  (`cargo run -p wrfcoin-forecast-score --example score_csv -- --predictions holdout-model.csv
  --reference holdout-persistence-trend.csv`; `skill_score = 1 − RMSE/stddev(observed)`,
  `skill_vs_ref` = `ReferenceStrategy::FixedClimatology` with the baseline's RMSE as reference):

  ```
  variable              n       mae      rmse      bias skill_score   ref_rmse  skill_vs_ref
  humidity_pct     2557   10.5315   13.6568    1.6750      0.3799    23.9561        0.4299
  pressure_hpa     2557    1.4450    1.9878    0.0089      0.7354     2.6123        0.2390
  temperature_c    2557    2.5084    3.4203   -0.3353      0.5038     6.7175        0.4908
  mean skill_score over 3 variables: 0.5397; mean skill_vs_reference: 0.3866
  ```
  and for `persistence-trend/v1` itself: skill_score 0.0000 / 0.6522 / 0.0255, mean 0.2259.

- **Parity:** one held-out window and the exporting process's CPU prediction are in
  `learned-conv1d-v1-export.json` (`input`, `expected_output`, `rust_parity_tolerance_abs:
  0.005`). `src/learned.rs::observed_data_onnx_matches_cpu_export` reproduces it through
  tract. onnxruntime 1.31.0 vs torch CPU over 64 held-out windows: max |Δ| 0.00095.
  Digest `sha256:c9975fc5cfa940f0c0ed6d1e1e0e00fe059fbdc0dee961dab6bdcd727d8947f7` (prefixed in the record so secret scanners ignore it).
- **Limits:** six-hour lead only; trained on two regions and one summer→autumn season; airport
  stations, not backyard sensors; no wind, precipitation or categorical outputs; skill on the
  chain's own stations is unmeasured. A forecaster who submits this model's output is paid at
  baseline 0.1 until an oracle signs receipts (#3149).

## Reproducing

Python 3.11 venv with `torch-directml` (pulls torch 2.4.1), `numpy`, `onnx`, `onnxruntime`
(`uv venv --python 3.11 .venv && uv pip install --python .venv/bin/python
torch-directml numpy pandas onnx onnxruntime`). Then:

```sh
# 1. real observations into the trainer's format (one CSV per station)
python tools/forecast-proto/gpu/fetch_asos_history.py \
  --stations DLH,HIB,INL,BRD,STC,MSP,SJC,OAK,SFO,LVK,SCK \
  --start 2026-06-01 --end 2026-10-19 --out /scratch/observations
# 2. train on the GPU (falls back to CPU, and says so, if DirectML is unavailable)
nice -n 19 python tools/forecast-proto/gpu/train_forecast.py --out /scratch/run \
  --observations /scratch/observations --data-source "IEM ASOS hourly METAR archive" \
  --holdout-days 10 --val-days 5 --channels 128 --batch-size 512 --epochs 150 --patience 12 \
  --device directml --export-onnx /scratch/run/learned-conv1d-v1.onnx
# 3. the chain's scoring library over the held-out predictions
cargo run -p wrfcoin-forecast-score --example score_csv -- \
  --predictions /scratch/run/holdout-model.csv --reference /scratch/run/holdout-persistence-trend.csv
# 4. Rust parity through the producer's loader (copy the .onnx and -export.json here first)
cargo test -p wrfcoin-forecast-proto learned
```

CPU and DirectML runs are not bit-identical (the shuffle RNG lives on the device); the
committed file is the DirectML run. The CPU run of the same configuration reached
inner-validation 0.2628 and held-out mean skill 0.4399 (29 epochs, 2,831 ms/epoch vs 1,469 on
DirectML).

### Observations CSV format (`--observations <dir>`)

One file per station, `<station>.csv`, header `time_utc,temperature_c,pressure_hpa,humidity_pct`.
`time_utc` ISO-8601 UTC; rows with sub-hourly times are averaged into their hour; blank =
missing. `tools/forecast-kit fetch-observations` and `gpu/fetch_asos_history.py` both write it.
Loader tests: `python -m unittest discover -s tools/forecast-proto/gpu/tests` — they also open every committed
`.onnx` here and assert its `doc_string`, `metadata_props` and raw bytes carry no station identifiers, personal
names, city names or user paths (a text `git log -p` check shows binaries as "Binary files differ" and would miss them;
`strings models/*.onnx | grep -i <token>` is the manual equivalent).

A chain-history model was also trained on two live contributor stations as an experiment
(`--variables temperature_c,pressure_hpa`, same held-out discipline): mean skill 0.4251 vs the
0.3561 baseline on its held-out window. That artifact is **not published** pending an owner
decision on models trained on a named third party's station data. Source stations will be cited by their on-chain station records once station identifiers no longer carry personal names.

### DirectML notes (AMD GPUs under WSL2)

- The GPU is reached only through `/dev/dxg` + `libd3d12.so` via `torch-directml`; no CUDA, no
  ROCm. `torch_directml` has no `__version__`.
- RNN cells cannot train (`aten::_thnn_fused_gru_cell` has no kernel and its CPU fallback
  raises); the model is a dilated Conv1d stack for that reason.
- `torch.optim.Adam` falls back to the CPU on every step even with `foreach=False`, because
  torch 2.4 updates its first moment with `lerp_` (`aten::lerp.Scalar_out`, no DirectML kernel).
  The trainer's `LerpFreeAdam` uses `mul_/add_/addcmul_/addcdiv_` only; on this run it cut
  3,328 ms/epoch to 2,138 at 256 channels.
- With that change the GPU was 1.9× the CPU at 183 k parameters / batch 512 (1,469 vs 2,831
  ms/epoch); the earlier synthetic benchmark (0.49× at 183 k params, batch 128, torch Adam)
  measured the fallback as much as the hardware. Size the batch up and watch for
  `UserWarning: The operator '<op>' is not currently supported on the DML backend` — that
  warning is the signal the GPU is not actually being used.

## `synthetic-placeholder.onnx`

Reproduce with Python 3.11, torch 2.4.1, numpy 1.26.4 and onnx 1.17.0:

```sh
OMP_NUM_THREADS=4 OPENBLAS_NUM_THREADS=4 MKL_NUM_THREADS=4 timeout 300 python \
  tools/forecast-proto/gpu/train_forecast.py \
  --out tools/forecast-proto/models \
  --export-onnx tools/forecast-proto/models/synthetic-placeholder.onnx
```

Export mode (without `--observations`) sets four intra-op threads, one inter-op thread, uses CPU
explicitly, and stops after at most 200 steps or 240 seconds of training. It never initializes
DirectML or CUDA. The outer timeout bounds the whole invocation to five minutes.

The graphs run locally in tract-onnx 0.23.7; no native ONNX Runtime download is needed.
Cargo.lock pins dependencies but does not vendor their source: populate the Cargo cache with
`cargo fetch --locked` before using offline builds.
