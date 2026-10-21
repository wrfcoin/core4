#!/bin/bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
wrf="${WRFCOIN_CLI_BINARY:-$repo_root/target/debug/wrfcoin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# ---------------------------------------------------------------------------
# Control 1 — the #2614 path this fix exists for: a failure during CLI
# CONTEXT CONSTRUCTION, before any command handler (and its emit_error JSON
# envelope) exists. A malformed --config file fails in ConfigManager before
# dispatch; the unfixed binary exited nonzero with NO output at all. The
# assertion pins the `wrfcoin:` diagnostic specifically, on stderr
# specifically — the #2616 review proved that a `|| *"error"*` fallback is
# satisfied by the JSON envelope's own "error" key and turns this control
# vacuous, and that the stdout/stderr split is what keeps --json machine
# consumers safe.
# ---------------------------------------------------------------------------
printf 'this is not [valid toml\n' > "$tmp/broken-config.toml"

pre_handler_stdout="$tmp/pre-handler.out"
pre_handler_stderr="$tmp/pre-handler.err"
if "$wrf" --json --config "$tmp/broken-config.toml" setup keygen --output "$tmp/pre" --count 1 \
    > "$pre_handler_stdout" 2> "$pre_handler_stderr"; then
    echo "pre-handler control unexpectedly succeeded" >&2
    exit 1
fi

grep -q 'wrfcoin:' "$pre_handler_stderr" || {
    echo "pre-handler failure produced no 'wrfcoin:' diagnostic on stderr" >&2
    echo "stderr was:" >&2
    cat "$pre_handler_stderr" >&2
    exit 1
}

if grep -q 'wrfcoin:' "$pre_handler_stdout"; then
    echo "the human diagnostic leaked onto stdout, which --json consumers parse" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Control 2 — a HANDLED validation error (--count 0) must keep emitting the
# machine-readable JSON envelope on stdout. This is deliberately NOT asserted
# via `wrfcoin:`: this path reaches a handler and was never silent, so the
# envelope is the contract here.
# ---------------------------------------------------------------------------
handled_stdout="$tmp/handled.out"
if "$wrf" --json setup keygen --output "$tmp/invalid" --count 0 > "$handled_stdout" 2>/dev/null; then
    echo "invalid-count unexpectedly succeeded" >&2
    exit 1
fi
grep -q '"error"' "$handled_stdout" || {
    echo "handled validation failure lost its JSON error envelope on stdout" >&2
    cat "$handled_stdout" >&2
    exit 1
}

# ---------------------------------------------------------------------------
# Control 3 — the success path still works and reports the generated keypair.
# ---------------------------------------------------------------------------
if real_output="$("$wrf" --json setup keygen --output "$tmp/real" --count 1 2>&1)"; then
    [[ "$real_output" == *"Generated 1 validator keypair"* ]] || {
        echo "successful keygen output was not the expected CLI response: $real_output" >&2
        exit 1
    }
else
    status=$?
    echo "real keygen failed with status $status: $real_output" >&2
    exit 1
fi

echo "keygen diagnostic controls passed"
