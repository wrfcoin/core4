# WRFCoin CLI JSON Response Schema

To make automation easier, the `wrfcoin` CLI now exposes a stable JSON envelope for every command when you pass `--json`. The same structure is emitted by `--quiet` (bare `data` payload only) and annotated with `--why` when you need to understand routing/decision logic.

## Envelope
| Field | Type | Description |
| --- | --- | --- |
| `success` | `bool` | `true` for successful invocations, `false` when errors occur. |
| `command` | `string` | The fully qualified CLI command that produced the output, e.g. `wallet balance`. |
| `message` | `string` | Human-friendly summary of what happened. |
| `data` | `any` | Command-specific details (objects, arrays, or primitives). |

Errors omit `data` and set `success` to `false`, for example:

```json
{
  "success": false,
  "command": "wallet send",
  "message": "Insufficient balance",
  "data": null
}
```

## Sample (wallet balance)
```
wrfcoin wallet balance --json
```
```json
{
  "success": true,
  "command": "wallet balance",
  "message": "Balance for wrfc_placeholder_address",
  "data": {
    "address": "wrfc_placeholder_address",
    "balance": 0
  }
}
```

When `--quiet` is also specified the CLI emits just the `data` block:
```
wrfcoin wallet balance --json --quiet
```
```json
{"address":"wrfc_placeholder_address","balance":0}
```

## Explain mode
Add `--why` to any command to get extra logging (visible in standard output or JSON) that documents the decision tree, such as default address selection, fee estimation, or retry/backoff reasoning. This flag is orthogonal to `--json`/`--quiet` and is ideal for debugging automation.

## Profile metadata
Profile commands (`wrfcoin profile list|use|create`) reuse the same schema. Example:

```json
{
  "success": true,
  "command": "profile list",
  "message": "Configured profiles",
  "data": [
    { "name": "default", "active": true },
    { "name": "station-west", "active": false }
  ]
}
```

The CLI configuration lives in `~/.wrfcoin/config.json` and now stores a `profiles` map plus the `active_profile` name so you can switch contexts without rewriting the underlying file manually.
