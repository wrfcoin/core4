"""Command line: `python -m forecaster_worker [run|once|address|status] [--dry-run]`."""
import argparse
import json
import sys
import time

from . import chain, gfs, wallet as wallet_mod
from .site import Site, SiteError
from .store import Store
from .worker import Config, run_cycle

UNITS = {"temperature_c": ("degC", 100), "pressure_hpa": ("hPa", 100)}


def _show(report, variable: str) -> None:
    unit, scale = UNITS[variable]
    for line in report.lines:
        print(line)
    print(f"Stations: {', '.join(report.stations) or 'none'}. New readings: {report.new_readings}. "
          f"New bias samples: {report.new_samples}.")
    for f in report.forecasts:
        verb = "WOULD SEND" if f["dry_run"] else "SENT"
        print(f"{verb} {f['station']} +{f['lead']}h {f['variable']} = {f['value']} {unit} "
              f"(model {f['model_raw'] / scale:.2f}, correction {f['correction'] / scale:+.2f} from {f['samples']} samples, "
              f"GFS run {f['model_run']})")
        if f["dry_run"]:
            print("  unsigned submit body (a dry run signs nothing): " + json.dumps(f["submit_body"], sort_keys=True))
        else:
            print(f"  commit id: {f['commit_id']}")
    if not report.forecasts:
        print("No forecast this cycle.")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="forecaster-worker", description="WRFCoin forecaster worker v0")
    parser.add_argument("command", nargs="?", default="run", choices=["run", "once", "address", "status"])
    parser.add_argument("--dry-run", action="store_true",
                        help="do everything except log in and submit; print what would be sent")
    args = parser.parse_args(argv)
    try:
        cfg = Config.from_env()
        wallet, created = wallet_mod.load_or_create(cfg.data_dir)
    except (ValueError, PermissionError, OSError) as error:
        print(f"Cannot start: {error}", file=sys.stderr)
        return 2
    if created:
        print(f"Made a new wallet. Its 12 words are in {cfg.data_dir}/{wallet_mod.PHRASE_FILE}. Keep that file private.")
    print(f"Wallet address: {wallet.address}")
    if args.command == "address":
        return 0
    print(gfs.ATTRIBUTION)
    site = Site(cfg.api_base)

    if args.command == "status":
        try:
            site.login(wallet, cfg.chain_id, cfg.network_id)
            rows = site.mine()
        except (SiteError, chain.LoginChallengeRefused) as error:
            print(f"Could not read your forecasts: {error}", file=sys.stderr)
            return 1
        if not rows:
            print("No forecasts from this wallet yet.")
        for row in rows:
            score = row.get("score") or {}
            scored = f"error {score.get('error')}" if score.get("status") == "scored" else (score.get("status") or "not scored yet")
            print(f"{row.get('issuedAt')} {row.get('stationAlias')} +{row.get('leadHours')}h {row.get('variable')} "
                  f"= {row.get('value')}: {row.get('status')}, {scored}")
        return 0

    loop = args.command == "run"
    while True:
        started = time.time()
        wait = cfg.cycle_seconds
        failed = False
        try:
            report = run_cycle(cfg, site, gfs.GfsClient(), Store(cfg.data_dir), wallet, dry_run=args.dry_run)
            _show(report, cfg.variable)
        except SiteError as error:
            failed = True
            wait = max(wait, error.retry_after or 0)
            print(f"Cycle stopped: {error}. Next try in {wait // 60} minutes.", file=sys.stderr)
        except Exception as error:  # noqa: BLE001 - one bad cycle must never end the worker or start a restart loop
            failed = True
            print(f"Cycle failed ({type(error).__name__}). Next try in {wait // 60} minutes.", file=sys.stderr)
        if not loop:
            return 1 if failed else 0
        sys.stdout.flush()
        # Always a full interval between cycles, also after an error: never a fast retry.
        time.sleep(max(60.0, wait - (time.time() - started)) if not failed else float(wait))


if __name__ == "__main__":
    sys.exit(main())
