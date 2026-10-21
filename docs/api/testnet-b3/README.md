# B3 launch_api route contract

[launch-api.openapi.json](launch-api.openapi.json) is an OpenAPI 3.0.3 inventory
of every registered method/path in core4 `65134a64492135b6ac81344dd0d2edd8dfaf1309`
under the deployed infra `750298cdf20c` node build features:
`full-mining,p2p,hsm,upgrade-notice-fetch`.

[deployed-node-build.Dockerfile](deployed-node-build.Dockerfile) records the
exact node build stanza from that infra revision's
`infra/docker/Dockerfile.core:43-45`. It is a documentation fixture, never a
build instruction run by the checker. The release adds HSM and upgrade-notice
fetch support relative to core4's standalone native-chain Dockerfile;
neither feature adds HTTP registrations in the pinned source. The complete
selected route surface is the same, with corrected build provenance.

The inventory contains 123 operations. Another 27 registrations are compiled
out: 13 privacy, 10 ZK, three execution and one development operation. They are
listed separately in `x-excluded-operations`, not advertised under `paths`.
Governance operations include their actual `/governance` scope prefix.

This is a **paths and methods** contract. Handler references, path parameters,
Rust cfg expressions, build features and the pinned Git revision provide provenance.
The check also verifies the production HttpServer mounts the router. Response
schemas, request bodies, authentication roles, capability readiness, public
edge admission and runtime behavior are outside this inventory. Registration
does not imply a usable bridge, successful transaction, or anonymous access.
The localhost server entry selects no public hostname.

The separate current-main first tranche and hand-transcribed schemas remain
with [core4#3262](https://github.com/wrfcoin/core4/pull/3262). This versioned
snapshot complements that work without editing its spec/parser/workflow or
regenerating legacy SDKs. The three legacy specs and generated SDKs describe
other interfaces and must not be generated from this deliberately shallow
inventory. Refs #3245; Refs #2995.

## Run the checks

From the repository root, Python 3.11+ and the pinned commit in local Git are
the only requirements. No Cargo, dependency install, live node or network
request is made:

```sh
python3 docs/api/testnet-b3/check_contract.py
python3 -m unittest discover -s docs/api/testnet-b3 -v
```

The gate rejects **both** invented spec operations and registered operations
missing from the spec. It checks literal cfg expressions against the build's
explicit and default/transitive native-chain features; disabled registrations
do not enter the advertised surface. Unknown routing forms are rejected for
explicit parser review. Paths are derived structurally from child scopes;
feature selection does not rely on route-name prefixes.

For a CI check of the current source against this deployed contract:

```sh
python3 docs/api/testnet-b3/check_contract.py --working-tree
```

This checks route, handler, scope, cfg and build-feature drift. Unrelated source
edits do not require a new inventory. A future source tree may legitimately differ from the
deployed release. Preserve this versioned document and introduce a new
release contract after reviewing that change. The default gate continues
checking the pinned release. A missing pinned commit is a nonzero result,
never a skipped PASS; CI must supply the relevant Git history locally.
The entrypoint is CI-runnable; no new workflow or #3262 workflow edit is
included in this docs-only slice.

The explicit inventory writer is available for reproducibility:

```sh
python3 docs/api/testnet-b3/check_contract.py --emit docs/api/testnet-b3/launch-api.openapi.json
```

Writing from a working tree, a different revision or changed deployment build
features is refused before any output is created, so later source cannot be
labelled as the deployed release.

The file is JSON so the gate uses only the standard library; JSON is also a
valid OpenAPI serialization. It was independently validated with the
[official OpenAPI 3.0 schema](https://spec.openapis.org/oas/3.0/schema/2024-10-18).
The tests cover scope/method/path changes, additions/removals, build/cfg
changes, production mount removal despite remaining test mounts, metadata
and parameter drift, comments and unsupported registration forms.

No deployment, node image change, feature scheduling, generated SDK change
or application behavior change accompanies this contract.
