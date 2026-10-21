# Offline station registration from stored owner proofs

`wrfcoin station build-registration` accepts either `--owner-key <file>` or
`--owner-pubkey <ed25519 hex> --owner-signature <signature hex>`. The latter
uses the phone's stored ownership proof; the operator needs only the governance
key. The CLI verifies the exact `station_ownership_message_with_domain`
(chain ID, network ID, station ID, owner address) and the protocol's canonical
owner address binding before reading the governance key or signing.

For a batch, project the backend export into a JSON array of rows:

```json
[
  {
    "station_id": "mob-example",
    "owner_address": "<canonical public address>",
    "owner_pubkey": "<public ed25519 hex>",
    "owner_signature": "<stored ownership signature hex>",
    "latitude": 46.5,
    "longitude": -92.5,
    "elevation_meters": 242.5,
    "hardware_model": "phone",
    "provenance_class": "real",
    "data_quality_tier": "gold",
    "submitted_at": 1790000000
  }
]
```

```sh
wrfcoin station build-registration --batch registrations.json \
  --governance-key governance.key --chain-id CHAIN --network-id testnet \
  --out registrations.artifact.json
```

The command-wide domain applies to every row. Single-row flags cannot accompany
`--batch`. Each row requires its own public key and signature; private keys are
never accepted in batch JSON. `elevation_meters` is optional; defaults for omitted
`hardware_model`, `provenance_class`, and `data_quality_tier` match the single-row
command (`reference-station`, `real`, `gold`). Unknown fields, duplicate station
IDs, station IDs longer than 128 UTF-8 bytes, empty batches, and invalid
timestamps are rejected. The exact rendered artifact (including whitespace and
metadata) must fit the launch API's 65,536-byte request ceiling; larger artifacts
are refused before any output write. Split the input into smaller arrays and
rebuild each array separately. Remove backend database
metadata before using this schema; retain the exact signed station ID and address
text (including its address prefix).

All row/schema/proof failures are listed by one-based row number. No governance
key is read and no output is written if any row fails. A successful batch emits
one `station_registration_artifact_v1` with transactions in input order; each
transaction timestamp comes from its row's `submitted_at`. Single-row artifacts
retain their original shape and deterministic bytes. The ownership signature
covers identity and domain; governance authorizes the remaining station fields.
Coordinate JSON stabilization and final protocol authorization checks apply to
both input paths. The governance public key must be present in the local
`WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS` allowlist. The command performs no network calls.


## Phone-batch size calibration (777/testnet)

The executable regression `realistic_phone_batch_artifact_size_calibration`
generates test-only Ed25519 keys, derives the real 36-character mobile station
IDs, supplies owner proofs and builds complete governance-signed transactions.
It uses the backend export's `phone-barometer` / `unverified` shape with
0.1-degree grid points and both 128-hex signatures. That schema has no MAC field;
unknown fields remain rejected. No production identifier or key is a fixture.

| Rows | Pretty artifact bytes including final newline | Result |
| --- | --- | --- |
| 1 | 3,091 | Fits |
| 21 | 62,631 | Fits |
| 22 | 65,609 | Refused |
| 30 | 89,384 | Refused |
| 63 | 187,556 | Refused |

For this pinned fixture the maximum is **21 rows**, with roughly **2,976 bytes
per row** in a multi-row batch. `CALIBRATED_MOBILE_STATION_BATCH_ROWS = 21` is the
operational recommendation. It is not a protocol row cap or a substitute for
`MAX_STATION_ARTIFACT_BYTES = 65,536`: transaction hashes serialize as decimal
byte arrays, so different keys change the exact size, as can other domains and
hardware labels. The 21-row fixture has 2,905 bytes of headroom. The previously
suggested 30 rows do not fit this real output shape. Split N invitees into
`ceil(N / 21)` batches, then retain the exact byte check on each actual build.

## Artifact publication and submission contract (#3385)

Artifacts are written to a unique temporary file in the output directory,
flushed to disk and renamed over the destination only when complete. A write or
sync failure preserves the previous artifact. A crash before rename can leave a
hidden `.wrfcoin-station-*.tmp` file; it is never the submission artifact.
Address-binding errors name `owner_address` and the one-based batch row instead
of suggesting a single-row flag.

A batch intentionally has no shared top-level `submitted_at`: its transactions
may have different timestamps. The actual node submission type in
`native-chain/src/bin/launch_api/routes/station.rs` accepts only `chain_id`,
`network_id`, and `transactions`; it validates `tx.timestamp` for every row.
The existing operator examples `scripts/demo-prove-the-loop.sh` and
`scripts/demo-two-node-convergence.sh` explicitly project those same three fields
with `jq -c '{chain_id, network_id, transactions}'` before submitting. Neither
reads a top-level `submitted_at`. Preserve that projection when submitting a
batch. This confirmation is from source; no live ceremony was performed.
