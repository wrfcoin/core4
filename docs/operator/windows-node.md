# Running a WRFCoin node on Windows

One download, no WSL, Docker, Rust or command line. The installer sets up a
**sync-only** node for testnet chain 888: it downloads and checks the whole chain
from the public seed, makes outbound connections only, and holds no
block-signing key.

Requirements: 64-bit Windows 10 or 11, free disk space for the chain (it grows),
and an internet connection. No administrator rights are needed.

## Install

1. Download `wrfcoin-node-<version>-windows-x86_64-setup.exe` and its `SHA256SUMS`.
   Optional check, in PowerShell from the download folder:
   `Get-FileHash .\wrfcoin-node-*-setup.exe -Algorithm SHA256` must print the hash
   in `SHA256SUMS`.
2. Run the installer. **It is not code-signed yet**, so Windows SmartScreen shows
   "Windows protected your PC". Click **More info**, then **Run anyway**.
3. Click through the wizard. You can tick "Start the WRFCoin node when I sign in to
   Windows" (off by default). On the last page, leave "Start the WRFCoin node now"
   ticked.

The installer puts the program in `%LOCALAPPDATA%\Programs\WRFCoin Node` and adds
a **WRFCoin Node** folder to the Start menu.

## Start and stop

- **Start:** Start menu → **WRFCoin Node**. The node starts in the background (no
  window) and your browser opens its status page once it is up. Choosing the
  entry again while the node runs just opens the page again.
- **Stop from the status page:** press **Stop node** on the page and confirm. The
  node disconnects from other nodes, saves its place and exits (the same steps as
  Ctrl+C); once it no longer answers, the page says "Node stopped. Start it again
  from Start menu → WRFCoin Node." The button only works on a page opened from the
  **WRFCoin Node** entry (or after you paste the operator token, below).
- **Stop from the Start menu:** Start menu → **Stop WRFCoin Node**. This sends the
  node the same signal as pressing Ctrl+C, waits until it has saved its place and
  exited, and then says so. It always works, even with the page closed.

The next start resumes from where the node stopped.

The node keeps running after you close the browser. It writes its log to
`%LOCALAPPDATA%\WRFCoin\logs\node.log` (the previous run's log is `node.log.1`).
Only one node can run at a time (it uses ports 8556 and 9000 on this PC).

### The firewall prompt

Windows may ask whether to allow `launch_api.exe` on networks. A sync-only node
needs **no inbound access**: it only connects out to the seed. Choose **Cancel**
(deny); the node syncs either way.

## Check that it is syncing

Start menu → **WRFCoin Node Status** opens `http://127.0.0.1:8556/dashboard` in
your browser. The chain panel shows the height climbing and the number of peers.
The raw numbers are at `http://127.0.0.1:8556/network/info`: look for
`"chain_id":"888"`, `chain_height` going up, and `peer_count` of 1 or more.

From an empty folder the node reached about 9,700 blocks in 15 minutes on a laptop.
The page only answers on this PC (127.0.0.1); nothing is exposed to your network.

### The operator token, and how the page gets it

The **Stop node** button and the host panel (CPU, memory, disks) need this
node's operator token. It is in `%LOCALAPPDATA%\WRFCoin\operator.token`
(created at install, unique to this PC, readable only by your Windows account).
You do not need it to watch the sync.

When you open the page with Start menu → **WRFCoin Node**, the launcher hands the
token over for you: it opens `http://127.0.0.1:8556/dashboard#operator=<token>`.
The part after `#` is a URL *fragment*: browsers never send it to the node, so it
appears in no request and no log. The page moves it into that browser tab's
session storage (gone when the tab closes) and immediately removes it from the
address bar. The **WRFCoin Node Status** shortcut and a typed or bookmarked URL
open the page without the token; there, paste the token into the field under
**Host** (on another computer, or a node you run some other way, paste works the
same). **Forget** clears it from the tab.

## Where the data lives

| What | Where |
|---|---|
| Program, chain-888 config, launcher | `%LOCALAPPDATA%\Programs\WRFCoin Node` |
| Chain database (`WRFCOIN_DATA_DIR`) | `%LOCALAPPDATA%\WRFCoin\data` |
| Node identity key | `%LOCALAPPDATA%\WRFCoin\data\peer_identity.key` |
| Local API operator token | `%LOCALAPPDATA%\WRFCoin\operator.token` |
| Node log, launcher errors | `%LOCALAPPDATA%\WRFCoin\logs\node.log`, `logs\launcher.log` |

`%LOCALAPPDATA%\WRFCoin` is restricted to your Windows account: the launcher
removes inherited permissions and grants only your user, so other accounts on the
PC cannot read the identity key or token. Start menu → **WRFCoin Node Data Folder**
opens it.

## Uninstall

Start menu → **Uninstall WRFCoin Node**, or Settings → Apps → **WRFCoin Node** →
Uninstall. The uninstaller stops a running node cleanly first, then asks whether to delete the chain
data and identity key too; answer **No** to keep them, so a reinstall resumes
where it stopped. To remove them later, delete `%LOCALAPPDATA%\WRFCoin`.

## Troubleshooting

- **Nothing happens, or a window flashes and nothing opens:** look in
  `%LOCALAPPDATA%\WRFCoin\logs\launcher.log`. The shortcuts run the launcher with
  `-ExecutionPolicy Bypass`, which a company Group Policy can override; ask your
  administrator, or use a personal PC.
- **"The node stopped during startup":** the message shows the end of
  `node.log`. "Address in use" means another node (or another program on port
  8556 or 9000) is already running.
- **Height stays at 0:** check the internet connection; the seed is
  `seed.wrfcoin.com` port 9000 (outbound TCP).

## Not done yet

- **Code signing.** The installer and `launch_api.exe` are unsigned, hence the
  SmartScreen warning.
- **Windows Service / tray icon.** The node runs in the background only while you
  are signed in, and there is no tray icon. A service would run it without a window and before sign-in; it needs
  a service host (the node binary does not speak the Service Control Manager
  protocol yet: either native support via the `windows-service` crate, or a
  wrapper), a dedicated low-privilege account and a data folder ACL'd to that
  account instead of the user, an installer that runs elevated (per-machine), and
  the graceful-stop path wired to the service stop request.

## For maintainers: how the package is built

`packaging/windows/` holds everything; `.github/workflows/windows-node-build.yml`
builds `launch_api.exe` (`--features full-mining,p2p`), packages it with Inno
Setup 6, installs it silently on the runner, starts it through its Start-menu
shortcut (checking it has no visible window), checks `/network/info` reports
chain 888 with the pinned genesis hash and that `/dashboard` carries the Stop
button, stops it through the "Stop" shortcut (checking `node.log` shows a graceful
shutdown), starts it again and stops it through the dashboard's route
`POST /node/shutdown` (refused without the operator token and with a wrong one,
accepted with the installed token, then the same graceful shutdown in `node.log`),
uninstalls it, and uploads `wrfcoin-node-windows-x86_64-setup` (installer +
`SHA256SUMS`).

The node is configured only by environment variables. `chain-888.conf` is the
exact set, one `KEY=VALUE` per line with no comments or quotes, generated from the
infra repository and committed:

```sh
python3 packaging/windows/gen_chain888_config.py --infra ../infra          # write
python3 packaging/windows/gen_chain888_config.py --infra ../infra --check  # verify
```

It reads infra `testnet/cut-pins.env` (chain identity and genesis pins, parsed the
same way as infra `testnet/scripts/chain-identity.py`, so inline `# comments`
after quoted JSON are handled) and the `wrfcoin-node` service of
`testnet/docker-compose.testnet.yml` (oracle-governance allowlist, producer-auth
flag), and refuses any missing, placeholder or malformed pin. Regenerate and
commit it whenever the cut pins change. The launcher `wrfcoin-node.ps1` adds the
two per-user values, `WRFCOIN_DATA_DIR` and `WRFCOIN_OPERATOR_TOKEN`.
