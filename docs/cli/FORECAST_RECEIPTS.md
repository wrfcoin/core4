# Offline forecast receipt signing

`wrfcoin forecast sign-receipt` produces a receipt file; it never submits a
transaction. Build/test preparation only: **no attester deployment before Thursday
2026-10-22**. Producing a valid signature does not prove scorer accuracy, oracle-set
membership, or payout activation.

Production signing requires a CLI built with `--features hsm`, the existing oracle
PKCS#11 configuration (`WRFCOIN_HSM_MODULE_PATH` and oracle-specific key/public-key
labels), and access to the token. It uses the same signer resolver as
`wrfcoin ceremony sign --role oracle`. The PIN comes from the hidden terminal
prompt. PIN-bearing environment variables are rejected; never supply a PIN in
arguments, environment variables, or files. There is no production file-key fallback.

Supply the same forecast JSON, station ID, commit nonce and timestamp used by the
commit/reveal. The command derives the commit ID with their existing canonical
helper and the manifest digest from the manifest. There is no commit-ID or
oracle-ID override. Metrics JSON contains `mae`, `rmse`, `bias` and `skill_score`.

```sh
WRFCOIN_CHAIN_ID=777 WRFCOIN_NETWORK_ID=testnet wrfcoin forecast sign-receipt \
  --chain-id 777 --network-id testnet \
  --forecaster example-forecaster --station-id example-station \
  --data-file forecast.json --nonce original-commit-nonce --timestamp 1700000000 \
  --manifest-file manifest.json --metrics-file metrics.json --out receipt.json
```

Both identity flags and environment values must be present, non-blank and match
exactly without surrounding whitespace. The captured binding is used for signing
and self-verification. `--receipt-nonce` optionally overrides the receipt nonce;
`--nonce` always means the original commit nonce. `--verified-at` optionally sets
the verification timestamp; otherwise the current time is used.

For an offline rehearsal with a **throwaway key only**, a debug build additionally
accepts `--rehearsal --oracle-key rehearsal.key`. The file must be regular and have
permissions no broader than 0600. Release builds refuse this path. Never export an
oracle token key for a rehearsal. Receipt output uses atomic no-overwrite creation
with mode 0600 on Unix; unsupported platforms refuse output. Existing files and
symlinks are not replaced. Pass the resulting file to the existing
`forecast reveal --receipt-file` command only when separately authorized to submit.
