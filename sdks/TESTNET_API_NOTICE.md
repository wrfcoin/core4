# Testnet API notice

The generated SDKs and legacy OpenAPI documents in this repository do **not**
describe the node API running on the WRFCoin testnet. Their historical server
names and generated operations must not be used as evidence that a route is
served.

The checked first-tranche node contract is
[`docs/api/launch-api.openapi.yaml`](../docs/api/launch-api.openapi.yaml). Its
paths and methods are verified against the Actix registrations in
`native-chain/src/bin/launch_api/routes/mod.rs`.

The per-file banners requested by core4#3245 remain intentionally deferred:
the same issue forbids modifying any file in merged PR #3231, whose diff owns
all eight generated SDK READMEs, `sdks/README.md`, and all three legacy specs.
