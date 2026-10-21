# Bonded node service: rehearsal, outage and recovery operations

Refs #2848 #2849 #3527. Companion to ADR-003 and the [operator CLI](../operator/node-share-cli.md).

This is an implementation and rehearsal runbook. It supplies no fleet rollout,
activation height, payout custody or GO evidence. The S6e admission PR sets `NODE_SHARE_3527_AND_S6_COMPLETE` only after
#3527 hardening and S6a–S6d merge evidence is reviewed. The flag permits a
governance-signed schedule extension; it installs no schedule and activates
nothing by itself. Activation still requires the owner-signed chain-bound
schedule extension at H=48000. Genesis admission remains refused. The owner must supply the exact chain-777 genesis, activation height,
three labelled payout addresses (`node-ws`, `node-mango`, `node-n5`), bond funds,
dedicated service/consumer keys, off-host spend custody and receiver capacity.

Use one 250 tWRF funded bond per deployment, shared between serving and consuming.
An epoch is 1,024 blocks; testnet challenge is 1,024 blocks inclusive, followed
by maturity. Withdrawal adds a 100-block buffer and remains owner-authorized.
Exactly one distinct non-self registered consumer qualifies a served node.
Eligible nodes split equally without a per-node cap. Missing work earns nothing;
it is not fraud or a liveness slash. Proven attributable fraud confiscates 100%
of the responsible bond into the nonspendable forfeiture account and jails it.

## Before scheduling or receiver operation

Verify all merged S6a–S6d heads and #3527 items 1–5; compare the pinned fleet binary
and feature support on all three deployments. Record actual nonzero current-head
checks separately from queued/skipped/billing-blocked hosted jobs (VOID).
The registered matrix entry lives in `scripts/integration_test_ratchet.json`;
`node_share_matrix.py` executes all selected targets and rejects zero-step runs.
`emit_determinism_matrix.py` emits the original OFF corpus plus the separate ON
companion. The ON anchor stores each root as 32 numeric bytes; the OFF anchor,
wire pins and existing roots stay unchanged. No golden regeneration can excuse
a divergence.

The ON runtime covers real producer mining, P2P application, persisted WAL,
checkpoint reload, imported reserve/root state and reorg reapplication. Its
registered companion target performs an actual trusted snapshot install,
consumer signing, durable archival, server endorsement and mined inclusion.
These are isolated test fixtures with synthetic funded accounts. They do not
prove production demand, wallet funding, device custody or production payouts.

Pre-stage compatible checkpoints, block/WAL envelopes and signed public recovery
artifacts with recorded hashes. New-format persistence constrains downgrade
even before activation. After activation use a forward compatible fix; do not
remove the schedule, reinterpret an old checkpoint, rewind the chain or fabricate
a missing activation marker. Test a restore with no healthy peer before rollout.

## Serial receiver rehearsals

Provision a dedicated scratch root with `.node-share-rehearsal-root` and a separate
public archive root. Neither root is a producer/standby database. Supply the
hashes and absolute paths of the pinned `launch_api` and `wrfcoin` binaries.
A public plan contains:

- `chain_id` = `777`, `network_id` = `testnet`, exact `genesis_hash`;
- `consumer_deployment_id` and one to three distinct non-self `providers`, each
  with `deployment_id`, exact registered `service_alias`, `base_url` and pinned
  `trusted_signers`;
- `scratch_root`, `archive_root`, `cli_binary`, `receiver_binary` and corresponding
  `cli_binary_sha256` / `receiver_binary_sha256`;
- owner-provisioned `min_free_bytes`, `poll_seconds` and `timeout_seconds`;
- `receiver_public_env` with the pinned oracle governance public-key set and
  explicit key count required by normal startup. This cannot override the DB,
  API bind, authentication lease or consumer signer.

Each lease starts a **signing receipt consumer**, using
`WRFCOIN_NODE_SHARE_REHEARSAL=true` and `WRFCOIN_SYNC_ONLY=false`. Its API must
bind explicitly to loopback, P2P must be disabled, and the database must be new
under the owned `.node-share-rehearsal-receiver` marker created by the runner.
The role disables consensus production and skips producer signer resolution;
it requires a dedicated snapshot/consumer signer even in debug. Ordinary
sync-only joiners remain unsigned and cannot be used for this rehearsal.
Authenticated provider reads refuse every redirect before forwarding credentials.

Run `python3 scripts/node_share_rehearsal.py --plan PUBLIC_PLAN --once` with
protected configuration on stdin. For ongoing scheduling omit `--once`; selection
uses canonical bond epoch reads and archives at most one successful claim per
server/consumer/epoch. Infra owns process supervision, deployment, disk/network
budget and the off-host owner submission queue; this PR installs none of them.

The stdin configuration has `servers` mapping provider IDs to Reader credentials
and `receiver_signer_env` for the dedicated hardware consumer signer, plus
`receiver_storage_key` (32-byte lowercase hex) for encrypted scratch storage.
Retain that key under owner custody if the scratch database must remain readable. It permits
only the module, dedicated key/public labels, `WRFCOIN_HSM_PIN_ENV`, and the
referenced `NODE_SHARE_RECEIVER_*_PIN` name. Keep this configuration out of argv,
logs, repositories and public receipt archives. No producer/governance/spend or
gamification key is inherited by the fresh child. Physical role separation still
needs the owner ceremony; matching a registered public key proves key control.

The client launches a new owned UUID database directory on loopback, with P2P and
production disabled and a new authentication lease. It proves that lease through
an Operator read, requires height 0 and zero blocks, then runs the production
CLI `snapshot fetch --bonded --auth-stdin`. The ordinary snapshot verifier checks
trust, signed roots, the chain window and atomic installation. The dedicated `/network/snapshot-sync/receiver-ingest` restore
route reads activation from the incoming marker, since a fresh receiver has no
schedule overlay yet. OFF/malformed imports are refused before installation.
The installed state refreshes the effective schedule before dedicated receiver
endorsement. A failed/dry-run fetch cannot become signed service.

Archive the installed receipt/manifest before endorsement/acknowledgement and
retain the exact signed claim. `INSTALLED_SIGNED_NOT_INCLUDED` is explicitly not
payment. The off-host owner authorizes a funded receipt batch, submits it through
`wrfcoin snapshot submit-receipt`, and records canonical inclusion by claim ID.
On uncertain submission retry the same durable packet after inspecting nonce and
inclusion. Do not manufacture a replacement success receipt or count HTTP
acknowledgement, a local ledger, pings or block production as paid service.
The client stops on capacity, signing, installation or archive failure and keeps
its owned scratch database for diagnosis; cleanup is a separate owner operation.

## Release-mode operator keys and Deposit assembly (S6d)

The explicitly selected software signer exposes only typed V2 Node bond-owner,
server/consumer possession and receipt-owner signing requests. It does not implement
BlockSigner or offer an arbitrary byte signer. Producer, attester, governance and
payout signing requests are refused; those existing roles retain their dedicated
HSM paths. Keep the accepted hot owner key distinct from service and off-host
payout/governance keys. This key holds the bond and owner funds, so a host compromise
risks those funds. No command here approves generation, signing, funding or deployment.

Use a fresh isolated owner HOME under the node's operator state directory.
`wallet list-addresses` on first initialization creates one random persisted key
without a mnemonic; recommend this path with an owner-approved encrypted backup.
`wallet new-address` emits a mnemonic: never run it into logs or tmux scrollback.
The normal wallet JSON is accepted directly, with no seed extraction or format
conversion. `node bond key-info --key-file OWNER_WALLET_JSON` prints only its
canonical address and public key and makes no API request.

`node bond deposit` has two stages. Both validate the explicit chain/genesis,
activation height against canonical state, current owner nonce, separate payout
and dedicated role keys. A dry-run takes only public keys plus the explicit
protected Operator token file for canonical reads; it never loads a signing key.
It prints the unsigned public Transaction and its hash, and optionally fsyncs a
new unsigned packet. Refuse observed tip before H; no pre-H registration is faked.
The effective epoch is epoch(next carrying height)+1. Deposit amount is fixed at
25000000000 base units and escrow uses the consensus V2 derivation.

```sh
wrfcoin --json node bond deposit --deployment-id DEPLOYMENT_HEX \
  --owner-key OWNER_PUBLIC_HEX --payout-address COLD_PAYOUT_ADDRESS \
  --payout-key PAYOUT_PUBLIC_HEX --server-key SERVER_PUBLIC_HEX \
  --consumer-key CONSUMER_PUBLIC_HEX --governance-key GOVERNANCE_PUBLIC_HEX \
  --service-alias REGISTERED_ALIAS --genesis-hash EXACT_GENESIS \
  --chain-id 777 --network-id testnet --activation-height 48000 \
  --api-url NODE_API --operator-token-file OPERATOR_TOKEN_FILE \
  --fee 1000000 --dry-run --out UNSIGNED_PUBLIC_PACKET
```

Obtain server, consumer, governance and payout approvals over that **same** public
packet using `node bond sign --transaction-file PUBLIC_PACKET --role ROLE
--chain-id 777 --network-id testnet --out NEW_PUBLIC_PACKET`. Explicit `--key-file`
wallet JSON/raw hex is accepted only for Node owner/server/consumer roles;
governance/payout use their existing dedicated HSM signer and hidden PIN prompt.
The final four-role approvals file remains public; no payout/governance key is
copied onto the owner host. An expected governance public key supplied to the plan
is not independent proof of current governance-allowlist membership; consensus
checks the actual allowlist at admission/application.

Repeat the Deposit command with identical public plan fields, removing --dry-run,
using `--approvals-file FOUR_ROLE_PUBLIC_PACKET --key-file OWNER_WALLET_JSON
--out NEW_SIGNED_PUBLIC_PACKET`. It refuses changes to authorization/outer fields,
nonce or inclusion epoch, obtains the owner signature, verifies all five signatures
through consensus and fsyncs the packet before **one** POST. There is no redirect
or retry. Refuse an existing output path before loading a key. An uncertain response
retains the packet, reads canonical bond state and reports UNCERTAIN; do not re-sign
or automatically retry. HTTP admission and matching canonical registration are
reported separately and do not establish inclusion of the exact outer transaction.
Verify canonical transaction/block inclusion before declaring success. A later
inclusion crossing an epoch still fails in consensus; reassemble/reapprove only
after resolving prior delivery/nonce, never silently patch signed approvals.

S6d does **not** open activation admission while the required #3527/S6a/S6b merge
proof is absent at its base. Source availability or an approved open stack is not
merged evidence. The guard and scheduling refusal tests remain false until the
required separately verified merge/evidence condition is satisfied.

## Receipt assembly and owner submission (S6c)

Registration has separate `owner_account` and `payout_account`. Receipt submission
uses the registered **server owner**, never the consumer/server endorsement key.
A distinct on-host owner key permits automation while all three labelled payout
spend keys remain off-host. This key also controls owner funds, bond updates,
exit and eventual principal withdrawal; it is not receipt-only delegation.
Provision only bond/fee funds there. Governance, producer, payout, server and
consumer keys must remain separate. Existing owner=payout registrations require
off-host signing or an owner-approved registration/custody plan; never copy their
payout key onto a host. The automation worker refuses owner=payout bindings.

Transfer only public `bonded-*.json` archives from S6b's scratch receiver to the
registered server owner's protected submission environment. Keep epoch archives
separate at `ARCHIVE_ROOT/DEPLOYMENT_ID/EPOCH`. The exact owner-selected chain,
genesis, deployment and sender are checked before signing; nonce is fetched from
`/addresses/{sender}`. The batch uses V2 `wrfcoin.node_receipt_submission.v1`.
Archive ordering is deterministic, exact duplicate claims are collapsed, and
wrong epoch/domain/genesis/deployment archives are refused. Before loading the
owner key, the CLI verifies the manifest, server ticket, inner receipt and consumer
endorsement using the consensus contract verifiers. Outer contracts require V2;
inner receipt signatures are checked under V1 or V2, with their permitted version
at the carrying height determined by the genesis schedule in consensus. Before signing,
it checks the public signer keys against canonical bond registrations. Consensus
still checks frozen roster, funding, height and settlement eligibility.

```sh
wrfcoin --json node receipt submit --archive PUBLIC_EPOCH_ARCHIVE --epoch EPOCH \
  --deployment-id SERVER_DEPLOYMENT --sender OWNER_ADDRESS \
  --genesis-hash EXACT_GENESIS --chain-id 777 --network-id testnet \
  --api-url SERVER_API --key-file OWNER_KEY_FILE \
  --operator-token-file OPERATOR_TOKEN_FILE --fee 1000000 --out NEW_PACKET
```

Owner key files may be the wallet CLI's base64 PKCS#8 JSON directly or a legacy
raw hex Ed25519 seed. On Unix they must be regular non-symlink files, mode exactly
0600, owned by the running effective user; other platforms refuse loading. The
stored public key/address must match SHA3-256(public key)[:20].
All inputs are bounded and errors do not include secret contents. Alternatively
replace `--key-file` with `--mnemonic-stdin` and pass the BIP-39 phrase on stdin.
Derivation matches the CLI wallet's SLIP-0010 Ed25519 master with empty passphrase.
No phrase/key/token belongs in argv, logs or a public packet. `--dry-run` loads
no key/credential and makes no requests; it prints the batch hash, size and claim
IDs. Consensus limits remain eight claims / 16 KiB binary batch. Receipt POST
retains its stricter 40 KiB JSON cap within the 2,097,152-byte bonded ingest cap.

Credential-bearing receipt requests expose only safe HTTP/error codes; response
bodies and transport details are never echoed into errors or diagnostics. Canonical
readback output contains local receipt IDs and inclusion booleans only.

The CLI saves/fsyncs a new exact signed packet **before** its single POST and then reads
every canonical receipt ID. `admitted=true` is pending until readback reports
`status=INCLUDED,included=true`; it does not establish matured payout or GO.
A lost response or missing acknowledgement reports `UNCERTAIN` after attempting
canonical inclusion readback. There is no automatic POST retry. Reusing an existing
output path refuses before loading a key. For uncertain delivery retain the packet,
inspect inclusion and current nonce,
then explicitly retry the exact packet using `snapshot submit-receipt`.
Do not re-sign a fresh nonce while an earlier packet remains outstanding.

`scripts/node_share_submit.py --plan PUBLIC_SUBMISSION_PLAN` is the explicit
submission hook alongside S6b. Its plan contains `deployment_id`, `sender`,
`chain_id`, `network_id`, `genesis_hash`, `api_url`, `archive_root`, `packet_root`,
`isolated_home`, `cli_binary`, `cli_binary_sha256`, `owner_key_file`,
`operator_token_file`, `fee_base_units` and `timeout_seconds`. Roots must already
exist, the binary is hash-pinned, and key/credential files are owner-only. It
selects the canonical epoch, submits once, and subsequently reads inclusion.
An existing packet without a journal stops at uncertain delivery for inspection.
The receiver's original rehearsal hook remains usable with off-host submission.

Infra may stage the following service/timer after owner deployment approval;
this PR installs or starts nothing. Use one instance per registered server owner,
mount public receiver archives, and provision separate owner key/token paths.

```ini
[Service]
Type=oneshot
User=node-receipts
ExecStart=/usr/bin/python3 /opt/core4/scripts/node_share_submit.py --plan /etc/node-receipts/plan.json
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/node-receipts/packets /var/lib/node-receipts/home
```

```ini
[Timer]
OnBootSec=2min
OnUnitInactiveSec=2min
Unit=node-receipts.service
```

Run receiver and submitter serially per instance. Do not share a sender nonce
with another automation process. A missing archive, stale epoch, incomplete
readback or pending packet remains unpaid/unverified. Recovery of an outstanding
prior-epoch packet is an explicit inspection/retry operation, not a new claim.

## Normal operation and one-host outage

Normal cyclic demand: N5 consumes ws; Mango consumes N5; ws consumes Mango. Each
server receives one actual signed import by a distinct bonded consumer.

| Full-epoch outage | Surviving real import pair | Earnings |
|---|---|---|
| Mango | N5 consumes ws; ws consumes N5 | ws and N5 eligible; Mango earns nothing |
| ws | Mango consumes N5; N5 consumes Mango | Mango and N5 eligible; ws earns nothing |
| N5 | ws consumes Mango; Mango consumes ws | ws and Mango eligible; N5 earns nothing |

Configure each consumer's preferred provider followed by its other distinct
provider. Selection may bypass an unavailable provider before starting work;
failed installation/signing stops the cycle for investigation. One survivor
cannot consume itself and has no qualifying receipt. Do not lower the witness
rule or create a new deployment identity from a scratch database or rotated key.
Restore ordinary cycles when the third host returns; old epoch bindings and
liability remain auditable. No missing work is punished as fraud.

## Restart/WAL, snapshot and activation reorg

1. Stop only the affected owned process through the owner-approved operator
   procedure. Preserve the database, checkpoint, signed packets and WAL hashes.
2. Use a binary compatible with the persisted marker/version. Load the checkpoint
   and replay retained block inputs plus the shared finalization fold; metadata
   cannot authorize an extra payout. Verify root, bond locks, reserve totals,
   eligibility, jail state, entitlement/maturity and terminal cursors at one
   canonical height. UNKNOWN stays UNKNOWN; container health is not this proof.
3. For snapshot recovery use a fresh isolated DB. Verify trusted manifest, entries,
   signed state root, genesis and block window. Import the durable activation
   marker, frozen bindings, reserve and evidence sections without deriving them
   from the local clock or an assumed activation height. Reject missing markers.
4. Reapply the retained tail. A matured epoch must pay exactly once across
   checkpoint, WAL and snapshot restart; prior reserves remain supply headroom
   commitments. Verify actual wallet deltas separately from reserve/provisional
   read models.
5. For a reorg up to 100 blocks crossing activation, close, maturity, confiscation
   or unlock, restore the prior canonical state and rederive the replacement
   branch's effects. Crossing activation removes/recreates the marker and epoch
   parent anchor with the branch. A global receipt cache or copied payout metadata
   cannot survive rollback as authority. Compare all monetary/lock/root fields.

An outage with no healthy peer needs the pre-staged compatible checkpoint and
retained canonical tail. If those are unavailable or roots disagree, halt and
report unavailable recovery evidence; do not patch balances, mint missing awards
or infer canonical roots from historical source hashes.

## GO evidence and owner follow-ups

Keep per-epoch roster/bond state, actual installation evidence, distinct consumer
mapping, canonical receipt IDs, outstanding/unissued reserves, challenge status,
matured derived payouts and each labelled wallet's observed balance delta.
#2849 remains open until all three have earned matured on-chain payouts, renewed
service, survived restart/recovery and passed the outage control. Source/CI
completion does not authorize invitations or owner GO.

Infra follow-ups: provision isolated receivers and capacity on ws/Mango/N5;
install serial supervision and outage fallback plans; provision dedicated signer
custody and protected stdin credentials; retain public artifacts and submit them
through an off-host owner queue; monitor inclusion/expiry, reserve/maturity and
wallet deltas; stage hashed recovery artifacts; execute an owner-approved fleet
upgrade and activation ceremony after the source guards are resolved.
