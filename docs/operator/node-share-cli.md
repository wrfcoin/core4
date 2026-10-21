# Bonded node service operator CLI (S6a, default OFF)

Refs #2848 #2849 #3527. This binary refuses live scheduling until #3527 and
both S6 slices are complete and merged. These commands prepare public packets
and inspect canonical state; they do not activate the feature or establish GO.

Keep owner and payout spend signers off node hosts. Each deployment shares one
250 tWRF funded bond for serving and consuming. Governance, owner, payout,
server and consumer approvals sign the exact authorization, with chain and
network binding. Use dedicated server and consumer keys. No dry-run fetch
counts as installed service.

## Offline authorization

Start with a public `NodeBondLifecycle` transaction packet. Its sender and nonce
must match the authorization; supply real transaction inputs, a funded fee,
the owner-approved genesis, deployment, bindings and effective epoch. Never
place private keys in the packet. Deposit locks existing balance; exit and
withdrawal require separate owner-authorized packets and nonces.

Run `wrfcoin node bond sign --transaction-file packet.json --role owner
--chain-id 777 --network-id testnet --out owner-signed.json` on the owner machine.
Each required signer takes the resulting public packet and writes a new packet
using `--role server`, `consumer`, `governance` or `payout`. All five signatures
are needed for registration/update. Only the owner signs exit/withdrawal.
The signer is resolved through the production BlockSigner loader, and must
match the role's public key in the packet. Wrong-key or online exit signing is
refused before signing. Output files must be new; existing files are preserved.

Configure the dedicated signer using `WRFCOIN_NODE_BOND_SIGNER_KEY_FILE` in a
debug build, or build with `--features hsm` and configure
`WRFCOIN_HSM_NODE_BOND_KEY_LABEL` plus the public-key label. There is no shared
HSM-key fallback. HSM PIN input uses the hidden terminal prompt; credential
variables carrying PIN values are refused. Do not pass keys or PINs in argv.

## Authenticated submission and inspection

After owner-authorized activation, use the normal CLI API credential configuration:

- `wrfcoin node bond register-bond --transaction-file fully-signed.json`
- `wrfcoin node bond inspect --deployment-id <32-byte-hex>`
- `wrfcoin node bond epoch --epoch <integer>`
- `wrfcoin node bond exit --transaction-file owner-exit.json`
- `wrfcoin node bond withdraw --transaction-file owner-withdrawal.json`
- `wrfcoin snapshot submit-receipt --transaction-file owner-receipt-batch.json`
- `wrfcoin node bond receipt --receipt-id <claim-hash-hex>`

Reader authorization is required for reads; Operator authorization is required
for submits. Bond submit authenticates the lifecycle approval and runs ordinary
bounded mempool admission at the locally observed canonical height. Admission
returns `included:false`: it is not a funded registration, withdrawal or payment.
Consensus executes state, bond funding, nonce, fee and liability checks when a
block carries the transaction. Retain the signed public packet for retry; inspect
canonical state before retrying an uncertain response.

An unavailable consensus section returns an unavailable error. Missing
registration, epoch or retained receipt is UNKNOWN, with null values rather
than a fabricated zero. Receipt inclusion comes from accepted consensus claims,
not the process-local service ledger or HTTP acknowledgement. Pruned receipts
may be UNKNOWN; retain inclusion evidence through the required horizon.

For actual service use `wrfcoin snapshot fetch --bonded --receipt-archive <dir>`
with trusted registered server signers and an isolated fresh receiver API.
That path verifies, installs, archives and obtains dedicated receiver signing.
Never target a producer or live standby database. S6b supplies receiver scheduling,
registered ON parity evidence, and outage/restart/reorg procedures; automation
and deployment are outside this S6a CLI change.
