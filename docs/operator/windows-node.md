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
(deny); the node syncs either way. (If you later open your port, below, you
remove that "deny" again.)

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

## Open your port (optional)

By default the node only connects out. Opening one port lets other nodes connect
to yours, which a bonded node needs. You do it in four steps: give the PC a fixed
address on your network, forward the port on your router, allow it in Windows
Firewall, and tell the node its public address. Only **TCP 9000** (the node's
peer-to-peer port) is opened; the status page and its API stay on this PC
(127.0.0.1) and are never reachable from outside.

**1. This PC's address on your network.** In PowerShell:

```powershell
Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.PrefixOrigin -in 'Dhcp','Manual' } | Select-Object InterfaceAlias, IPAddress
```

Note the address of the adapter you use (for example `192.168.1.20`). In your
router's settings, reserve that address for this PC (often called "DHCP
reservation" or "static lease"), so the forward below keeps pointing at it.

**2. Router: forward TCP 9000.** In the router's port-forwarding page add one
rule: protocol **TCP**, external port **9000**, internal address = the address
from step 1, internal port **9000**. Nothing else. Also note your router's
**public (WAN) address**, shown on its status page (or search "what is my IP"
from this PC). If it changes now and then, a free dynamic-DNS name for it works
too.

**3. Windows Firewall: allow the node on TCP 9000.** Open PowerShell **as
administrator** (Start → type PowerShell → Run as administrator) and run the
lines below. If Windows asked for a *different* administrator account's
password, replace `$env:LOCALAPPDATA` with `C:\Users\<your user name>\AppData\Local`.

```powershell
$node = "$env:LOCALAPPDATA\Programs\WRFCoin Node\launch_api.exe"
# If you answered "Cancel" to the first firewall prompt, Windows created block
# rules for the node; a block rule wins over an allow rule, so remove them.
Get-NetFirewallApplicationFilter -Program $node | Get-NetFirewallRule |
  Where-Object { $_.Direction -eq 'Inbound' -and $_.Action -eq 'Block' } | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'WRFCoin node P2P (TCP 9000)' -Direction Inbound -Action Allow `
  -Protocol TCP -LocalPort 9000 -Program $node -Profile Any
```

The rule admits only `launch_api.exe`, only on TCP 9000. To undo it later:
`Remove-NetFirewallRule -DisplayName 'WRFCoin node P2P (TCP 9000)'`.

**4. Tell the node.** Create `%LOCALAPPDATA%\WRFCoin\reachable.conf`
(`notepad "$env:LOCALAPPDATA\WRFCoin\reachable.conf"`) with these two lines,
your public address in the second:

```
WRFCOIN_P2P_HOST=0.0.0.0
WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/203.0.113.7/tcp/9000
```

(With a dynamic-DNS name: `WRFCOIN_P2P_EXTERNAL_ADDR=/dns4/yourname.example.org/tcp/9000`.)
The first line makes the node listen on your network instead of only on this PC;
the second is the address it tells other nodes to dial. The file may contain only
these two settings (and `#` comment lines): anything else, a typo or a malformed
address stops the start with a message saying which line is wrong, and
`launch_api.exe` settings from the installed `chain-888.conf` stay as they are.
Restart the node (Start menu → **Stop WRFCoin Node**, then **WRFCoin Node**).

**Check it.**

- The status page shows your address under **Reachable at** (it says "outbound
  only" while no address is set).
- `Get-NetTCPConnection -LocalPort 9000 -State Listen` lists `0.0.0.0` port 9000.
- From **outside** your network, for example a phone on mobile data with a port
  checker app, or a friend's PC: `Test-NetConnection 203.0.113.7 -Port 9000`
  (your address) should report `TcpTestSucceeded : True`. Testing your public
  address from inside your own network often fails even when everything is
  right, because many routers do not loop back.

To close the port again: delete `reachable.conf`, restart the node, and remove
the router rule and the firewall rule.

## Where the data lives

| What | Where |
|---|---|
| Program, chain-888 config, launcher | `%LOCALAPPDATA%\Programs\WRFCoin Node` |
| Chain database (`WRFCOIN_DATA_DIR`) | `%LOCALAPPDATA%\WRFCoin\data` |
| Node identity key | `%LOCALAPPDATA%\WRFCoin\data\peer_identity.key` |
| Local API operator token | `%LOCALAPPDATA%\WRFCoin\operator.token` |
| Server key (only if you bond, below) | `%LOCALAPPDATA%\WRFCoin\server.key` |
| Open-port settings (optional) | `%LOCALAPPDATA%\WRFCoin\reachable.conf` |
| Node log, launcher errors | `%LOCALAPPDATA%\WRFCoin\logs\node.log`, `logs\launcher.log` |

`%LOCALAPPDATA%\WRFCoin` is restricted to your Windows account: the launcher
removes inherited permissions and grants only your user, so other accounts on the
PC cannot read the identity key or token. Start menu → **WRFCoin Node Data Folder**
opens it.

## Server key for bonding (testnet only)

A bonded node registers a **server key** and signs every snapshot it serves with
it. Linux nodes keep that key in a hardware token (PKCS#11); on testnet a Windows
node may keep it in a file instead. Create it once, in PowerShell:

```powershell
& "$env:LOCALAPPDATA\Programs\WRFCoin Node\wrfcoin-node.ps1" -Action ServerKey
```

This writes `%LOCALAPPDATA%\WRFCoin\server.key` (a random ed25519 seed, 64 hex
characters), readable only by your Windows account. It never replaces an
existing key. Start the node again (Start menu → **Stop WRFCoin Node**, then
**WRFCoin Node**); from then on the launcher passes the file to the node as
`WRFCOIN_SNAPSHOT_SERVER_KEY_FILE` and `node.log` shows

```
[signer] TESTNET software server key active: KeyFileSigner(...server.key) (public key <64 hex>)
```

That public key is the `--server-key` of your bond. The node still never
produces blocks; it now signs the snapshots it serves.

Rules the node enforces (it refuses to start otherwise):

- **Testnet only.** The key file is accepted only when the node runs with
  `WRFCOIN_NETWORK_ID=testnet` and a testnet chain id (chain 888). Any other
  network, mainnet included, still requires a hardware token.
- **Only your account can read it.** The file's owner and every allow entry
  must be you, SYSTEM or Administrators. The launcher locks the file on every
  start; if you copy it elsewhere, lock it again (`icacls <file> /inheritance:r
  /grant:r "%USERNAME%:F"`).
- **Not together with an HSM.** If `WRFCOIN_HSM_MODULE_PATH` is also set, the node
  stops: the token is the default signer, so pick one.

Back up `server.key` somewhere only you can read: a new key means a new bond. To
stop serving as a bonded node, stop the node and move the file out of the folder.

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
creates a testnet server key (`-Action ServerKey`), starts with it and checks the
node logs it, checks that `launch_api.exe` refuses a copy of the key that
`BUILTIN\Users` can read, starts it with a `reachable.conf` (checks the `0.0.0.0:9000` listener, the
announced address on `/network/peer_id` and the dashboard's "Reachable at") and
checks that a `reachable.conf` with any other key stops the start, uninstalls it, and uploads `wrfcoin-node-windows-x86_64-setup` (installer +
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
two per-user values, `WRFCOIN_DATA_DIR` and `WRFCOIN_OPERATOR_TOKEN`, and the
optional `reachable.conf` overrides, limited to `WRFCOIN_P2P_HOST` and
`WRFCOIN_P2P_EXTERNAL_ADDR`.
