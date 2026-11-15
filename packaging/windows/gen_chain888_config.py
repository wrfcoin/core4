#!/usr/bin/env python3
"""Emit the canonical chain-888 sync-node config the Windows package ships.

The node is configured only by environment variables. On Linux the infra join
path (infra testnet/join-testnet-sync-node.sh) assembles them from three
places: testnet/cut-pins.env (chain identity and genesis pins), the
bootstrap.888.env interpolation done by testnet/scripts/chain-identity.py, and
the wrfcoin-node service in testnet/docker-compose.testnet.yml. A Windows user
has none of those, so this script reads the same infra files once and writes
one flat KEY=VALUE file with no comments and no quoting, which the launcher
(wrfcoin-node.ps1) can read without a shell parser.

Run it against an infra checkout whenever cut-pins.env changes:

    python3 packaging/windows/gen_chain888_config.py --infra ../infra
    python3 packaging/windows/gen_chain888_config.py --infra ../infra --check

It fails (exit 1) on any missing pin, placeholder (<...>), duplicate key or
malformed value, and with --check if the committed file differs.

cut-pins.env is parsed with the same rules as infra chain-identity.py
(value()): quoted values end at the matching quote and a trailing
`# comment` after it is allowed; unquoted values end at whitespace + `#`.
The file is never sourced or evaluated.
"""
import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "chain-888.conf"

# runtime variable -> cut-pins.env key. The same map as infra
# testnet/scripts/chain-identity.py GENESIS_888: a joiner validates block 1
# onward against exactly the inputs the chain was cut with.
GENESIS_888 = {
    "WRFCOIN_GENESIS_HASH": "WRFCOIN_CUT_GENESIS_HASH",
    "WRFCOIN_GENESIS_ACTIVATION_SCHEDULE": "WRFCOIN_GENESIS_ACTIVATION_SCHEDULE",
    "WRFCOIN_GENESIS_PRODUCER_SET": "WRFCOIN_GENESIS_PRODUCER_SET",
    "WRFCOIN_GENESIS_ORACLE_SET": "WRFCOIN_GENESIS_ORACLE_SET",
    "WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS": "WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS",
    "WRFCOIN_GENESIS_FAUCET_ADDRESS": "WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS",
    "WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS": "WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS",
    "WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS": "WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS",
    "WRFCOIN_GENESIS_FLAT_REWARD_SECONDS": "WRFCOIN_GENESIS_FLAT_REWARD_SECONDS",
    "WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS": "WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS",
    "WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS": "WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS",
    "WRFCOIN_SIGNING_PROTOCOL_VERSION": "WRFCOIN_SIGNING_PROTOCOL_VERSION",
}
POSITIVE_INTEGERS = [
    "WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS",
    "WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS",
    "WRFCOIN_GENESIS_FLAT_REWARD_SECONDS",
    "WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS",
    "WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS",
    "WRFCOIN_SIGNING_PROTOCOL_VERSION",
]

# Values the wrfcoin-node compose service pins as literals, which every syncing
# peer needs (a peer without the oracle-governance allowlist rejects block 1).
COMPOSE_LITERALS = [
    "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
    "WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT",
    "WRFCOIN_REQUIRE_PRODUCER_AUTH",
]

# Sync-only posture, as join-testnet-sync-node.sh writes it into .env
# (set_env WRFCOIN_SYNC_ONLY 1 / WRFCOIN_MIN_WORK_FACTOR 0 /
# ALLOW_UNENCRYPTED_STORAGE true) and as the compose service maps it
# (WRFCOIN_P2P_ENABLED true, WRFCOIN_P2P_PORT 9000, API on 8556). Two values
# differ from the container on purpose: a native process has no port mapping,
# so the API binds 127.0.0.1 directly, and P2P listens on 127.0.0.1 because a
# sync-only node is outbound-only and must not ask for inbound access.
NATIVE_SYNC_NODE = [
    ("WRFCOIN_SYNC_ONLY", "1"),
    ("WRFCOIN_MIN_WORK_FACTOR", "0"),
    ("ALLOW_UNENCRYPTED_STORAGE", "true"),
    ("WRFCOIN_API_HOST", "127.0.0.1"),
    ("WRFCOIN_API_PORT", "8556"),
    ("WRFCOIN_P2P_ENABLED", "true"),
    ("WRFCOIN_P2P_HOST", "127.0.0.1"),
    ("WRFCOIN_P2P_PORT", "9000"),
    ("RUST_LOG", "info"),
]

HEX64 = r"[0-9a-f]{64}"


class ConfigError(ValueError):
    pass


def pin_value(text, key):
    """One key from cut-pins.env, by infra chain-identity.py's value() rules."""
    rows = []
    for line in text.splitlines():
        match = re.match(r"^\s*(?:export\s+)?" + re.escape(key) + r"\s*=(.*)$", line)
        if not match:
            continue
        raw = match.group(1).strip()
        if raw.startswith(('"', "'")):
            quote = raw[0]
            end = raw.find(quote, 1)
            if end < 0 or raw[end + 1:].strip().split("#", 1)[0].strip():
                raise ConfigError(key + " is malformed (unterminated quote or text after it)")
            raw = raw[1:end]
        else:
            raw = re.split(r"\s+#", raw, maxsplit=1)[0].strip()
        rows.append(raw)
    if len(rows) > 1:
        raise ConfigError(key + " is defined more than once")
    return rows[0] if rows else ""


def compose_node_literals(text):
    """The literal values the wrfcoin-node service sets for COMPOSE_LITERALS."""
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if re.match(r"^  wrfcoin-node:\s*$", line))
    except StopIteration:
        raise ConfigError("docker-compose.testnet.yml has no wrfcoin-node service") from None
    block = []
    for line in lines[start + 1:]:
        if re.match(r"^  \S", line):
            break
        block.append(line)
    found = {}
    for key in COMPOSE_LITERALS:
        rows = [m.group(1) for m in (re.match(r"^\s+" + key + r":\s*(.*?)\s*$", l) for l in block) if m]
        if len(rows) != 1:
            raise ConfigError(f"wrfcoin-node sets {key} {len(rows)} times, expected once")
        raw = rows[0]
        if raw[:1] in "\"'" and raw[-1:] == raw[:1] and len(raw) >= 2:
            raw = raw[1:-1]
        if "${" in raw or "$" in raw:
            raise ConfigError(f"wrfcoin-node {key} is interpolated, not a pinned literal")
        found[key] = raw
    return found


def check_value(key, value):
    if not value.strip():
        raise ConfigError(key + " is missing or empty")
    if any(c in value for c in "<>\r\n"):
        raise ConfigError(key + " is still a placeholder or spans lines")
    if value != value.strip():
        raise ConfigError(key + " has leading or trailing whitespace")


def build(pins_text, compose_text):
    """Return the ordered (key, value) list, or raise ConfigError."""
    chain = pin_value(pins_text, "WRFCOIN_CUT_CHAIN_ID")
    if chain != "888":
        raise ConfigError(f"cut-pins.env WRFCOIN_CUT_CHAIN_ID is {chain!r}; this package is for chain 888")
    network = pin_value(pins_text, "WRFCOIN_CUT_NETWORK_ID")
    if network != "testnet":
        raise ConfigError(f"cut-pins.env WRFCOIN_CUT_NETWORK_ID is {network!r}, expected 'testnet'")

    out = [("WRFCOIN_CHAIN_ID", chain), ("WRFCOIN_NETWORK_ID", network)]
    genesis = {}
    for runtime, source in GENESIS_888.items():
        value = pin_value(pins_text, source)
        try:
            check_value(source, value)
        except ConfigError as error:
            raise ConfigError(str(error) + " in cut-pins.env") from None
        genesis[runtime] = value

    if not re.fullmatch(r"(?:0x)?" + HEX64, genesis["WRFCOIN_GENESIS_HASH"]):
        raise ConfigError("WRFCOIN_CUT_GENESIS_HASH must be 64 lowercase hex")
    if genesis["WRFCOIN_GENESIS_HASH"].removeprefix("0x") == "0" * 64:
        raise ConfigError("WRFCOIN_CUT_GENESIS_HASH must be nonzero")
    if not re.fullmatch(r"[0-9]+:[a-zA-Z_][a-zA-Z_0-9]*(?:,[0-9]+:[a-zA-Z_][a-zA-Z_0-9]*)*",
                        genesis["WRFCOIN_GENESIS_ACTIVATION_SCHEDULE"]):
        raise ConfigError("WRFCOIN_GENESIS_ACTIVATION_SCHEDULE must be an inline activation schedule")
    try:
        producers = json.loads(genesis["WRFCOIN_GENESIS_PRODUCER_SET"])
        oracles = json.loads(genesis["WRFCOIN_GENESIS_ORACLE_SET"])
    except ValueError:
        raise ConfigError("the genesis producer/oracle sets must be inline JSON") from None
    if not isinstance(producers, dict) or producers.get("version") != 1 or not producers.get("members"):
        raise ConfigError("WRFCOIN_GENESIS_PRODUCER_SET needs version 1 and nonempty members")
    if (not isinstance(oracles, dict) or not isinstance(oracles.get("oracle_set"), list)
            or not oracles["oracle_set"] or type(oracles.get("oracle_quorum")) is not int):
        raise ConfigError("WRFCOIN_GENESIS_ORACLE_SET needs a nonempty oracle_set and an integer oracle_quorum")
    if not re.fullmatch(HEX64 + r"(?:," + HEX64 + r")*", genesis["WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS"]):
        raise ConfigError("WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS must be comma-separated 64-hex keys")
    faucet = genesis["WRFCOIN_GENESIS_FAUCET_ADDRESS"].lower()
    if not re.fullmatch(r"0x[0-9a-f]{40}", faucet) or faucet == "0x" + "0" * 40:
        raise ConfigError("WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS must be 0x + 40 hex, not the zero address")
    for key in POSITIVE_INTEGERS:
        if not re.fullmatch(r"[1-9][0-9]*", genesis[key]):
            raise ConfigError(key + " must be a positive integer")
    out.extend(genesis.items())

    bootstrap = pin_value(pins_text, "WRFCOIN_CUT_BOOTSTRAP")
    check_value("WRFCOIN_CUT_BOOTSTRAP", bootstrap)
    for addr in bootstrap.split(","):
        if not re.fullmatch(r"/(?:dns4|dns|ip4)/[^/\s]+/tcp/[0-9]+/p2p/12D3KooW[1-9A-HJ-NP-Za-km-z]+", addr):
            raise ConfigError(f"WRFCOIN_CUT_BOOTSTRAP entry {addr!r} is not a /dns4|/ip4 .../tcp/N/p2p/<id> multiaddr")
    out.append(("WRFCOIN_P2P_BOOTSTRAP", bootstrap))

    signers = pin_value(pins_text, "WRFCOIN_CUT_SNAPSHOT_TRUSTED_SIGNERS")
    check_value("WRFCOIN_CUT_SNAPSHOT_TRUSTED_SIGNERS", signers)
    if not re.fullmatch(HEX64 + r"(?:," + HEX64 + r")*", signers):
        raise ConfigError("WRFCOIN_CUT_SNAPSHOT_TRUSTED_SIGNERS must be comma-separated 64-hex keys")
    out.append(("WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS", signers))

    literals = compose_node_literals(compose_text)
    keys = literals["WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS"]
    if not re.fullmatch(HEX64 + r"(?:," + HEX64 + r")*", keys):
        raise ConfigError("compose WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS must be comma-separated 64-hex keys")
    if literals["WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT"] != str(len(keys.split(","))):
        raise ConfigError("compose WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT does not match the key list")
    if literals["WRFCOIN_REQUIRE_PRODUCER_AUTH"] != "1":
        raise ConfigError("compose WRFCOIN_REQUIRE_PRODUCER_AUTH is not '1'")
    out.extend((key, literals[key]) for key in COMPOSE_LITERALS)

    out.extend(NATIVE_SYNC_NODE)

    seen = set()
    for key, value in out:
        if key in seen:
            raise ConfigError(key + " emitted twice")
        seen.add(key)
        check_value(key, value)
    return out


def render(pairs):
    return "".join(f"{key}={value}\n" for key, value in pairs)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--infra", type=Path, help="infra checkout (reads testnet/cut-pins.env and testnet/docker-compose.testnet.yml)")
    parser.add_argument("--cut-pins", type=Path, help="override path to cut-pins.env")
    parser.add_argument("--compose", type=Path, help="override path to docker-compose.testnet.yml")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--check", action="store_true", help="do not write; fail if --out differs")
    args = parser.parse_args(argv)
    pins = args.cut_pins or (args.infra / "testnet/cut-pins.env" if args.infra else None)
    compose = args.compose or (args.infra / "testnet/docker-compose.testnet.yml" if args.infra else None)
    if pins is None or compose is None:
        parser.error("give --infra, or both --cut-pins and --compose")
    try:
        text = render(build(pins.read_text(encoding="utf-8"), compose.read_text(encoding="utf-8")))
    except (OSError, UnicodeError) as error:
        print(f"ERROR: cannot read infra inputs: {error}", file=sys.stderr)
        return 1
    except ConfigError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else None
        if current != text:
            print(f"ERROR: {args.out} is stale; regenerate it from infra", file=sys.stderr)
            return 1
        print(f"OK: {args.out} matches the infra pins ({text.count(chr(10))} variables)")
        return 0
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {args.out} ({text.count(chr(10))} variables)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
