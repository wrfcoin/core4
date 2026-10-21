#!/usr/bin/env python3
"""Owner-run, bounded GET-only audit. Output contains counts and identifiers."""
import argparse
import http.client
import json
import re
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


class Stop(Exception):
    pass


def address(value):
    if not isinstance(value, str):
        raise Stop("OWNER_INVALID")
    value = value.removeprefix("wrf").removeprefix("0x")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", value):
        raise Stop("OWNER_INVALID")
    return "0x" + value.lower()


def integer(value):
    if type(value) is not int or value < 0:
        raise Stop("SHAPE_INVALID")
    return value


def context(page):
    root = page.get("state_root_hex")
    if not isinstance(root, str) or not re.fullmatch(r"[0-9a-f]{64}", root):
        raise Stop("SHAPE_INVALID")
    height = integer(page.get("observed_at_height"))
    settled = integer(page.get("settled_height"))
    if settled > height:
        raise Stop("SHAPE_INVALID")
    return height, settled, root, integer(page.get("total"))


MAX_NODE_READS = 30  # owner ruling 2026-10-03 (option A): at most 30 node reads per check
MIN_DELAY = 1.0
STATE_VERSION = 1


def validate_state(state, identity):
    """A state file is untrusted input; accept only the exact shape this helper writes."""
    try:
        if state.get("version") != STATE_VERSION or state.get("identity") != identity:
            raise Stop("STATE_INVALID" if state.get("version") != STATE_VERSION else "STATE_IDENTITY_MISMATCH")
        e = state["expected"]
        if e is not None:
            if not (isinstance(e, list) and len(e) == 4 and isinstance(e[2], str)
                    and re.fullmatch(r"[0-9a-f]{64}", e[2])):
                raise Stop("STATE_INVALID")
            for i in (0, 1, 3):
                integer(e[i])
        if state["cursor"] is not None and not isinstance(state["cursor"], str):
            raise Stop("STATE_INVALID")
        if not isinstance(state["previous"], str):
            raise Stop("STATE_INVALID")
        for key in ("pages", "reads_total", "runs"):
            integer(state[key])
        if type(state["registry_done"]) is not bool or not isinstance(state["stations"], list):
            raise Stop("STATE_INVALID")
        for row in state["stations"]:
            if not (isinstance(row, list) and len(row) == 3 and isinstance(row[0], str)
                    and re.fullmatch(r"[A-Za-z0-9_.:-]{1,128}", row[0])
                    and type(row[1]) is bool and (row[2] is None or isinstance(row[2], str))):
                raise Stop("STATE_INVALID")
        results = state["owner_results"]
        if not isinstance(results, dict):
            raise Stop("STATE_INVALID")
        for key, value in results.items():
            try:
                valid = address(key) == key
            except Stop:
                valid = False
            if not valid or value not in ("ok", "missing", "zero"):
                raise Stop("STATE_INVALID")
        check_registry_invariants(state)
    except (KeyError, AttributeError, TypeError):
        raise Stop("STATE_INVALID") from None


def check_registry_invariants(state):
    """A resumed audit must never PASS or NO-GO on a registry it did not fully and coherently read."""
    rows, expected, pages = state["stations"], state["expected"], state["pages"]
    ids = [row[0] for row in rows]
    if any(a >= b for a, b in zip(ids, ids[1:])):
        raise Stop("STATE_INVALID")  # unique, strictly increasing station ids
    if state["previous"] != (ids[-1] if ids else ""):
        raise Stop("STATE_INVALID")
    if (pages == 0) != (expected is None):
        raise Stop("STATE_INVALID")
    if pages == 0:
        if rows or state["cursor"] is not None or state["registry_done"] or state["owner_results"]:
            raise Stop("STATE_INVALID")
        return
    if state["registry_done"]:
        if len(rows) != expected[3] or state["cursor"] is not None:
            raise Stop("REGISTRY_INCOMPLETE" if len(rows) != expected[3] else "STATE_INVALID")
        owners = set()
        for _sid, is_active, owner in rows:
            if is_active:
                try:
                    owners.add(address(owner))
                except Stop:
                    pass
        if not set(state["owner_results"]) <= owners:
            raise Stop("STATE_INVALID")
    else:
        # More pages were pending: the cursor is the last saved id and the count is short.
        if not rows or state["cursor"] != state["previous"] or len(rows) >= expected[3]:
            raise Stop("STATE_INVALID")
        if state["owner_results"]:
            raise Stop("STATE_INVALID")


def new_state(identity):
    return {"version": STATE_VERSION, "identity": identity, "expected": None,
            "cursor": None, "previous": "", "pages": 0, "registry_done": False,
            "stations": [], "owner_results": {}, "reads_total": 0, "runs": 0}


def audit(get, limit=100, max_pages=8, max_reads=MAX_NODE_READS, delay=MIN_DELAY,
          sleep=time.sleep, state=None, save=None, identity=""):
    """GET returns (HTTP status, decoded JSON); it never logs a response.

    At most `max_reads` (hard ceiling 30) node reads are made per call, with a fixed
    `delay` (>= 1s) slept between consecutive reads, and no read is ever retried. If
    the audit cannot finish inside the budget it returns status PARTIAL after saving
    progress through `save(state)`; the next call with the same `state` resumes. With
    no `state`, running out of budget is STOP READ_BUDGET (no partial audit).
    """
    if type(max_reads) is not int or not 1 <= max_reads <= MAX_NODE_READS:
        raise Stop("READ_BOUND_EXCEEDS_30")
    if not delay >= MIN_DELAY:
        raise Stop("DELAY_TOO_SHORT")
    resumable = state is not None
    if state is None:
        state = new_state(identity)
    else:
        validate_state(state, identity)
    reads = 0

    def left():
        return max_reads - reads

    def read(path):
        nonlocal reads
        if reads >= max_reads:  # defence in depth: callers check left() first
            raise Stop("READ_BUDGET")
        if reads:
            sleep(delay)
        reads += 1
        state["reads_total"] += 1
        status, obj = get(path)
        return status, obj

    def read_registry(path):
        status, obj = read(path)
        if status != 200 or not isinstance(obj, dict):
            raise Stop("READ_REFUSED")
        return obj

    def checkpoint():
        if save is not None:
            save(state)

    def partial(phase):
        if not resumable:
            raise Stop("READ_BUDGET")
        state["runs"] += 1
        checkpoint()
        return {"status": "PARTIAL", "phase": phase, "reads_this_run": reads,
                "reads_total": state["reads_total"], "registry_pages": state["pages"],
                "registry_stations_collected": len(state["stations"]),
                "owners_checked": len(state["owner_results"])}

    endpoint = "/transactions/station/registrations?limit=" + str(limit)
    expected = tuple(state["expected"]) if state["expected"] is not None else None

    if not state["registry_done"]:
        while True:
            if state["pages"] >= max_pages:
                raise Stop("PAGE_BOUND")
            if left() < 1:
                return partial("registry")
            cursor = state["cursor"]
            path = endpoint + ("&after=" + urllib.parse.quote(cursor, safe="") if cursor else "")
            page = read_registry(path)
            current = context(page)
            if expected is None:
                expected = current
            if current != expected:
                raise Stop("CANONICAL_STATE_MOVED")
            rows = page.get("stations")
            if not isinstance(rows, list) or len(rows) > limit:
                raise Stop("SHAPE_INVALID")
            previous = state["previous"]
            collected = []
            for row in rows:
                if not isinstance(row, dict):
                    raise Stop("SHAPE_INVALID")
                sid = row.get("station_id")
                if not isinstance(sid, str) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,128}", sid):
                    raise Stop("STATION_ID_INVALID")
                if sid <= previous or type(row.get("is_active")) is not bool:
                    raise Stop("PAGE_ORDER_INVALID")
                previous = sid
                owner = row.get("owner_address")
                # Only these fields survive this page. Coordinates are discarded.
                collected.append([sid, row["is_active"], owner if isinstance(owner, str) else None])
            more = page.get("has_more")
            next_cursor = page.get("next_cursor")
            if type(more) is not bool:
                raise Stop("SHAPE_INVALID")
            if more and (not rows or next_cursor != previous):
                raise Stop("CURSOR_INVALID")
            if not more and next_cursor is not None:
                raise Stop("CURSOR_INVALID")
            state["expected"] = list(expected)
            state["stations"].extend(collected)
            state["previous"] = previous
            state["pages"] += 1
            if more:
                state["cursor"] = next_cursor
                checkpoint()
                continue
            state["cursor"] = None
            if len(state["stations"]) != expected[3]:
                raise Stop("REGISTRY_INCOMPLETE")
            state["registry_done"] = True
            checkpoint()
            break
    else:
        # Resumed in the account phase: re-anchor on the registry context first. Every
        # earlier read is bracketed by this read and the final one; a moved tip stops.
        if len(state["stations"]) != expected[3]:
            raise Stop("REGISTRY_INCOMPLETE")
        if left() < 1:
            return partial("accounts")
        if context(read_registry(endpoint)) != expected:
            raise Stop("CANONICAL_STATE_MOVED")

    stations = [tuple(row) for row in state["stations"]]
    owners = {}
    invalid = []
    active = []
    for sid, is_active, owner in stations:
        if not is_active:
            continue
        active.append(sid)
        try:
            canonical = address(owner)
        except Stop:
            invalid.append(sid)
            continue
        owners.setdefault(canonical, []).append(sid)

    results = state["owner_results"]
    for canonical in sorted(owners):
        if canonical in results:
            continue
        if left() < 1:
            return partial("accounts")
        status, body = read("/addresses/" + canonical)
        if status == 404:
            if not isinstance(body, dict) or body.get("ok") is not False:
                raise Stop("SHAPE_INVALID")
            error = body.get("error")
            if not isinstance(error, dict) or error.get("code") != "NOT_FOUND":
                raise Stop("READ_REFUSED")
            results[canonical] = "missing"
            checkpoint()
            continue
        if status != 200 or not isinstance(body, dict) or body.get("ok") is not True:
            raise Stop("READ_REFUSED")
        account = body.get("data")
        if not isinstance(account, dict) or address(account.get("address")) != canonical:
            raise Stop("ACCOUNT_ID_MISMATCH")
        balance = account.get("balance")
        if not isinstance(balance, str) or not re.fullmatch(r"[0-9]{1,39}", balance):
            raise Stop("SHAPE_INVALID")
        if int(balance) >= 1 << 128:
            raise Stop("SHAPE_INVALID")
        results[canonical] = "zero" if int(balance) == 0 else "ok"
        checkpoint()

    # Account responses lack a height/root. Bracket all reads with canonical
    # registry context, and refuse an advancing tip or reorg without retry.
    if left() < 1:
        return partial("closing-bracket")
    if context(read_registry(endpoint)) != expected:
        raise Stop("CANONICAL_STATE_MOVED")
    missing = [c for c in sorted(owners) if results[c] == "missing"]
    zero = [c for c in sorted(owners) if results[c] == "zero"]
    debt = bool(invalid or missing or zero)
    return {
        "status": "NO-GO" if debt else "AUDIT-PASS",
        "registry_count": len(stations), "active_count": len(active),
        "owner_count": len(owners), "request_count": state["reads_total"],
        "reads_this_run": reads, "runs": state["runs"] + 1,
        "observed_height": expected[0], "active_station_ids": active,
        "owner_account_ids": sorted(owners),
        "invalid_owner_station_ids": invalid, "missing_account_ids": missing,
        "zero_balance_account_ids": zero,
        "station_owner_ids": [
            {"station_id": sid, "owner_account_id": canonical}
            for canonical in sorted(owners) for sid in owners[canonical]
        ],
    }


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def reader(base, token):
    parsed = urllib.parse.urlsplit(base)
    local = parsed.hostname in ("localhost", "127.0.0.1", "::1")
    if (parsed.scheme != "https" and not (parsed.scheme == "http" and local)) or not parsed.hostname:
        raise Stop("BASE_INVALID")
    if parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/"):
        raise Stop("BASE_INVALID")
    if not token or len(token) > 4096 or any(ord(c) < 33 or ord(c) > 126 for c in token):
        raise Stop("TOKEN_INVALID")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def get(path):
        req = urllib.request.Request(base.rstrip("/") + path, method="GET",
                                     headers={"Authorization": "Bearer " + token})
        try:
            response = opener.open(req, timeout=5)
        except urllib.error.HTTPError as err:
            response = err
        except (OSError, ValueError, http.client.HTTPException):
            raise Stop("TRANSPORT_REFUSED") from None
        with response:
            payload = response.read(1048577)
            if len(payload) > 1048576:
                raise Stop("RESPONSE_BOUND")
            try:
                obj = json.loads(payload)
            except (ValueError, UnicodeError):
                raise Stop("SHAPE_INVALID") from None
            return response.code, obj
    return get


def load_state(path, identity):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            raw = handle.read(8 * 1024 * 1024 + 1)
    except FileNotFoundError:
        return new_state(identity)
    except OSError:
        raise Stop("STATE_INVALID") from None
    if len(raw) > 8 * 1024 * 1024:
        raise Stop("STATE_INVALID")
    try:
        state = json.loads(raw)
    except (ValueError, UnicodeError):
        raise Stop("STATE_INVALID") from None
    if not isinstance(state, dict):
        raise Stop("STATE_INVALID")
    validate_state(state, identity)
    return state


def writer(path):
    def save(state):
        tmp = path + ".tmp"
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(state, handle, sort_keys=True)
        os.replace(tmp, path)
    return save


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--max-reads", type=int, default=MAX_NODE_READS,
                        help="node reads for this run; hard ceiling %d" % MAX_NODE_READS)
    parser.add_argument("--delay", type=float, default=2.0,
                        help="fixed seconds between reads; minimum %s" % MIN_DELAY)
    parser.add_argument("--state", help="resumable state file; one per node. Without it the whole "
                        "audit must fit in one run's reads or it STOPs READ_BUDGET")
    args = parser.parse_args()
    try:
        token = sys.stdin.readline(4098).rstrip("\n")
        identity = args.base.rstrip("/")
        state = save = None
        if args.state:
            state = load_state(args.state, identity)
            save = writer(args.state)
        result = audit(reader(args.base, token), max_reads=args.max_reads, delay=args.delay,
                       state=state, save=save, identity=identity)
        if save is not None and result["status"] != "PARTIAL":
            os.remove(args.state)  # finished: the next check starts fresh
    except (Stop, OSError, ValueError, TypeError, KeyError, RecursionError, http.client.HTTPException) as err:
        code = str(err) if isinstance(err, Stop) else "SHAPE_INVALID"
        print(json.dumps({"status": "STOP", "code": code}))
        return 2
    print(json.dumps(result, sort_keys=True))
    if result["status"] == "PARTIAL":
        return 3
    return 0 if result["status"] == "AUDIT-PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
