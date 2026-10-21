# Recorded live 777/testnet activation schedule

Refs #3277, #3092, #3361. This deployment record captures the already-carried
E0/E0f schedule for review and later E1 Stage A preflight. It is not consumed by
the node, a cutter or a signer. Importing it into cut-time configuration would
misrepresent the history: block 1 **omits** `protocol_activation_schedule`, so
the chain signing version is v1 and genesis schedules no consensus features.
The binary protocol version is separately at least 3.

| Origin | Sequence | Feature | First active height | Carried at |
| --- | --- | --- | --- | --- |
| Binary bootstrap | — | `schedule_extension` | 29800 | — |
| E0 | 0 | `reward_gross_outlier_guard` | 32500 | 29802 |
| E0 | 0 | `weather_skew_block_time_allowance` | 32500 | 29802 |
| E0f | 1 | `faucet_sink_binding` | 33000 | 30656 |

[777-testnet.json](777-testnet.json) records chain/genesis identity, these
entries, public transaction IDs, empty version-step lists, minimum binary
protocol and signed release references. The public faucet address is bound
from height 33000; no key or credential is stored.

Provenance: the compiled `LEGACY_CHAIN_BOOTSTRAP` and `LEGACY_FAUCET_BINDINGS`
in `shared-protocol/src/crypto/chain_binding.rs` at source main `9efd50b116ed`;
[core4 #3277](https://github.com/wrfcoin/core4/issues/3277) documents the absent
genesis schedule. The workspace sprint `STATUS.md` records E0 R7 inclusion at
2026-10-01T23:45:10Z, and E0f admission/inclusion at 2026-10-02 18:13Z/18:36Z.
The reviewed `b3-e0f-block-v2.md` pins E0's transaction/height and E0f's recipe;
`b3-e0-blocks-v6-FINAL.md` pins chain identity and the B3 release. These are
existing owner/LEAD records, not new live observations by this contribution.
Inclusion is recorded; activation, faucet payment and earnings remain separate
owner execution evidence. No signature or carrying-block proof is invented.

## Read-only comparison

The checker uses the production `/network/info` and `/network/upgrade`
projections (`native-chain/src/bin/launch_api/routes/network.rs`,
`native-chain/src/schedule_view.rs`). An authorized operator may run:

```sh
python3 docs/deployment/live-feature-schedule/check.py --node-url "$NODE_BASE_URL"
```

One invocation makes **at most three GETs**, in order: info, upgrade, info.
Each JSON body has a 64 KiB byte cap, 32-level JSON cap, 5-second socket timeout
and 10-second whole-request deadline. HTTP mode requires POSIX interval timers.
No redirects, proxies, authentication, cookies, retries, block scan, shell or
write request. Use a previously authorized loopback tunnel or operator endpoint;
the builder does not contact live nodes. Repeating against three nodes costs
at most nine GETs; this checker does not claim common-height hash/finality parity.

Offline use accepts a regular UTF-8 JSON file with `info_before`, `upgrade`,
`info_after`, containing the three exact response bodies:

```sh
python3 docs/deployment/live-feature-schedule/check.py --sample node-schedule.json
python3 docs/deployment/live-feature-schedule/test_check.py
```

No fixture is presented as a live capture. Duplicate members, nonfinite JSON,
oversize/deep data and FIFO/device/symlink inputs refuse. The record is validated
before any request. A moving tip refuses without retry; take a separately
authorized fresh sample. Exit 0 reports `MATCH`, the sampled tip/next height,
feature count and canonical parsed-record SHA-256. Exit 1 prints a bounded
diagnostic without echoing URL, response content or credentials.

Comparison is exact on identity, feature/height, source, extension sequence and
carrying height; extra/missing/duplicate entries refuse. It also requires a
established untruncated ready schedule, known features, no warning/halt, current
signing v1, and consistent `active_at_next`/remaining-block counts and faucet
sink. At tip 32999 the binding is active **for the next block**, not evidence
that a faucet payment has happened. No response type coercion is accepted.

`/network/upgrade` does not expose transaction IDs, extension signatures,
release/minimum-protocol history or all future version steps. Those record
fields retain owner-record provenance; the checker cannot authenticate them.
`MATCH` is agreement with this node's projection, not cryptographic inclusion,
full historical-version validation, chain health or activation proof.

E1 adds new entries and must produce a separately reviewed successor record;
this record deliberately refuses that drift. Do not edit/adopt the record
automatically from a response. No feature, genesis, environment, migration,
image, protocol implementation or schedule changes in this contribution.
