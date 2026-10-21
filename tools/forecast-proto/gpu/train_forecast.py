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
  .venv/bin/python tools/forecast-proto/gpu/train_forecast.py --out <dir>
"""

import argparse
import json
import math
import os
import platform
import random
import time

import numpy as np

WINDOW_HOURS = 24
HORIZON_HOURS = 6
VARIABLES = ["temperature_c", "pressure_hpa", "humidity_pct"]


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--hours", type=int, default=24 * 240)
    ap.add_argument("--epochs", type=int, default=30)
    ap.add_argument("--batch-size", type=int, default=128)
    ap.add_argument("--seed", type=int, default=20260909)
    ap.add_argument("--channels", type=int, default=128)
    ap.add_argument("--export-onnx", help="Export a CPU-only synthetic placeholder (200 steps, four threads max)")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)

    random.seed(args.seed)
    np.random.seed(args.seed)

    import torch

    if args.export_onnx:
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
