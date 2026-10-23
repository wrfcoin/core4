# Build a faucet bond-grant request

Run this on the node whose peer identity will claim the grant:

```sh
wrfcoin --output json node bond grant-request \
  --owner-address 0xYOUR40HEXCHARACTEROWNERADDRESS \
  --api-url http://127.0.0.1:8556 --out grant-request.json
```

The command uses the same dedicated bond signer and hidden PIN prompt as
`node bond sign --role server`. Build with `--features hsm` for PKCS#11 support;
configure the existing `WRFCOIN_HSM_*` settings, including the dedicated
`WRFCOIN_HSM_NODE_BOND_KEY_LABEL` and `WRFCOIN_HSM_NODE_BOND_PUBKEY_LABEL`.
It never falls back to a shared HSM role. For a software seed, pass
`--key-file <file>` containing a 32-byte ed25519 seed encoded as hex.
The existing infra `testnet/services/faucet/bond-grant-request.js` is another
request builder for seed-file nodes.

Peer possession requires the node's existing libp2p protobuf ed25519 private
key. The default is `$WRFCOIN_DATA_DIR/peer_identity.key`, or
`./data/peer_identity.key` when that variable is unset, matching launch_api.
Use `--p2p-key-file <path>` to override it. The CLI never creates a peer key.
Both explicit key files must be regular, current-user-owned mode 0600 files;
symlinks and non-ed25519 peer identities are refused.

The only default network operations are GET `/network/info` and GET
`/chain/block/<tip-2>`. The peer key must derive the exact `local_peer_id` the
node reports. Chain and network IDs come from that node response. The request
carries a server-key signature over the faucet message and a separate
peer-key signature over the peer domain followed by that same message.
No private key bytes are printed or included in the request.

The JSON request appears in the standard CLI output envelope's `data` field.
`--out` writes the unwrapped request to a new file, refusing to overwrite an
existing file. Inspect it before submission. Only `--submit <backend-base-url>`
performs network writes: first POST `/api/onboarding/faucet/challenge`, then
display the backend question on stderr and read the operator answer from stdin.
The CLI never solves it. It appends `challengeId` and `answer` to the signed
request and POSTs `/api/faucet/bond-grant`. Each POST has one attempt, without
retries or redirects. Empty/invalid stdin refuses before the grant POST.
The command returns the grant response status and body alongside
the request in the output envelope. A transport error may leave the outcome
unknown; inspect the faucet audit before manually retrying.

A signed request is not a grant approval or an on-chain bond deposit. Faucet
probation, spacing, identity and funding gates still apply. This command does
not enable the faucet or change its owner-controlled settings.
