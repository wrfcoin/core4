# macOS sync-peer release packet

This packet installs a non-producing testnet peer on Apple Silicon arm64 Macs,
specifically the M4 target, after the actual hardware and release artifact are
selected. Intel macOS is not supported: the installer intentionally rejects
non-arm64 hosts and artifacts. This packet does not include a binary, archive,
checksum, signature, bootstrap address, or credential. Those are explicit
inputs; no build or release authenticity claim is made by this repository
change.

## Inputs and architecture

Obtain one reviewed release artifact from the release owner:

- the native arm64 executable named `wrfcoin-node`, its build manifest, and
  SHA-256 values;
- detached signatures for both the executable and build manifest, plus the
  already trusted release-key fingerprint and exact source commit.

Copy `sync-peer.conf.example` to
`~/Library/Application Support/WRFCoinSyncPeer/sync-peer.conf`, then fill only
the artifact path/checksum, both signature paths, build manifest path, source
commit, signer fingerprint, and approved public bootstrap multiaddr. The
documented default path is used in place and preserved by the installer; set
`WRFCOIN_SYNC_PEER_CONFIG` only when supplying an alternate source file. The
network, chain, and genesis values are frozen in the template. Do not change
them to make a peer connect: a mismatch means stop and obtain the current cut
packet.

The configuration is intentionally not a general node configuration. The
wrapper rejects role-bearing settings and launches the node through an empty,
allowlisted environment. The API must remain on `127.0.0.1:8556`.

## Install and control

Run from this directory after reviewing the inputs:

```bash
./install.sh --verify-only
./install.sh
./control.sh status
./healthcheck.sh
./control.sh stop
./control.sh start
./control.sh uninstall
```

`install.sh` invokes the shared signed-artifact verifier before copying
anything. It requires the raw artifact, build manifest, both detached
signatures, approved primary fingerprint, exact source commit, and matching
SHA-256 values; absent or mismatched material cannot reach install or
launchd. It installs `sync-peer.sh` and its required `common.sh` runtime
helper together, then registers the launchd agent and starts it. `uninstall`
removes only the launchd registration; peer data, including both runtime shell
files, and logs are retained for diagnosis or rollback.

## Health, rollback, and diagnostics

`healthcheck.sh` requires all of the following: the frozen identity from
`/network/info`, `sync_status` equal to `synced`, at least one peer, and a
positive chain height. It is a local, read-only check.

For a failed update, stop the agent, preserve the bounded logs, restore the
previous verified artifact/configuration, reinstall, and rerun the health
check. Reinstall the package rather than restoring `sync-peer.sh` alone so it
and `common.sh` remain version-matched. Do not merge chain data from another
cut.

```bash
./control.sh stop
tail -n 200 "$HOME/Library/Logs/WRFCoinSyncPeer/node.out.log"
tail -n 200 "$HOME/Library/Logs/WRFCoinSyncPeer/node.err.log"
# Restore the previously verified local artifact and its recorded checksum.
./install.sh
./healthcheck.sh
```

For support, collect the two bounded log tails plus the outputs of
`./control.sh status`, `curl -sS http://127.0.0.1:8556/node/sync/status`, and
`curl -sS http://127.0.0.1:8556/network/info`. Do not send any role authority
or private configuration in that report.
