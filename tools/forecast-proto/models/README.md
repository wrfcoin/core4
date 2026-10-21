# Synthetic placeholder — not for payouts

`synthetic-placeholder.onnx` is a small dilated causal Conv1d trained on the
trainer's synthetic station series. It is a runtime integration fixture, not a
validated weather forecast model. No observed-data or payout-quality claim is made.

Input: float32 `[1,24,3]`, 24 consecutive hourly samples, in
`temperature_c, pressure_hpa, humidity_pct` order, oldest first. Output: float32
`[1,3]`, the same variable order at six hours after the last input. Normalization
and physical-unit conversion are inside the graph. Opset: 17. Model size: 87,640
bytes. Export metadata and one CPU prediction for Rust parity are in
`placeholder-export.json`.

Reproduce with Python 3.11, torch 2.4.1, numpy 1.26.4 and onnx 1.17.0:

```sh
OMP_NUM_THREADS=4 OPENBLAS_NUM_THREADS=4 MKL_NUM_THREADS=4 timeout 300 python \
  tools/forecast-proto/gpu/train_forecast.py \
  --out tools/forecast-proto/models \
  --export-onnx tools/forecast-proto/models/synthetic-placeholder.onnx
```

Export mode sets four intra-op threads, one inter-op thread, uses CPU explicitly,
and stops after at most 200 steps or 240 seconds of training. It never initializes
DirectML or CUDA. The outer timeout bounds the whole invocation to five minutes.

To opt into inference, add `--model learned-conv1d/v1 --onnx-path <model.onnx>`
to `forecast-proto run`, or set `WRFCOIN_FORECAST_MODEL` and
`WRFCOIN_FORECAST_ONNX`. The default remains `persistence-trend/v1`. A missing,
invalid or incompatible model, non-finite output, or unsuitable hourly window
falls back to persistence and logs the fallback once per process. Forecast,
manifest and run-summary model IDs identify the model that actually ran.

The graph runs locally in tract-onnx 0.23.7; no native ONNX Runtime download is
needed. Cargo.lock pins dependencies but does not vendor their source: populate
the Cargo cache with `cargo fetch --locked` before using offline builds.
