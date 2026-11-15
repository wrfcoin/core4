#!/usr/bin/env python3
"""Train the T+6h station forecast model on the workstation GPU via DirectML.

The workstation GPU is an AMD Radeon RX 6700 XT reachable from WSL2 only
through /dev/dxg + libd3d12.so — no CUDA, no ROCm. The usable path is
torch-directml, which supplies a `privateuseone` device backed by DirectML.

This trains a dilated causal Conv1d on a synthetic hourly station history (24 h of
temperature / pressure / humidity -> the same three variables at T+6h) and
scores it against the `persistence-trend/v1` baseline that the on-chain
prototype actually ships. The on-chain loop does NOT depend on this: if
DirectML is unavailable the script falls back to CPU, and the Rust producer
can optionally load an exported ONNX graph, with persistence-trend fallback.
The --export-onnx mode is CPU-only and never probes the GPU.

Usage:
  # synthetic fixture (CPU vs DirectML timing comparison, unchanged):
  .venv/bin/python tools/forecast-proto/gpu/train_forecast.py --out <dir>
  # REAL hourly station history (see --observations below and models/README.md):
  .venv/bin/python tools/forecast-proto/gpu/train_forecast.py --out <dir> \
      --observations <dir-of-station-csvs> --holdout-days 10 \
      --export-onnx <dir>/learned-conv1d-v1.onnx
"""

import argparse
import csv
import datetime as dt
import json
import math
import os
import platform
import random
import time

import numpy as np

WINDOW_HOURS = 24
HORIZON_HOURS = 6
VARIABLES = ["temperature_c", "pressure_hpa", "humidity_pct"]  # mutated in place by --variables
# Fixed physical scales inside the exported graph, per variable.
SCALES = {"temperature_c": 10.0, "pressure_hpa": 10.0, "humidity_pct": 20.0}


# ---------------------------------------------------------------------------
# Fixture: a synthetic hourly station history
# ---------------------------------------------------------------------------

def synth_history(hours: int, seed: int = 20260909) -> np.ndarray:
    """Hourly (temperature, pressure, humidity) with diurnal + synoptic structure.

    Deterministic given `seed`, and written to CSV next to the report so the
    run is reproducible offline. Three superposed scales: a 24 h diurnal
    cycle, a slow synoptic pressure wave (~3.5 d) that drags temperature and
    anti-correlates humidity, and AR(1) noise. That mix is what makes the task
    non-trivial: pure persistence does well at 6 h, so a model has to learn the
    diurnal phase and the synoptic trend to beat it.
    """
    rng = np.random.default_rng(seed)
    t = np.arange(hours, dtype=np.float64)

    diurnal = 7.0 * np.sin((t % 24) / 24.0 * 2 * math.pi - 1.9)
    synoptic = 5.0 * np.sin(t / (3.5 * 24) * 2 * math.pi)
    temp = 12.0 + diurnal + synoptic

    pres = 1013.0 + 9.0 * np.sin(t / (3.5 * 24) * 2 * math.pi + 0.7)
    hum = 70.0 - 1.6 * (temp - 12.0) + 0.25 * (1013.0 - pres)

    # AR(1) noise so consecutive samples are correlated, as a real sensor is.
    def ar1(scale, rho=0.7):
        e = rng.normal(0.0, scale, size=hours)
        out = np.zeros(hours)
        for i in range(1, hours):
            out[i] = rho * out[i - 1] + e[i]
        return out

    temp += ar1(0.45)
    pres += ar1(0.30)
    hum += ar1(1.2)
    hum = np.clip(hum, 5.0, 100.0)
    return np.stack([temp, pres, hum], axis=1)


def make_windows(series: np.ndarray):
    """Sliding windows: WINDOW_HOURS of history -> the values HORIZON_HOURS later."""
    xs, ys = [], []
    last = len(series) - HORIZON_HOURS
    for i in range(WINDOW_HOURS, last):
        xs.append(series[i - WINDOW_HOURS : i])
        ys.append(series[i + HORIZON_HOURS - 1])
    return np.asarray(xs, dtype=np.float32), np.asarray(ys, dtype=np.float32)


# ---------------------------------------------------------------------------
# Baseline: the same maths the shipped Rust producer uses
# ---------------------------------------------------------------------------

def persistence_trend(window: np.ndarray, damping: float = 0.5) -> np.ndarray:
    """`persistence-trend/v1`, reimplemented for scoring parity.

    Damped OLS slope over the window, added to the last observation. This is
    exactly what `tools/forecast-proto/src/main.rs::forecast_variable` does.
    """
    n = window.shape[0]
    hours = np.arange(n, dtype=np.float64)
    mean_t = hours.mean()
    den = ((hours - mean_t) ** 2).sum()
    out = np.empty(window.shape[1])
    for v in range(window.shape[1]):
        y = window[:, v].astype(np.float64)
        slope = 0.0 if den == 0 else ((hours - mean_t) * (y - y.mean())).sum() / den
        out[v] = y[-1] + damping * slope * HORIZON_HOURS
    return out


def mae_per_var(pred: np.ndarray, truth: np.ndarray) -> np.ndarray:
    return np.abs(pred - truth).mean(axis=0)


# ---------------------------------------------------------------------------
# Real observations: one CSV per station (format in models/README.md)
# ---------------------------------------------------------------------------

def load_observations(directory: str, max_gap_hours: int = 2):
    """Read `<station>.csv` files of hourly `time_utc,temperature_c,pressure_hpa,humidity_pct`.

    Returns `{station: (times, values)}` where `times` is an int64 array of
    unix hours on a contiguous hourly grid and `values` is float64 (hours, 3)
    with NaN where no observation exists. Gaps of at most `max_gap_hours`
    consecutive missing hours are filled by linear interpolation; longer gaps
    stay NaN and every window touching them is dropped later. Rows with
    sub-hourly timestamps are averaged into their hour.
    """
    stations = {}
    for name in sorted(os.listdir(directory)):
        if not name.lower().endswith(".csv"):
            continue
        station = os.path.splitext(name)[0]
        buckets = {}
        with open(os.path.join(directory, name), newline="") as fh:
            reader = csv.DictReader(fh)
            missing = [v for v in ["time_utc"] + list(VARIABLES) if v not in (reader.fieldnames or [])]
            if missing:
                raise SystemExit(f"{name}: missing columns {missing}; expected time_utc,{','.join(VARIABLES)} "
                                 "(use --variables to train on a subset)")
            for row in reader:
                raw = row["time_utc"].strip().replace("Z", "+00:00")
                ts = dt.datetime.fromisoformat(raw)
                if ts.tzinfo is None:
                    ts = ts.replace(tzinfo=dt.timezone.utc)
                hour = int(ts.timestamp() // 3600)
                slot = buckets.setdefault(hour, [[] for _ in VARIABLES])
                for i, var in enumerate(VARIABLES):
                    cell = (row.get(var) or "").strip()
                    if cell:
                        slot[i].append(float(cell))
        if not buckets:
            continue
        first, last = min(buckets), max(buckets)
        hours = np.arange(first, last + 1, dtype=np.int64)
        values = np.full((len(hours), len(VARIABLES)), np.nan)
        for hour, cells in buckets.items():
            for i, xs in enumerate(cells):
                if xs:
                    values[hour - first, i] = float(np.mean(xs))
        # Fill short gaps only.
        for i in range(len(VARIABLES)):
            col = values[:, i]
            ok = ~np.isnan(col)
            if ok.sum() < 2:
                continue
            filled = np.interp(np.arange(len(col)), np.flatnonzero(ok), col[ok])
            # Mark runs of NaN longer than max_gap_hours so they stay NaN.
            bad = ~ok
            run_start = None
            for j in range(len(col) + 1):
                if j < len(col) and bad[j]:
                    run_start = j if run_start is None else run_start
                elif run_start is not None:
                    if j - run_start > max_gap_hours or run_start == 0 or j == len(col):
                        filled[run_start:j] = np.nan
                    run_start = None
            values[:, i] = filled
        stations[station] = (hours, values)
    if not stations:
        raise SystemExit(f"no station CSVs found under {directory}")
    return stations


def make_station_windows(hours, values):
    """Sliding windows over one station's hourly grid; windows touching NaN are dropped.

    Returns (xs, ys, target_hours) with xs (n, WINDOW_HOURS, 3) float32,
    ys (n, 3) float32 and target_hours (n,) int64 (unix hour of the truth).
    """
    xs, ys, ts = [], [], []
    last = len(values) - HORIZON_HOURS
    for i in range(WINDOW_HOURS, last + 1):
        window = values[i - WINDOW_HOURS : i]
        target = values[i + HORIZON_HOURS - 1]
        if np.isnan(window).any() or np.isnan(target).any():
            continue
        xs.append(window)
        ys.append(target)
        ts.append(hours[i + HORIZON_HOURS - 1])
    if not xs:
        return (np.zeros((0, WINDOW_HOURS, len(VARIABLES)), np.float32),
                np.zeros((0, len(VARIABLES)), np.float32), np.zeros((0,), np.int64))
    return (np.asarray(xs, dtype=np.float32), np.asarray(ys, dtype=np.float32),
            np.asarray(ts, dtype=np.int64))


def per_forecast_skill(pred: np.ndarray, truth: np.ndarray, clim: np.ndarray) -> np.ndarray:
    """The receipt rule, per forecast and variable: max(0, 1 - |e| / |e_clim|).

    This is what tools/forecast-proto `skill::score` signs into the
    ForecastVerificationReceipt (window-mean climatology reference), so the
    mean over forecasts is the skill the chain would pay on average.
    """
    err = np.abs(pred - truth)
    clim_err = np.abs(clim - truth)
    with np.errstate(divide="ignore", invalid="ignore"):
        skill = np.where(clim_err <= np.finfo(float).eps,
                         np.where(err <= np.finfo(float).eps, 1.0, 0.0),
                         1.0 - err / clim_err)
    return np.clip(skill, 0.0, 1.0)


def write_pairs_csv(path: str, stations, target_hours, pred: np.ndarray, truth: np.ndarray):
    """`station,target_time_utc,variable,predicted,observed` for forecast-score's score_csv example."""
    with open(path, "w", newline="") as fh:
        writer = csv.writer(fh)
        writer.writerow(["station", "target_time_utc", "variable", "predicted", "observed"])
        for row in range(len(truth)):
            when = dt.datetime.fromtimestamp(int(target_hours[row]) * 3600, dt.timezone.utc)
            for v, var in enumerate(VARIABLES):
                writer.writerow([stations[row], when.strftime("%Y-%m-%dT%H:%M:%SZ"), var,
                                 f"{pred[row, v]:.6f}", f"{truth[row, v]:.6f}"])


# ---------------------------------------------------------------------------
# Model
# ---------------------------------------------------------------------------

def build_model(torch):
    class Forecaster(torch.nn.Module):
        """Dilated causal Conv1d stack + MLP head, predicting the DELTA from
        the last observation.

        A GRU would be the obvious choice, and was the first thing tried — but
        `aten::_thnn_fused_gru_cell` has no DirectML kernel and its CPU
        fallback then fails outright, so an RNN cannot train on this GPU at
        all (recorded in the report). Dilated 1-D convolutions cover the same
        24 h receptive field with ops DirectML implements natively, so the
        whole forward/backward pass stays on the GPU.

        Predicting a residual rather than an absolute value gives the model
        persistence for free and lets its capacity go to the part that is
        actually hard: the diurnal phase and the synoptic trend.
        """

        def __init__(self, n_vars=3, channels=128):
            super().__init__()
            layers = []
            in_ch = n_vars
            # Dilations 1,2,4,8 over a 24-sample window => receptive field 31.
            for dilation in (1, 2, 4, 8):
                layers += [
                    torch.nn.ConstantPad1d((2 * dilation, 0), 0.0),
                    torch.nn.Conv1d(
                        in_ch, channels, kernel_size=3, padding=0, dilation=dilation
                    ),
                    torch.nn.ReLU(),
                ]
                in_ch = channels
            self.tcn = torch.nn.Sequential(*layers)
            self.head = torch.nn.Sequential(
                torch.nn.Linear(channels, 256),
                torch.nn.ReLU(),
                torch.nn.Linear(256, n_vars),
            )

        def forward(self, x):
            # x: (batch, time, vars) -> Conv1d wants (batch, vars, time)
            h = self.tcn(x.transpose(1, 2))
            return x[:, -1, :] + self.head(h[:, :, -1])

    return Forecaster


def export_placeholder(torch, args):
    """CPU-only synthetic placeholder, never evidence of payout-quality skill."""
    torch.set_num_threads(4)
    torch.set_num_interop_threads(1)
    torch.manual_seed(args.seed)
    start = time.monotonic()
    xs, ys = make_windows(synth_history(24 * 30, args.seed))
    # Fixed physical-unit normalization is part of the exported graph. Rust
    # supplies raw (temperature_c, pressure_hpa, humidity_pct), never z-scores.
    class PhysicalForecast(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.register_buffer("center", torch.tensor([12., 1013., 70.]))
            self.register_buffer("scale", torch.tensor([10., 10., 20.]))
            self.model = build_model(torch)(channels=32)

        def forward(self, x):
            return self.model((x - self.center) / self.scale) * self.scale + self.center

    model = PhysicalForecast().cpu()
    opt = torch.optim.Adam(model.parameters(), lr=2e-3, foreach=False)
    inputs, targets = torch.from_numpy(xs), torch.from_numpy(ys)
    steps = 0
    for step in range(200):
        if time.monotonic() - start >= 240:
            break
        idx = torch.randint(len(inputs), (32,))
        opt.zero_grad()
        loss = ((model(inputs[idx]) - targets[idx]) / model.scale).square().mean()
        loss.backward()
        opt.step()
        steps = step + 1
    model.eval()
    sample = inputs[:1]
    with torch.no_grad():
        expected = model(sample).numpy().tolist()
    os.makedirs(os.path.dirname(os.path.abspath(args.export_onnx)), exist_ok=True)
    torch.onnx.export(model, sample, args.export_onnx, input_names=["window"],
                      output_names=["forecast"], opset_version=17,
                      do_constant_folding=True)
    import onnx
    graph = onnx.load(args.export_onnx)
    onnx.checker.check_model(graph)
    graph.doc_string = "SYNTHETIC-TRAINED PLACEHOLDER; NOT FOR PAYOUTS. CPU, 24h -> T+6h."
    onnx.save(graph, args.export_onnx)
    report = {"purpose": "synthetic-trained placeholder; not for payouts",
              "device": "cpu", "torch_threads": torch.get_num_threads(),
              "steps": steps, "seconds": time.monotonic() - start,
              "seed": args.seed, "opset": 17, "input_shape": [1, 24, 3],
              "output_shape": [1, 3], "variables": VARIABLES,
              "bytes": os.path.getsize(args.export_onnx),
              "input": sample.numpy().tolist(), "expected_output": expected}
    with open(os.path.join(args.out, "placeholder-export.json"), "w") as fh:
        json.dump(report, fh, indent=2)
    print(json.dumps({k: v for k, v in report.items() if k not in ("input", "expected_output")}, indent=2))


def train(torch, device, xs_tr, ys_tr, xs_te, epochs, batch_size, seed, label, channels=128):
    torch.manual_seed(seed)
    Forecaster = build_model(torch)
    model = Forecaster(channels=channels).to(device)
    # foreach=False: the fused multi-tensor path calls aten::lerp.Scalar_out,
    # which has no DirectML kernel and forces a CPU round-trip on EVERY step.
    opt = torch.optim.Adam(model.parameters(), lr=2e-3, foreach=False)
    lossf = torch.nn.SmoothL1Loss()

    xt = torch.from_numpy(xs_tr).to(device)
    yt = torch.from_numpy(ys_tr).to(device)
    n = xt.shape[0]
    print(f"  [{label}] training on {n} windows, params="
          f"{sum(p.numel() for p in model.parameters())}")

    # One warm-up step so kernel compilation is not billed to epoch 1.
    model(xt[:batch_size])
    if hasattr(torch, "cuda") and device.type == "cuda":
        torch.cuda.synchronize()

    start = time.perf_counter()
    history = []
    for epoch in range(epochs):
        model.train()
        perm = torch.randperm(n, device=device)
        total = 0.0
        for i in range(0, n, batch_size):
            idx = perm[i : i + batch_size]
            opt.zero_grad()
            loss = lossf(model(xt[idx]), yt[idx])
            loss.backward()
            opt.step()
            total += float(loss.detach().to("cpu")) * len(idx)
        history.append(total / n)
    elapsed = time.perf_counter() - start

    model.eval()
    with torch.no_grad():
        preds = model(torch.from_numpy(xs_te).to(device)).to("cpu").numpy()
    print(f"  [{label}] {epochs} epochs in {elapsed:.2f}s "
          f"({elapsed / epochs * 1000:.0f} ms/epoch), final loss {history[-1]:.4f}")
    return preds, elapsed, history



# ---------------------------------------------------------------------------
# Observed-data training (the mode behind --observations)
# ---------------------------------------------------------------------------

def build_physical_window_model(torch, channels: int):
    class PhysicalWindowForecast(torch.nn.Module):
        """Raw physical units in and out; normalization lives inside the graph.

        Each input window is centred on its own per-variable mean and scaled
        by a fixed physical scale before the Conv1d stack, and the stack's
        residual is added back to the last raw observation. Centring on the
        window mean (rather than a fixed climatological centre) makes the
        model indifferent to a station's level offset: the archive this is
        trained on reports sea-level pressure while the chain's stations
        report their own sensor's pressure, and both are then the same input.
        The Rust producer supplies raw (temperature_c, pressure_hpa,
        humidity_pct) and reads raw units back, exactly as for the placeholder.
        """

        def __init__(self):
            super().__init__()
            self.register_buffer("scale", torch.tensor([SCALES[v] for v in VARIABLES]))
            self.model = build_model(torch)(n_vars=len(VARIABLES), channels=channels)

        def forward(self, x):
            center = x.mean(dim=1, keepdim=True)
            z = (x - center) / self.scale
            delta = self.model(z) - z[:, -1, :]
            return x[:, -1, :] + delta * self.scale

    return PhysicalWindowForecast



class LerpFreeAdam:
    """Adam written with mul_/add_/addcmul_ only, for DirectML.

    torch 2.4's `torch.optim.Adam` updates its first moment with
    `exp_avg.lerp_(grad, 1 - beta1)` even when `foreach=False`, and
    `aten::lerp.Scalar_out` has no DirectML kernel: every step round-trips
    every parameter through the CPU (the `dml_cpu_fallback` warning). This is
    the textbook Adam update (Kingma & Ba, with decoupled weight decay) using
    ops DirectML implements natively, so the optimizer step stays on the GPU.
    """

    def __init__(self, torch, params, lr=1e-3, betas=(0.9, 0.999), eps=1e-8, weight_decay=0.0):
        self.torch = torch
        self.params = [p for p in params if p.requires_grad]
        self.lr, self.betas, self.eps, self.weight_decay = lr, betas, eps, weight_decay
        self.step_count = 0
        self.exp_avg = [torch.zeros_like(p) for p in self.params]
        self.exp_avg_sq = [torch.zeros_like(p) for p in self.params]

    def zero_grad(self):
        for p in self.params:
            p.grad = None

    @property
    def param_groups(self):
        return [{"lr": self.lr, "params": self.params}]

    def step(self):
        self.step_count += 1
        b1, b2 = self.betas
        bias1 = 1.0 - b1 ** self.step_count
        bias2 = 1.0 - b2 ** self.step_count
        step_size = self.lr / bias1
        with self.torch.no_grad():
            for p, m, v in zip(self.params, self.exp_avg, self.exp_avg_sq):
                if p.grad is None:
                    continue
                if self.weight_decay:
                    p.mul_(1.0 - self.lr * self.weight_decay)
                m.mul_(b1).add_(p.grad, alpha=1.0 - b1)
                v.mul_(b2).addcmul_(p.grad, p.grad, value=1.0 - b2)
                denom = (v / bias2).sqrt_().add_(self.eps)
                p.addcdiv_(m, denom, value=-step_size)


def train_observed(torch, args, dml_device, report):
    """Train on real hourly station history with a chronological held-out window."""
    import hashlib

    t_start = time.perf_counter()
    stations = load_observations(args.observations, args.max_gap_hours)
    coverage = {}
    all_hours = []
    for name, (hours, values) in stations.items():
        complete = int((~np.isnan(values).any(axis=1)).sum())
        coverage[name] = {"hours": int(len(hours)), "complete_hours": complete,
                          "missing_hours": int(len(hours) - complete),
                          "first": dt.datetime.fromtimestamp(int(hours[0]) * 3600, dt.timezone.utc).isoformat(),
                          "last": dt.datetime.fromtimestamp(int(hours[-1]) * 3600, dt.timezone.utc).isoformat()}
        all_hours.append(hours)
    last_hour = int(max(h[-1] for h in all_hours))
    first_hour = int(min(h[0] for h in all_hours))
    holdout_start = last_hour - args.holdout_days * 24 + 1
    val_start = holdout_start - args.val_days * 24
    report["data"] = {
        "source": args.data_source,
        # Basename only: the record is committed next to the model and must not carry a user's paths.
        "observations_dir": os.path.basename(os.path.normpath(args.observations)),
        "stations": sorted(stations),
        "station_count": len(stations),
        "window_first": dt.datetime.fromtimestamp(first_hour * 3600, dt.timezone.utc).isoformat(),
        "window_last": dt.datetime.fromtimestamp(last_hour * 3600, dt.timezone.utc).isoformat(),
        "holdout_start": dt.datetime.fromtimestamp(holdout_start * 3600, dt.timezone.utc).isoformat(),
        "holdout_days": args.holdout_days,
        "inner_validation_days": args.val_days,
        "max_gap_hours_interpolated": args.max_gap_hours,
        "coverage": coverage,
    }
    total_hours = sum(c["hours"] for c in coverage.values())
    total_complete = sum(c["complete_hours"] for c in coverage.values())
    print(f"observations: {len(stations)} stations, {total_hours} station-hours "
          f"({total_complete} complete), {report['data']['window_first']} .. {report['data']['window_last']}")
    print(f"held-out window: targets from {report['data']['holdout_start']} ({args.holdout_days} d); "
          f"inner validation: last {args.val_days} d before it")

    fit_x, fit_y, val_x, val_y = [], [], [], []
    test_x, test_y, test_t, test_s = [], [], [], []
    for name, (hours, values) in stations.items():
        xs, ys, ts = make_station_windows(hours, values)
        test = ts >= holdout_start
        val = (ts >= val_start) & ~test
        fit = ts < val_start
        fit_x.append(xs[fit]); fit_y.append(ys[fit])
        val_x.append(xs[val]); val_y.append(ys[val])
        test_x.append(xs[test]); test_y.append(ys[test]); test_t.append(ts[test])
        test_s.extend([name] * int(test.sum()))
    fit_x, fit_y = np.concatenate(fit_x), np.concatenate(fit_y)
    val_x, val_y = np.concatenate(val_x), np.concatenate(val_y)
    test_x, test_y, test_t = np.concatenate(test_x), np.concatenate(test_y), np.concatenate(test_t)
    test_s = np.asarray(test_s)
    report["windows"] = {"fit": int(len(fit_x)), "inner_validation": int(len(val_x)), "held_out": int(len(test_x))}
    print(f"windows: {len(fit_x)} fit / {len(val_x)} inner-validation / {len(test_x)} held-out")
    if len(fit_x) == 0 or len(val_x) == 0 or len(test_x) == 0:
        raise SystemExit("not enough complete windows for fit/validation/held-out; check coverage and --holdout-days")

    # -- references on the held-out window (the producer's maths) -----------
    base_pred = np.stack([persistence_trend(w) for w in test_x])
    clim_pred = test_x.mean(axis=1)
    base_mae = mae_per_var(base_pred, test_y)
    clim_mae = mae_per_var(clim_pred, test_y)

    # -- device ------------------------------------------------------------
    device = torch.device("cpu")
    device_label = "cpu"
    if args.device != "cpu" and dml_device is not None:
        device, device_label = dml_device, "directml"
    elif args.device == "directml":
        print("DirectML requested but unavailable; training on CPU")
    report["training_device"] = device_label

    torch.manual_seed(args.seed)
    model = build_physical_window_model(torch, args.channels)().to(device)
    scale = model.scale.to(device)
    n_params = sum(p.numel() for p in model.parameters())
    opt = LerpFreeAdam(torch, model.parameters(), lr=args.lr, weight_decay=args.weight_decay)
    xf, yf = torch.from_numpy(fit_x).to(device), torch.from_numpy(fit_y).to(device)
    xv, yv = torch.from_numpy(val_x).to(device), torch.from_numpy(val_y).to(device)
    print(f"  [{device_label}] training {n_params} params on {len(xf)} windows, batch {args.batch_size}, "
          f"up to {args.epochs} epochs, early stop patience {args.patience}")

    def normalized_mae(x, y):
        model.eval()
        with torch.no_grad():
            out = []
            for i in range(0, len(x), 4096):
                out.append(((model(x[i : i + 4096]) - y[i : i + 4096]).abs() / scale).mean(dim=0))
            return float(torch.stack(out).mean().to("cpu"))

    model(xf[: args.batch_size])  # warm-up: kernel compilation is not billed to epoch 1
    start = time.perf_counter()
    best_val, best_state, best_epoch, bad_epochs = float("inf"), None, -1, 0
    history = []
    n = len(xf)
    for epoch in range(args.epochs):
        model.train()
        perm = torch.randperm(n, device=device)
        total = 0.0
        for i in range(0, n, args.batch_size):
            idx = perm[i : i + args.batch_size]
            opt.zero_grad()
            loss = torch.nn.functional.smooth_l1_loss(model(xf[idx]) / scale, yf[idx] / scale)
            loss.backward()
            opt.step()
            total += float(loss.detach().to("cpu")) * len(idx)
        val_mae = normalized_mae(xv, yv)
        history.append({"epoch": epoch + 1, "train_loss": total / n, "val_normalized_mae": val_mae})
        if val_mae < best_val - 1e-5:
            best_val, best_epoch, bad_epochs = val_mae, epoch + 1, 0
            best_state = {k: v.detach().to("cpu").clone() for k, v in model.state_dict().items()}
        else:
            bad_epochs += 1
        if (epoch + 1) % 5 == 0 or epoch == 0:
            print(f"  [{device_label}] epoch {epoch + 1}: train loss {total / n:.4f}, "
                  f"inner-val normalized MAE {val_mae:.4f}")
        if bad_epochs >= args.patience:
            print(f"  [{device_label}] early stop at epoch {epoch + 1} (best epoch {best_epoch})")
            break
    elapsed = time.perf_counter() - start
    epochs_run = len(history)
    model.load_state_dict(best_state)
    print(f"  [{device_label}] {epochs_run} epochs in {elapsed:.2f}s ({elapsed / epochs_run * 1000:.0f} ms/epoch); "
          f"best inner-val normalized MAE {best_val:.4f} at epoch {best_epoch}")
    report["training"] = {"device": device_label, "params": n_params, "epochs_run": epochs_run,
                          "best_epoch": best_epoch, "seconds": round(elapsed, 3),
                          "ms_per_epoch": round(elapsed / epochs_run * 1000, 1),
                          "best_inner_val_normalized_mae": best_val, "lr": args.lr,
                          "weight_decay": args.weight_decay, "batch_size": args.batch_size,
                          "channels": args.channels, "seed": args.seed, "history": history}

    # -- held-out evaluation: ONE pass, nothing tuned on it -------------------
    model.eval()
    with torch.no_grad():
        preds = []
        xt = torch.from_numpy(test_x).to(device)
        for i in range(0, len(xt), 4096):
            preds.append(model(xt[i : i + 4096]).to("cpu").numpy())
    model_pred = np.concatenate(preds)
    model_mae = mae_per_var(model_pred, test_y)

    def summarize(pred, label):
        m = mae_per_var(pred, test_y)
        pf = per_forecast_skill(pred, test_y, clim_pred)
        return {
            "mae": dict(zip(VARIABLES, m.round(4).tolist())),
            "rmse": dict(zip(VARIABLES, np.sqrt(((pred - test_y) ** 2).mean(axis=0)).round(4).tolist())),
            "aggregate_skill_vs_climatology": dict(zip(VARIABLES, np.clip(1.0 - m / clim_mae, 0.0, 1.0).round(4).tolist())),
            "mean_per_forecast_skill": dict(zip(VARIABLES, pf.mean(axis=0).round(4).tolist())),
            "mean_skill_score": float(pf.mean(axis=1).mean().round(4)),
            "per_station_mean_skill_score": {
                s: float(pf[test_s == s].mean(axis=1).mean().round(4)) for s in sorted(set(test_s))
            },
            "label": label,
        }

    held_out = {
        "climatology_mae": dict(zip(VARIABLES, clim_mae.round(4).tolist())),
        "persistence_trend_v1": summarize(base_pred, "persistence-trend/v1 (shipped fallback)"),
        "learned_conv1d_v1": summarize(model_pred, "learned-conv1d/v1 (this run)"),
    }
    compare_pred = None
    if args.compare_onnx:
        # An existing graph (e.g. the ASOS-trained model) on the SAME held-out windows.
        # Its input variables come from the -export.json next to it (default: the
        # three canonical ones); variables this dataset lacks are filled with the
        # constant from --fill-missing, and only the selected variables are scored.
        import onnxruntime as ort
        record_path = os.path.splitext(args.compare_onnx)[0] + "-export.json"
        model_vars = ["temperature_c", "pressure_hpa", "humidity_pct"]
        if os.path.exists(record_path):
            with open(record_path) as fh:
                model_vars = json.load(fh).get("variables", model_vars)
        fills = dict(item.split("=") for item in (args.fill_missing or []))
        columns = []
        for v in model_vars:
            if v in VARIABLES:
                columns.append(("col", VARIABLES.index(v)))
            elif v in fills:
                columns.append(("const", float(fills[v])))
            else:
                raise SystemExit(f"--compare-onnx needs variable {v}; pass --fill-missing {v}=<value>")
        sess = ort.InferenceSession(args.compare_onnx, providers=["CPUExecutionProvider"])
        name = sess.get_inputs()[0].name
        outs = []
        for w in test_x:
            x = np.empty((1, WINDOW_HOURS, len(model_vars)), np.float32)
            for j, (kind, val) in enumerate(columns):
                x[0, :, j] = w[:, int(val)] if kind == "col" else val
            outs.append(sess.run(None, {name: x})[0][0])
        full = np.asarray(outs, dtype=np.float32)
        compare_pred = np.stack([full[:, model_vars.index(v)] for v in VARIABLES], axis=1)
        held_out["compare_onnx"] = summarize(compare_pred, f"{os.path.basename(args.compare_onnx)} (existing graph, same held-out window)")
        held_out["compare_onnx"]["path"] = os.path.basename(args.compare_onnx)
        held_out["compare_onnx"]["model_variables"] = model_vars
        held_out["compare_onnx"]["filled_variables"] = {v: fills[v] for v in model_vars if v not in VARIABLES}
        print(f"held-out mean per-forecast skill_score of {os.path.basename(args.compare_onnx)} on the same window: "
              f"{held_out['compare_onnx']['mean_skill_score']:.4f} (per variable {held_out['compare_onnx']['mean_per_forecast_skill']}; "
              f"filled {held_out['compare_onnx']['filled_variables']})")
    held_out["learned_conv1d_v1"]["skill_vs_persistence_trend"] = dict(
        zip(VARIABLES, np.clip(1.0 - model_mae / base_mae, 0.0, 1.0).round(4).tolist()))
    ratio = (held_out["learned_conv1d_v1"]["mean_skill_score"]
             / max(held_out["persistence_trend_v1"]["mean_skill_score"], 1e-9))
    held_out["reward_ratio_vs_persistence_trend"] = round(ratio, 3)
    held_out["beats_baseline"] = bool(
        held_out["learned_conv1d_v1"]["mean_skill_score"] > held_out["persistence_trend_v1"]["mean_skill_score"]
        and all(model_mae < base_mae))
    report["held_out"] = held_out
    print("held-out MAE (model / persistence-trend / climatology):")
    for v, var in enumerate(VARIABLES):
        print(f"  {var:<15} {model_mae[v]:8.4f} / {base_mae[v]:8.4f} / {clim_mae[v]:8.4f}")
    print("held-out aggregate skill vs climatology (1 - MAE/MAE_clim): "
          f"model {held_out['learned_conv1d_v1']['aggregate_skill_vs_climatology']} | "
          f"persistence-trend {held_out['persistence_trend_v1']['aggregate_skill_vs_climatology']}")
    print("held-out mean per-forecast skill_score (the receipt rule): "
          f"model {held_out['learned_conv1d_v1']['mean_skill_score']:.4f} | "
          f"persistence-trend {held_out['persistence_trend_v1']['mean_skill_score']:.4f} | "
          f"ratio {ratio:.3f} | beats baseline on every variable: {held_out['beats_baseline']}")

    # -- pairs CSVs for forecast-score's score_csv example -------------------
    csv_paths = {}
    pairs = [("model", model_pred), ("persistence-trend", base_pred), ("climatology", clim_pred)]
    if compare_pred is not None:
        pairs.append(("compare", compare_pred))
    for label, pred in pairs:
        path = os.path.join(args.out, f"holdout-{label}.csv")
        write_pairs_csv(path, test_s, test_t, pred, test_y)
        csv_paths[label] = path
    report["held_out_pairs_csv"] = csv_paths
    print(f"wrote held-out pairs CSVs: {', '.join(csv_paths.values())}")

    # -- ONNX export + CPU parity record ------------------------------------
    if args.export_onnx:
        cpu_model = build_physical_window_model(torch, args.channels)()
        cpu_model.load_state_dict(best_state)
        cpu_model.eval()
        sample = torch.from_numpy(test_x[:1])
        with torch.no_grad():
            expected = cpu_model(sample).numpy()
        os.makedirs(os.path.dirname(os.path.abspath(args.export_onnx)), exist_ok=True)
        torch.onnx.export(cpu_model, sample, args.export_onnx, input_names=["window"],
                          output_names=["forecast"], opset_version=17, do_constant_folding=True)
        import onnx
        graph = onnx.load(args.export_onnx)
        onnx.checker.check_model(graph)
        graph.doc_string = (
            f"learned-conv1d/v1 trained on REAL hourly station observations ({args.data_source}); "
            f"{len(stations)} stations, {report['data']['window_first'][:10]}..{report['data']['window_last'][:10]}, "
            f"held-out {args.holdout_days} d, seed {args.seed}, device {device_label}. "
            f"Input [1,24,{len(VARIABLES)}] raw {','.join(VARIABLES)} oldest first; output [1,{len(VARIABLES)}] at T+6h. "
            "Not validated for payouts."
        )
        onnx.save(graph, args.export_onnx)
        parity = {"torch_cpu_vs_onnxruntime_max_abs_delta": None}
        try:
            import onnxruntime as ort
            sess = ort.InferenceSession(args.export_onnx, providers=["CPUExecutionProvider"])
            # The graph is exported with a fixed [1,24,3] input (what the Rust
            # producer feeds), so onnxruntime is checked one window at a time.
            check = test_x[:64]
            with torch.no_grad():
                ref = cpu_model(torch.from_numpy(check)).numpy()
            got = np.concatenate([sess.run(None, {"window": check[i : i + 1]})[0] for i in range(len(check))])
            parity["torch_cpu_vs_onnxruntime_max_abs_delta"] = float(np.abs(ref - got).max())
            parity["windows_checked"] = int(len(check))
            parity["onnxruntime"] = ort.__version__
        except Exception as exc:  # noqa: BLE001 — parity via onnxruntime is advisory; Rust/tract is the gate
            parity["onnxruntime_error"] = f"{type(exc).__name__}: {exc}"
        sha256 = hashlib.sha256(open(args.export_onnx, "rb").read()).hexdigest()
        export = {
            "purpose": "learned-conv1d/v1 trained on real hourly station observations; not validated for payouts",
            "model_id": "learned-conv1d/v1",
            "trained_on_device": device_label,
            "exported_on": "cpu",
            "data": {k: v for k, v in report["data"].items() if k != "coverage"},
            "windows": report["windows"],
            "training": {k: v for k, v in report["training"].items() if k != "history"},
            "held_out": {k: v for k, v in held_out.items()},
            "seed": args.seed, "opset": 17, "input_shape": [1, WINDOW_HOURS, len(VARIABLES)],
            "output_shape": [1, len(VARIABLES)], "variables": VARIABLES,
            # Prefixed so secret scanners (detect-secrets HexHighEntropyString) do not
            # mistake a file digest for a key.
            "bytes": os.path.getsize(args.export_onnx), "sha256": f"sha256:{sha256}",
            "rust_parity_tolerance_abs": 0.005,
            "python_parity": parity,
            "input": sample.numpy().tolist(), "expected_output": expected.tolist(),
        }
        export_path = os.path.splitext(args.export_onnx)[0] + "-export.json"
        with open(export_path, "w") as fh:
            json.dump(export, fh, indent=2)
        report["export"] = {k: v for k, v in export.items() if k not in ("input", "expected_output", "data", "windows", "training", "held_out")}
        report["export"]["path"] = os.path.abspath(args.export_onnx)
        report["export"]["parity_json"] = os.path.abspath(export_path)
        print(f"exported ONNX {args.export_onnx} ({export['bytes']} bytes, sha256 {sha256[:16]}…); "
              f"torch-cpu vs onnxruntime max |Δ| = {parity.get('torch_cpu_vs_onnxruntime_max_abs_delta')}; "
              f"parity record {export_path}")

    report["wall_seconds"] = round(time.perf_counter() - t_start, 3)
    out_path = os.path.join(args.out, "observed-training-report.json")
    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2)
    print(f"\nwrote {out_path} (wall {report['wall_seconds']}s)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--hours", type=int, default=24 * 240)
    ap.add_argument("--epochs", type=int, default=30)
    ap.add_argument("--batch-size", type=int, default=128)
    ap.add_argument("--seed", type=int, default=20260909)
    ap.add_argument("--channels", type=int, default=128)
    ap.add_argument("--export-onnx", help="Without --observations: export the CPU-only synthetic placeholder "
                    "(200 steps, four threads max). With --observations: export the trained model and its parity record.")
    ap.add_argument("--observations", help="Directory of per-station hourly CSVs (time_utc,temperature_c,pressure_hpa,"
                    "humidity_pct); switches to observed-data training with a chronological held-out window")
    ap.add_argument("--data-source", default="unspecified", help="Provenance label written into the report and the ONNX doc_string")
    ap.add_argument("--holdout-days", type=int, default=10, help="Final days whose targets are held out (never trained or tuned on)")
    ap.add_argument("--val-days", type=int, default=5, help="Days before the held-out window used for early stopping")
    ap.add_argument("--max-gap-hours", type=int, default=2, help="Longest gap filled by linear interpolation")
    ap.add_argument("--patience", type=int, default=10, help="Early-stopping patience in epochs (observed mode)")
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--weight-decay", type=float, default=1e-5)
    ap.add_argument("--variables", help="Comma-separated subset of temperature_c,pressure_hpa,humidity_pct to train and "
                    "score on (stations that do not report a variable); the exported graph then has that many channels")
    ap.add_argument("--compare-onnx", help="Observed mode: also score this existing ONNX graph on the same held-out window")
    ap.add_argument("--fill-missing", action="append", metavar="VAR=VALUE",
                    help="Constant for a --compare-onnx input variable this dataset does not have (repeatable)")
    ap.add_argument("--device", choices=["auto", "cpu", "directml"], default="auto",
                    help="Observed mode: auto picks DirectML when it works, else CPU")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)

    random.seed(args.seed)
    np.random.seed(args.seed)

    import torch

    if args.export_onnx and not args.observations:
        export_placeholder(torch, args)
        return  # Never probe or initialize DirectML/CUDA in export mode.

    report = {
        "platform": platform.platform(),
        "python": platform.python_version(),
        "torch": torch.__version__,
        "window_hours": WINDOW_HOURS,
        "horizon_hours": HORIZON_HOURS,
        "variables": VARIABLES,
        "epochs": args.epochs,
        "batch_size": args.batch_size,
        "channels": args.channels,
        "architecture": "dilated causal Conv1d x4 (dilations 1,2,4,8) + MLP head, residual on last obs",
        "architecture_note": (
            "A GRU was tried first and CANNOT train on this GPU: "
            "aten::_thnn_fused_gru_cell has no DirectML kernel, and its CPU "
            "fallback then raises NotImplementedError. Dilated Conv1d covers "
            "the same 24 h receptive field with ops DirectML implements "
            "natively, keeping the whole forward/backward pass on the GPU."
        ),
    }

    # -- device ------------------------------------------------------------
    dml_device = None
    try:
        import torch_directml

        report["torch_directml"] = getattr(torch_directml, "__version__", "0.2.5.dev240914")
        report["directml_available"] = bool(torch_directml.is_available())
        report["directml_device_count"] = int(torch_directml.device_count())
        if torch_directml.is_available():
            dml_device = torch_directml.device()
            report["directml_device_name"] = torch_directml.device_name(0)
            # The smoke test the owner asked for.
            probe = torch.ones(1).to(dml_device)
            report["directml_smoke_test"] = {
                "tensor_device": str(probe.device),
                "roundtrip_value": float(probe.to("cpu").item()),
                "ok": float(probe.to("cpu").item()) == 1.0,
            }
            print(f"DirectML device: {report['directml_device_name']} "
                  f"({report['directml_smoke_test']['tensor_device']}), "
                  f"round-trip ok = {report['directml_smoke_test']['ok']}")
    except Exception as exc:  # noqa: BLE001 — any failure means "fall back to CPU"
        report["directml_error"] = f"{type(exc).__name__}: {exc}"
        print(f"DirectML unavailable ({type(exc).__name__}: {exc}) — CPU only")

    if args.observations:
        if args.variables:
            selected = [v.strip() for v in args.variables.split(",") if v.strip()]
            unknown = [v for v in selected if v not in SCALES]
            if unknown or not selected:
                raise SystemExit(f"--variables must be a non-empty subset of {sorted(SCALES)}; got {selected}")
            VARIABLES[:] = selected
            report["variables"] = list(VARIABLES)
        if args.device == "cpu":
            dml_device = None
        train_observed(torch, args, dml_device, report)
        return

    # -- data --------------------------------------------------------------
    series = synth_history(args.hours, seed=args.seed)
    csv_path = os.path.join(args.out, "station-history.csv")
    with open(csv_path, "w") as fh:
        fh.write("hour_index," + ",".join(VARIABLES) + "\n")
        for i, row in enumerate(series):
            fh.write(f"{i}," + ",".join(f"{v:.4f}" for v in row) + "\n")
    report["fixture_csv"] = csv_path
    report["fixture_rows"] = int(series.shape[0])

    xs, ys = make_windows(series)
    split = int(len(xs) * 0.8)
    xs_tr, ys_tr, xs_te, ys_te = xs[:split], ys[:split], xs[split:], ys[split:]
    report["train_windows"] = int(len(xs_tr))
    report["test_windows"] = int(len(xs_te))
    print(f"windows: {len(xs_tr)} train / {len(xs_te)} test")

    # -- baseline ----------------------------------------------------------
    base_pred = np.stack([persistence_trend(w) for w in xs_te])
    base_mae = mae_per_var(base_pred, ys_te)
    clim_pred = np.stack([w.mean(axis=0) for w in xs_te])
    clim_mae = mae_per_var(clim_pred, ys_te)
    report["baseline_persistence_trend_mae"] = dict(zip(VARIABLES, base_mae.round(4).tolist()))
    report["climatology_mae"] = dict(zip(VARIABLES, clim_mae.round(4).tolist()))
    print(f"persistence-trend/v1 MAE: {dict(zip(VARIABLES, base_mae.round(3)))}")

    # -- train: CPU then DirectML -----------------------------------------
    results = {}
    cpu_pred, cpu_time, _ = train(
        torch, torch.device("cpu"), xs_tr, ys_tr, xs_te,
        args.epochs, args.batch_size, args.seed, "cpu", args.channels,
    )
    results["cpu"] = {"seconds": round(cpu_time, 3)}

    gpu_pred = None
    if dml_device is not None:
        try:
            gpu_pred, gpu_time, _ = train(
                torch, dml_device, xs_tr, ys_tr, xs_te,
                args.epochs, args.batch_size, args.seed, "directml", args.channels,
            )
            results["directml"] = {
                "seconds": round(gpu_time, 3),
                "speedup_vs_cpu": round(cpu_time / gpu_time, 2),
            }
        except Exception as exc:  # noqa: BLE001
            results["directml"] = {"error": f"{type(exc).__name__}: {exc}"}
            print(f"  [directml] training failed: {type(exc).__name__}: {exc}")

    report["timing"] = results

    # -- score -------------------------------------------------------------
    def skill(pred):
        m = mae_per_var(pred, ys_te)
        return {
            "mae": dict(zip(VARIABLES, m.round(4).tolist())),
            # Same definition the on-chain receipt uses: 1 - MAE/MAE_reference,
            # against the window-mean climatology.
            "skill_vs_climatology": dict(
                zip(VARIABLES, np.clip(1.0 - m / clim_mae, 0.0, 1.0).round(4).tolist())
            ),
            "skill_vs_persistence_trend": dict(
                zip(VARIABLES, np.clip(1.0 - m / base_mae, 0.0, 1.0).round(4).tolist())
            ),
        }

    report["gru_cpu"] = skill(cpu_pred)
    if gpu_pred is not None:
        report["gru_directml"] = skill(gpu_pred)
        report["cpu_vs_directml_max_abs_pred_delta"] = float(
            np.abs(cpu_pred - gpu_pred).max()
        )
    report["baseline_skill_vs_climatology"] = dict(
        zip(VARIABLES, np.clip(1.0 - base_mae / clim_mae, 0.0, 1.0).round(4).tolist())
    )

    out_path = os.path.join(args.out, "gpu-training-report.json")
    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2)
    print(json.dumps(report, indent=2))
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
