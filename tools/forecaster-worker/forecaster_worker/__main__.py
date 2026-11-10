"""Command line: `python -m forecaster_worker [run|once|address|status] [--dry-run]`."""
import argparse
import json
import os
import sys
import time

from . import chain, gfs, wallet as wallet_mod
from .site import MAX_RETRY_AFTER_SECONDS, Site, SiteError
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
        if dry_run_from_env(os.environ.get("WORKER_DRY_RUN", "")):
            args.dry_run = True
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
    if args.command in ("run", "once"):
        print("Dry run: nothing is signed or sent." if args.dry_run else "Live: forecasts will be signed and sent.")
    site = Site(cfg.api_base)

    if args.command == "status":
        if args.dry_run:
            print("A dry run never logs in, and reading your forecasts needs a login. "
                  "Run `status` without --dry-run and with WORKER_DRY_RUN off.", file=sys.stderr)
            return 1
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
    if loop:
        # Restart-proofing: however the last process ended, the next cycle starts no sooner than one
        # interval after the last one started. `once` is a manual command and runs at once.
        last = Store(cfg.data_dir)
        wait = seconds_until_next_cycle(last.cycle_started, cfg.cycle_seconds, time.time(), last.not_before)
        if wait > 0:
            print(f"The last cycle started less than {cfg.cycle_seconds // 60} minutes ago, or the site asked "
                  f"the worker to wait. Waiting {max(1, round(wait / 60))} minutes before the next one.")
            sys.stdout.flush()
            time.sleep(float(wait))
    while True:
        started = time.time()
        wait = cfg.cycle_seconds
        failed = False
        try:
            store = Store(cfg.data_dir)
            if loop:
                store.mark_cycle_start(int(started))
            report = run_cycle(cfg, site, gfs.GfsClient(), store, wallet, dry_run=args.dry_run)
            _show(report, cfg.variable)
        except SiteError as error:
            failed = True
            wait = max(wait, error.retry_after or 0)
            if loop and wait > cfg.cycle_seconds:
                try:    # remember a long back-off, so a restart does not cut it short
                    Store(cfg.data_dir).mark_not_before(int(started) + wait, int(started))
                except Exception:  # noqa: BLE001 - never let bookkeeping end the loop
                    pass
            print(f"Cycle stopped: {error}. Next try in {wait // 60} minutes.", file=sys.stderr)
        except Exception as error:  # noqa: BLE001 - one bad cycle must never end the worker or start a restart loop
            failed = True
            print(f"Cycle failed ({_describe(error)}). Next try in {wait // 60} minutes.", file=sys.stderr)
        if not loop:
            return 1 if failed else 0
        sys.stdout.flush()
        # Always a full interval between cycles, also after an error: never a fast retry.
        time.sleep(max(60.0, wait - (time.time() - started)) if not failed else float(wait))


def seconds_until_next_cycle(last_start, cycle_seconds: int, now: float, not_before=None) -> int:
    """How long a starting worker must wait so cycles are never closer than one interval, and a
    longer wait the site asked for (Retry-After) is kept across a restart."""
    asked = 0
    if isinstance(not_before, int) and not isinstance(not_before, bool):
        asked = min(max(0, int(not_before - now)), MAX_RETRY_AFTER_SECONDS)
    if not isinstance(last_start, int) or isinstance(last_start, bool):
        return asked
    elapsed = now - last_start
    if elapsed < 0:             # the clock moved back: wait one interval, not longer
        return max(cycle_seconds, asked)
    return max(0, int(cycle_seconds - elapsed), asked)


_DRY_RUN_ON = ("1", "true", "yes", "on")
_DRY_RUN_OFF = ("", "0", "false", "no", "off")


def dry_run_from_env(text: str) -> bool:
    """WORKER_DRY_RUN must be a known word. A typo stops the worker; it never silently runs live."""
    value = text.strip().lower()
    if value in _DRY_RUN_ON:
        return True
    if value in _DRY_RUN_OFF:
        return False
    raise ValueError(f"WORKER_DRY_RUN must be one of {', '.join(_DRY_RUN_ON)} or {', '.join(v for v in _DRY_RUN_OFF if v)}")


def _describe(error: Exception) -> str:
    """Class and message of an error, one line, cut short. Wallet words and login tokens are never
    part of an error message: the phrase is not kept in memory and the token only travels in a header."""
    text = " ".join(str(error).split())[:200]
    return f"{type(error).__name__}: {text}" if text else type(error).__name__


if __name__ == "__main__":
    sys.exit(main())
