# WRFCoin Core

WRFCoin Core is the Rust node and protocol implementation for a test network that records weather contributions and distributes test tokens.

## Public testnet

The public testnet opened on **2026-10-22**.

This is a test network. It may be reset without notice. Balances and tokens have no monetary value. No migration to mainnet is promised or implied. Please attach no production bridges, exchange listings or irreplaceable assets.

Read the [reset-and-expectations disclaimer](docs/TESTNET_DISCLAIMER.md) before using the network.

[Website and web wallet](https://wrfcoin.com) ·
[Quickstart](https://github.com/wrfcoin/docs/blob/main/testnet/quickstart.md) ·
[Testnet support](https://github.com/wrfcoin/testnet-support) ·
[Security policy](SECURITY.md)

## Try it

1. Open the [WRFCoin website](https://wrfcoin.com) to watch the chain: block height, the weather map and the block explorer.
2. Follow the [quickstart](https://github.com/wrfcoin/docs/blob/main/testnet/quickstart.md) to set up a wallet and claim testnet WRF from the faucet.
3. Read the [facts page](https://github.com/wrfcoin/docs/blob/main/testnet/facts.md) for what the testnet does and does not offer.
4. To run a sync node, follow the [node join guide](https://github.com/wrfcoin/infra/blob/main/testnet/JOIN_TESTNET.md).
5. Ask for help or report bugs at [testnet-support](https://github.com/wrfcoin/testnet-support); report vulnerabilities privately through the [security policy](SECURITY.md).



## Where to start

| If you want to read… | Go to | What it is |
| --- | --- | --- |
| The node and its HTTP API | [`native-chain/`](native-chain/) | Node runtime and HTTP API |
| Shared types and validation | [`shared-protocol/`](shared-protocol/) | Shared protocol types, validation and state transitions |
| Networking | [`p2p/`](p2p/) | Peer networking |
| Consensus | [`consensus-engines/`](consensus-engines/) | Consensus engine implementations; the public testnet uses Proof-of-Publication |
| Command-line tools | [`tools/cli/WrfcoinCLI/`](tools/cli/WrfcoinCLI/) | Command-line interface for WRFCoin |
| Forecast scoring | [`forecast-score/`](forecast-score/) | Forecast scoring types and algorithms, used by the node through shared-protocol |
| Documentation | [`docs/`](docs/) | Protocol, security and design documentation |
| Integration tests | [`tests/integration/`](tests/integration/) | Tests of interactions between platform components |

## What works today

- The Proof-of-Publication testnet uses a **77-second target block interval** and an authorized producer set. Running a sync node does not make it a producer.
- The node accepts weather contributions and oracle attestations and exposes chain data through its HTTP API.
- Node-service reward accounting is implemented. Successful epoch payouts still need operational verification; launch does not depend on the first node payout.
- The separate web wallet provides the user interface for test-token transfers. Start with the quickstart above; this repository supplies the chain, not the website.

These statements describe the current testnet and source capabilities, not a production-readiness guarantee. A healthy container or passing unit test alone does not prove end-to-end operation.

## Planned and experimental

Public internet peering is targeted for launch; follow the
[node join guide](https://github.com/wrfcoin/infra/blob/main/testnet/JOIN_TESTNET.md)
for the currently documented prerequisites. Signed downloadable binaries are not yet published.

The [architecture decisions](docs/adr/) describe proposed work, including native smart accounts and payment channels. They are design records, not a list of activated features. Other workspace crates include experimental execution, privacy and proof systems; their presence does not mean the public testnet enables them.

## Build and join

Install the pinned [Rust toolchain](rust-toolchain.toml), OpenSSL development libraries and the Protobuf compiler. Build from source:

```bash
git clone https://github.com/wrfcoin/core4.git
cd core4
cargo build -p wrfcoin-native-chain
```

To configure and run a sync node, follow the [infra join guide](https://github.com/wrfcoin/infra/blob/main/testnet/JOIN_TESTNET.md). Building a binary alone does not join the network. See [CONTRIBUTING.md](CONTRIBUTING.md) for development and test commands.

## Repository map

| Path | Contents |
| --- | --- |
| `native-chain/` | Node runtime and HTTP API |
| `shared-protocol/` | Shared protocol types, validation and state transitions |
| `p2p/` | Peer networking |
| `tools/cli/WrfcoinCLI/` | Command-line tools |
| `docs/` | Protocol, security and design documentation |

Contributions follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report vulnerabilities privately through the security policy above.

## Forecast scoring implementations

The node uses the local [`wrfcoin-forecast-score` crate](forecast-score/), through
shared-protocol. The separate [standalone forecast-score repository](https://github.com/wrfcoin/forecast-score)
is reference tooling and is not substituted into that dependency path. Both
packages are version 0.1.0, but their APIs and receipt wire behavior need separate
validation. See the standalone repository's README for the reconciliation note
and migration options; no package migration or feature activation is implied.

## License

Licensed under [Apache-2.0](LICENSE).

---

_Note: issue and pull-request numbers mentioned in this repository refer to a private archive that predates this public repository; they are not links into this repo._
