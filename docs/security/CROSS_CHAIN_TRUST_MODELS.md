# Cross-Chain Trust Models

Source audit for core4 issue #2191 at commit `798b333675404955997e7d89b42edf62d58dfbe3`.
This inventory classifies executable code, not architecture diagrams or provider
marketing. A **path** is one distinct code-defined transfer or message-submission
surface. The networks in `CrossChainNetwork` share one generic implementation and
therefore count as one path.

## Executive result

- **13 code-defined paths** were found.
- **11 are weaker or less complete than their names and comments imply.** They
  either authenticate only a single API/issuer, simulate success, have no receiving
  verifier, or are client stubs for a server path that does not exist in core4.
- **0 production transfer paths are reachable on the first public testnet from this
  repository.** The native `BridgeLock` and `BridgeUnlock` variants deliberately
  produce no state transition, and `launch_api` exposes bridge safety controls but
  no transfer endpoint.
- **0 first-public-testnet launch blockers were found, provided the no-production-
  bridges attachment gate remains enforced.** Three production enablement blockers
  were filed as #2215, #2216, and #2217.

"Not reachable" is not a safety endorsement. It is the reason the unsafe paths do
not block the first valueless testnet. Enabling any provider, bridge contract,
relayer, issuer, or mint/release handler changes that conclusion and requires a new
go/no-go review.

## Source-derived inventory

### What each path claims and actually verifies

| # | Path | Counterparty | What it presents | What the code actually verifies | Actual trust model | First-testnet reachability |
|---:|---|---|---|---|---|---|
| 1 | Rust `CrossChainTransfer` / `BridgeLock` / `BridgeUnlock` adapter and relay queue | Ethereum, Polygon, Avalanche, Arbitrum, Optimism, Base, Solana, Sui, Aptos, Cosmos, Polkadot, or `Custom` | A generic lock/unlock/mint/burn/attestation message path | Struct shape, deterministic message ID, and in-memory duplicate ID only. There is no signature, source lock proof, counterparty finality proof, or receiving executor. Native block application treats both bridge transaction variants as no-ops. | **None (transport-only)** | **No value path.** Types compile, but no mint/release state transition exists. |
| 2 | Axelar SDK provider | Configured Axelar gateway and target chain | Secure cross-chain token transfer | Required JSON fields and one HTTP response from `gatewayUrl`; no source receipt, committee proof, destination receipt, or quorum signature | **Single issuer-attestation endpoint replacing committee verification** | No; not configured by `launch_api` |
| 3 | LayerZero SDK provider | Configured LayerZero endpoint and target chain | LayerZero token bridge | Required JSON fields and one HTTP response from `endpointUrl`; no oracle/DVN, executor, source, or destination proof | **Single issuer-attestation endpoint replacing oracle/hybrid verification** | No; not configured by `launch_api` |
| 4 | Wormhole SDK provider | Wormhole Guardian network through configured API | Guardian-secured token and arbitrary-message bridge | Required JSON fields and API-returned status/VAA fields. Guardian signatures, quorum, set rotation, emitter, and VAA bytes are not verified locally. | **Single issuer-attestation endpoint replacing committee verification** | No; not configured by `launch_api` |
| 5 | Ethereum bridge-contract SDK provider | Configured EVM chain, RPC, wallet, and bridge contract | EVM bridge transfer | Local wallet submission and source-chain receipt; status calls use 12 source confirmations. No target-chain verifier, issuer, mint, or release implementation exists here. | **Issuer-attestation / configured-contract hybrid** | No; SDK-only |
| 6 | Cosmos IBC SDK provider | Configured Cosmos chains and IBC channel | IBC asset transfer | A signer submits `MsgTransfer`; consensus and IBC proof verification are delegated to the configured chains. The SDK checks only source inclusion/status and does not prove destination acknowledgement. | **Light-client, as enforced by the external IBC runtimes** | No WRFCoin IBC client or channel exists |
| 7 | Polkadot XCMP SDK provider | Configured parachains and relay-chain consensus | XCMP/XCM asset transfer | A local account signs an XCM extrinsic and the SDK reads chain events. Cross-chain validity is delegated to the relay/parachain runtimes. | **Consensus-native / light-client-equivalent** | No WRFCoin parachain or runtime integration exists |
| 8 | Arbitrum provider source | Ethereum and Arbitrum gateways | Optimistic-rollup bridge with fraud-proof delay | No transaction is broadcast: submission returns a random hash, deposit status is hard-coded complete, and claim data contains a zero proof. | **None (simulation)** | No; not registered or exported by the controller |
| 9 | BSC provider source | BSC and configured EVM target contracts | BSC bridge transfer | Builds a transaction but `submitTransaction` simulates a hash; no signing, broadcast, destination proof, or issuer verification | **None (simulation)** | No; not registered or exported by the controller |
| 10 | Hop provider source | Hop API, bonders, and configured L2 pair | Hop liquidity bridge | Sends transfer/status/recovery requests to one HTTP API; no bonder, contract-event, or canonical-settlement proof is checked | **Single issuer-attestation endpoint replacing hybrid/bonded verification** | No; not registered or exported by the controller |
| 11 | `SecureMessagePassing` source | Caller-selected signer and transport | Authenticated, encrypted, replay-protected cross-chain messaging | RSA signature against a public key supplied by the caller, plus process-local nonce memory. There is no signer registry or chain authorization. Encrypted messages are signed before payload encryption but verified before decryption, so they cannot verify as constructed. | **Single signer / self-asserted issuer** | No; not exported and no receiving execution path |
| 12 | Python `BridgeService` | Configured GraphQL server and its relayers | Cross-chain bridge operations | Address/amount shape and GraphQL response only; no chain receipt, relayer proof, or mint authorization | **Single API issuer-attestation** | No matching GraphQL bridge server exists in core4 |
| 13 | Rust CLI `/bridges/*` client | Configured REST server and selected provider | Transfer, configure, test, and inspect bridges | API response only. The called `/bridges/transfer`, `/bridges/configure`, `/bridges/test`, and `/bridges/transactions/*` routes do not exist in `launch_api`. | **Single API issuer-attestation, currently non-functional** | No; server routes are absent |

Source anchors: `shared-protocol/src/cross_chain/adapters.rs:15-98`,
`shared-protocol/src/cross_chain/types.rs:7-145`,
`shared-protocol/src/cross_chain/relay_queue.rs:47-100`,
`native-chain/src/bin/launch_api/transition_extractor.rs:189-204`,
`sdks/extensions/javascript/cross-chain/CrossChainController.ts:21-28,167-178`, and
`tools/cli/WrfcoinCLI/src/commands/bridge.rs:121-256`.

### Compromise set, loss bounds, controls, reorgs, and tests

| # | Minimum compromise needed to forge or censor | Value at risk | Rate limit / maximum loss per unit time | Circuit breaker | Deep-reorg behavior | Integration coverage |
|---:|---|---|---|---|---|---|
| 1 | Any caller can construct a message or enqueue it; any relay process can censor it | Zero in current native execution; a future receiver that trusts the message would expose its full issuer limit | None / unbounded once connected to an issuer | The shared safety manager exists but no transfer path calls it | No confirmation depth or reorg handling | Inline unit tests only; #1391 tracks missing integration coverage |
| 2 | One configured/compromised Axelar HTTP endpoint | Downstream issuer or liquidity limit; zero WRF today | None / downstream limit | None in provider | None | Controller tests replace provider with a mock; #2215 |
| 3 | One configured/compromised LayerZero HTTP endpoint | Downstream issuer or liquidity limit; zero WRF today | None / downstream limit | None in provider | None | Controller tests replace provider with a mock; #2217 |
| 4 | One configured/compromised Wormhole HTTP endpoint, rather than Guardian quorum | Downstream issuer or liquidity limit; zero WRF today | None / downstream limit | None in provider | `consistency` is sent as a string but not verified | Controller tests replace provider with a mock; #2216 |
| 5 | One controlling trust domain for the configured bridge contract/target issuer; a stolen source wallet can also submit locks | Funds sent to the source bridge contract and any target issuance | None / contract or issuer limit | None in provider; shared manager is disconnected | Source status waits for 12 blocks; a deeper reorg is not reconciled with target issuance | Controller test mocks the provider; no two-chain test |
| 6 | Counterparty consensus/client failure or compromised configured signing key | Tokens committed to the external IBC channel | No SDK limit / channel balance | None in SDK; IBC timeout/refund is the protocol fallback | Source UI status marks one confirmation; canonical IBC clients handle proof updates, but this SDK does not verify the destination acknowledgement | Provider is mocked; no live two-chain test |
| 7 | Relay/parachain consensus or runtime/asset-issuer control; compromised local signing account can submit transfers | Tokens submitted to the configured XCM route | No SDK limit / account or runtime limit | None in SDK | Reads recent blocks without an explicit finalized-head requirement; no local reorg reconciliation | Provider is mocked; no live relay/parachain test |
| 8 | Any caller receives simulated success; a consumer that trusts it can be fooled without compromising a chain | Zero on-chain; consumer-defined if fake status authorizes value elsewhere | None / consumer limit | None | Hard-coded status ignores reorgs | No current provider integration test |
| 9 | Any caller receives a simulated source hash; a consumer that trusts it can be fooled | Zero on-chain; consumer-defined if fake status authorizes value elsewhere | None / consumer limit | None | No submitted transaction to reconcile | No current provider integration test |
| 10 | One configured/compromised Hop HTTP endpoint | Downstream consumer or liquidity limit; zero WRF today | None / downstream limit | None | No canonical-settlement or reorg check | No current provider integration test |
| 11 | Any caller can supply a key matching its own forged message; the configured private key can censor all messages | No receiver is wired; future handler-defined | None / handler-defined | None | Timestamp expiry is wall-clock only; no chain reorg model | No current non-deprecated test |
| 12 | One configured/compromised GraphQL endpoint or relayer | Server-defined; zero in core4 | Values are reported by the server, not enforced by this client / server-defined | Server-defined, absent here | Server-defined, absent here | No end-to-end bridge server test |
| 13 | One configured/compromised API server; currently calls return route-not-found | Zero in current `launch_api` | None in client / server-defined | The separate safety-status endpoint cannot guard absent transfer routes | Server-defined, absent here | No matching route integration test |

## Safety controls and the runbook

`shared-protocol/src/bridge/` is a safety **library**, not a value-moving bridge.
It provides exposure reservation, manual/anomaly halts, challenge bookkeeping,
stuck-item detection, and alert hooks. `native-chain/src/bridge/mod.rs` only
re-exports that library. The authenticated `launch_api` routes expose its control
plane with Reader, Contributor, and Operator roles.

No production transfer, adapter, relay queue, SDK provider, CLI path, or state
transition calls `BridgeSafetyManager::check_bridge_operation`,
`complete_bridge_operation`, or `fail_bridge_operation`. Consequently:

- the default 1,000,000 per-chain and 10,000,000 global exposure caps do not bound
  any current transfer loss;
- the circuit breaker can halt only this in-memory manager, not an external bridge;
- a challenge records caller-provided strings and an operator-provided outcome; it
  verifies no counterparty proof or challenger signature; and
- restarts discard the in-memory safety state.

`docs/BRIDGE_SAFETY_RUNBOOK.md` describes the intended operating controls and is
cross-linked to this audit. Its historical references to `src/cross_chain/bridge.rs`,
`src/layer2/bridge.rs`, backend bridge routes, and 5,000,000/25,000,000 caps do not
match the current core4 source. Operators must not treat that runbook as evidence
that a transfer path is protected.

## Findings

### Production bridge enablement blockers

1. **Axelar is actually single-endpoint trust:** #2215.
2. **Wormhole VAAs are consumed without Guardian verification:** #2216.
3. **LayerZero is actually single-endpoint trust:** #2217.

Each can forge or censor a value-bearing result if a downstream issuer trusts the
adapter. None is attached to the first public testnet today, so none is a first-
testnet blocker while the attachment gate below remains closed.

### Non-launch-blocking gaps

- The Rust message path has no authenticity or finality verification, but it also
  has no receiving mint/release path. Issues #2192 and #2193 must not make it
  value-bearing until those checks exist.
- Arbitrum and BSC return simulated transaction hashes, and Arbitrum returns
  hard-coded completion/zero proof data. They are currently unregistered source
  files, not transfer paths.
- Hop, the Python service, and the CLI trust APIs that core4 does not implement.
- `SecureMessagePassing` is self-asserted single-signer messaging and its encrypted
  construction/verification order is inconsistent. It has no receiver or value
  effect.

## First public testnet gate

The first public testnet is **NO-GO** if any production bridge adapter, bridge
contract, relayer, external issuer, wrapped asset, or mint/release handler is
configured or reachable. The gate is recorded in
`docs/operations/LAUNCH_READINESS_GO_NOGO_CADENCE.md`.

Before any later bridge enablement, repeat this inventory against the exact release
commit, resolve the relevant production blockers, connect safety controls to the
actual value path, set an explicit loss-per-time limit, and run a two-chain
adversarial integration test including forged proofs, replay, censorship, and a
reorg deeper than the configured confirmation depth.
