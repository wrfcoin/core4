# Forecast prototype

`forecast-proto keygen --out ./keys` creates fresh key files with mode 0600.
It refuses to overwrite an existing key. Standard output and `keys.json`
contain public keys and addresses only; private keys are stored only in the
individual `.key` files.

`run` keeps the existing options, with `--oracle-key` and `--forecaster-key`
now taking **file paths**, not literal keys. Each file must be a regular file
with permissions no wider than 0600 (0400 is also accepted). Permissions are
checked on the opened file before reading it. Private key contents and parsing
errors are never printed.

```sh
forecast-proto run \
  --node-url http://127.0.0.1:8556 \
  --station-id YOUR_STATION \
  --oracle-key ./keys/oracle.key \
  --forecaster-key ./keys/forecaster.key \
  --token "$CONTRIBUTOR_TOKEN" --oracle-token "$ORACLE_TOKEN" \
  --observations-dir ./observations --evidence-dir ./evidence
```

Set `WRFCOIN_CHAIN_ID` and `WRFCOIN_NETWORK_ID` to the node's values first.
Alternatively, omit either key option and supply its hex key through
`WRFCOIN_FORECAST_ORACLE_KEY` or `WRFCOIN_FORECAST_FORECASTER_KEY` in the process
environment. Populate these through your secret manager or a hidden prompt;
do not paste literal keys into shell commands. A file option takes precedence
over its environment variable. Environment values are not included in help.
File permission enforcement and secure key generation require Unix; use the
environment for key input on other platforms.

The prototype still uses collector observation artifacts and the existing
persistence/trend backtest. Run `forecast-proto run --help` for all options.
Forecast submissions are **wallet-scoped** (#3070): the Contributor bearer
passed as `--token` must carry a wallet claim equal to the forecaster address
this tool derives from `--forecaster-key` (compared case-insensitively). On the
node, bind it with `WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS=<forecaster address>`
next to `WRFCOIN_CONTRIBUTOR_TOKEN`; a bearer with no wallet claim is refused
with `FORECASTER_BEARER_UNBOUND`, and a mismatch with
`FORECASTER_NOT_BOUND_TO_BEARER`. An operator bearer may submit for any
forecaster.

## Persistence-relative skill (Refs #3244)

After reading its input artifacts, the tool reads
`GET /transactions/oracle/pending?limit=1` with the oracle bearer. Its
chain/network identity must match, and its `active_features` list describes
the node's next inclusion height, including applied schedule extensions.
Only the exact `forecast_skill_baseline` name selects the new scorer. An
older node omitting that list retains legacy scoring; a malformed view stops
before publishing a transaction. There is no scoring-mode CLI or environment
override. This feature ships unscheduled and refuses live scheduling pending
a reviewed scorer rollout and activation decision.

When active, each variable compares the forecast's error with the last input
observation's persistence error at the same truth target. Skill is
`clamp(1 - forecast_RMSE / persistence_RMSE, 0, 1)` per variable, then averaged;
a zero-error persistence tie earns zero skill. Different physical units are
never pooled into one RMSE. MAE, RMSE and bias keep their legacy definitions.
The manifest's existing `model_id` carries the versioned suffix
`;skill=persistence-rmse/v1`, which its digest and the oracle receipt bind.
No manifest field, positional wire layout or schema version changes.

With the feature off, model IDs, forecast artifacts, score reports and signed
receipts retain their legacy bytes. This adds one authenticated feature-view
read. The tool remains a collector-artifact backtest, not a production oracle.
Nodes authorize and verify the signed receipt and consume its skill; they do
not recompute this floating-point baseline or enforce an oracle's chosen
strategy. Reward/state application, transaction types, checkpoints, genesis
and deployed schedule pins are untouched. No activation height is selected.

Local controls: `cargo test -p wrfcoin-forecast-proto` exercises the real binary
with synthetic loopback transport, including signed receipts, digest binding,
feature-off artifact parity and zero-publication refusals.
