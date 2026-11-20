<#
.SYNOPSIS
  Starts, stops or prepares the WRFCoin sync-only node for chain 888 (the
  Windows package launcher). No console window is shown to the user.

.DESCRIPTION
  -Action Start (default)
      If the node is not running, starts launch_api.exe in the background with
      the environment from chain-888.conf (installed next to this script,
      generated from the infra cut pins), the per-user data folder and the
      operator token; its output goes to %LOCALAPPDATA%\WRFCoin\logs\node.log.
      Then opens the node's status page in the default browser
      (/dashboard, or /network/info on a build without the dashboard). The
      dashboard URL carries the operator token in its fragment
      (/dashboard#operator=<token>): browsers never send a fragment to the
      server, and the page moves it into the tab's session storage and clears
      it from the address bar, so the page's Stop button and host panel work
      without copying the token by hand.
  -Action Stop
      Sends the running node Ctrl+C on its own hidden console (the same signal
      as pressing Ctrl+C in a node window) and waits for it to finish its
      shutdown checkpoint and exit.
  -Action Setup
      Creates %LOCALAPPDATA%\WRFCoin, restricts it to the current user and
      creates the operator token. The installer runs this once.
  -Action ServerKey
      Creates %LOCALAPPDATA%\WRFCoin\server.key (testnet only): the node's
      software server key, the key a bonded node registers and signs the
      snapshots it serves with. Never overwrites an existing key. Once the file
      exists, every Start locks it to the current user and passes it to the
      node as WRFCOIN_SNAPSHOT_SERVER_KEY_FILE; the node logs its public key.
  -Action NewOracleKey
      Creates %LOCALAPPDATA%\WRFCoin\oracle.key (testnet only): the oracle
      attester's software key, locked to the current user before the seed is
      written and never shown. Never overwrites an existing key. Also creates
      oracle.token, the node's Oracle-role API token. Writes only the PUBLIC key
      (the oracle id, from wrfcoin.exe oracle public-key) to oracle-public.txt
      in the data folder and on the Desktop as wrfcoin-oracle-public.txt (every
      node Start names oracle-public.txt to the node as
      WRFCOIN_ORACLE_PUBLIC_KEY_FILE, for the status page's attester rung). Once
      oracle.key exists, every node Start passes oracle.token to the node as
      WRFCOIN_ORACLE_TOKEN; restart a running node to pick it up.
  -Action StartAttester
      Starts oracle_attester.exe in the background against the local node,
      with chain-888.conf, attester-888.conf (installed next to this script)
      and the operator's own %LOCALAPPDATA%\WRFCoin\attester.conf layered on
      top, oracle.key as WRFCOIN_TESTNET_ORACLE_KEY_FILE and oracle.token as its
      API token. The node must be running. Output goes to logs\attester.log.
      The reference mode is a setting the operator must choose: until
      attester.conf sets WRFCOIN_ATTESTER_REFERENCE_MODE (and, for shadow or
      per_station, WRFCOIN_ATTESTER_MAX_REFERENCE_KM), the start stops with a
      message naming that file. If server.key exists, its public key is added
      to the attester's refused snapshot keys, so the oracle key can never be
      the server key.
  -Action StopAttester
      Sends the attester Ctrl+C on its hidden console and waits for it to exit.
      -Action Stop stops the attester too, before the node.
  -Action ReceiptTimer
      For a bonded node (testnet): registers a per-user Scheduled Task,
      \WRFCoin\WRFCoin receipt, that runs -Action Receipt every 30 minutes
      while this user is signed in (no admin). Needs bond.conf and owner.key
      (below); stops with a message if either is missing or bond.conf is wrong.
      Running it again replaces the task.
  -Action Receipt
      One pass of `wrfcoin.exe node receipt auto` for the bond in bond.conf:
      reads this epoch's claim from the claim mailbox, signs it once with
      owner.key, keeps the packet in receipts\ and posts it to this node,
      re-posting the same packet on later passes until it is included. One
      line per pass in logs\receipt.log. Never shows a window.
  -Action StopReceiptTimer
      Removes the Scheduled Task (the uninstaller does this too).
  -Action AddressTimer
      For a bonded node (testnet): registers a per-user Scheduled Task,
      \WRFCoin\WRFCoin address, that runs -Action PublishAddress every 60
      minutes while this user is signed in (no admin). Needs bonded mode
      (reachable.conf with an /ip4/ external address, server.key) and
      address.conf (below); stops with a message if anything is missing.
      Running it again replaces the task.
  -Action PublishAddress
      One pass of `wrfcoin.exe node address publish`: signs this node's
      address record once per epoch with server.key and the node's peer key,
      and posts it to the address mailbox (later passes in the same epoch
      re-post the same bytes). While the mailbox is not open yet, or the node
      is not running, a pass just waits for the next one. One line per change
      in logs\address.log (repeats are not written again).
  -Action StopAddressTimer
      Removes the address Scheduled Task (the uninstaller does this too).
  -Action AttesterTimer
      Turns attester auto-start on: registers a per-user Scheduled Task,
      \WRFCoin\WRFCoin Attester, that runs -Action AttesterAutoStart every 5
      minutes while this user is signed in (no admin), and clears an earlier
      opt-out. Needs oracle.key (testnet). Every node Start and 'Create
      Attester Key' do this by themselves once oracle.key exists, unless
      auto-start was turned off.
  -Action AttesterAutoStart
      One pass of the seat gate. Starts the attester only when this node's
      oracle id is in the chain's oracle set AND the tip is at least 3 blocks
      past the first height this launcher saw it seated (kept in
      attester-seat.json), so the set change is at least 3 blocks deep. While
      not seated it waits (leaving the set restarts the count; it never stops
      a running attester). One line per change in logs\attester-auto.log.
  -Action AttesterAutoStartOff
      The opt-out: writes attester-autostart.off and removes the task. Node
      Starts then never re-register it; -Action AttesterTimer turns it back on.
      A running attester keeps running until stopped.
  -Action StopAttesterTimer
      Removes the attester Scheduled Task only (the uninstaller does this).
  -Action Status
      Opens the running node's status page (the "WRFCoin Node Status" entry),
      on the API port reachable.conf chooses. Starts nothing.

  -NoBrowser  start without opening the browser (sign-in start, CI).

  Opening a port (optional): %LOCALAPPDATA%\WRFCoin\reachable.conf may set
  ONLY these keys, one KEY=VALUE per line (blank lines and # comments allowed):
    WRFCOIN_P2P_HOST=0.0.0.0                        an IPv4 address to listen on
    WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<ip>/tcp/<port>  the address other nodes dial
                              (or /dns4/<name>/tcp/<port>)
  Two nodes on one PC (optional): the same reachable.conf may also move the
  node's local ports off the defaults (API 8556, P2P 9000):
    WRFCOIN_API_PORT=8566                   the local API and status page port
    WRFCOIN_P2P_PORT=9010                   the P2P listen port
  Each is a whole number 1024-65535 and the two must differ. Every action
  that talks to the node (status page, attester, receipt and address passes,
  attester auto-start) follows the chosen API port.
  They override the same keys of chain-888.conf; any other key, a duplicate or
  a malformed value stops the start with a message. chain-888.conf itself is
  still checked as the chain-888 sync-only config. See docs/operator/
  windows-node.md, "Open your port".

  Bonded mode (optional, testnet): the same reachable.conf may also set
    WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=1   serve bonded snapshot fetches over
                              P2P (exactly 1; remove the line to turn it off)
    WRFCOIN_SERVICE_ID=<alias>              the bond's service alias, 1-128 of
                              A-Z a-z 0-9 . _ - (starting with a letter or digit)
  Bonded mode is all or nothing: the start stops with a message unless both
  lines are there together with a non-loopback WRFCOIN_P2P_HOST, a
  WRFCOIN_P2P_EXTERNAL_ADDR and server.key (-Action ServerKey). The node stays
  sync-only (WRFCOIN_SYNC_ONLY=1): it never makes blocks, and the server key
  still signs the snapshots it serves. See windows-node.md, "Bonded mode".
  -Quiet      never show a message box (installer, CI).

  Per-user state lives in %LOCALAPPDATA%\WRFCoin:
    data\              chain database and peer_identity.key (WRFCOIN_DATA_DIR)
    logs\node.log      node output (previous run: node.log.1)
    logs\launcher.log  this script's own errors
    operator.token     the node's local API operator token
    server.key         optional testnet server key (-Action ServerKey)
    oracle.key         optional testnet oracle attester key (-Action NewOracleKey)
    oracle.token       the node's Oracle-role API token (created with oracle.key)
    oracle-public.txt  the oracle key's public half (safe to share)
    logs\attester.log  oracle attester output (previous run: attester.log.1)
    reachable.conf     optional: open a P2P port, bonded mode, local ports (see above)
    bond.conf          a bonded node's receipt settings, only these keys:
                         WRFCOIN_BOND_DEPLOYMENT_ID  the bond id (64 lowercase hex)
                         WRFCOIN_BOND_OWNER_ADDRESS  the bond owner (0x + 40 lowercase hex)
                         WRFCOIN_CLAIM_MAILBOX_URL   https://... (the claim mailbox's /claims base)
    owner.key          the bond owner's key, locked to this user; signs receipts
    receipts\          one signed receipt packet per epoch (public data)
    logs\receipt.log   one line per receipt pass
    address.conf       a bonded node's address-record settings, only these keys:
                         WRFCOIN_BOND_DEPLOYMENT_ID   the bond id (64 lowercase hex)
                         WRFCOIN_ADDRESS_MAILBOX_URL  https://... (the address mailbox route)
    addresses\         this epoch's signed address record (address-record-epoch-<E>.json)
    logs\address.log   one line per address-publish outcome
    attester-seat.json      the first height the launcher saw the oracle id seated
    attester-autostart.off  present = attester auto-start turned off (opt-out)
    logs\attester-auto.log  one line per attester auto-start outcome
    attester.conf      attester settings the operator chooses (see StartAttester); only
                       these WRFCOIN_ATTESTER_* keys: REFERENCE_MODE, MAX_REFERENCE_KM,
                       NWS_STATION, DEM_ELEVATIONS_FILE, POLL_SECS, MAX_AGE_SECS,
                       MAX_FUTURE_SKEW_SECS, REFERENCE_MAX_PAIR_SECS,
                       REFERENCE_LOOKUP_TIMEOUT_SECS, MAX_REFERENCE_STATIONS_PER_CYCLE
  The folder is restricted to the current user (inheritance removed, one
  full-control entry for the user's SID).
#>
[CmdletBinding()]
param(
    [ValidateSet('Start', 'Stop', 'Status', 'Setup', 'ServerKey', 'NewOracleKey', 'StartAttester', 'StopAttester', 'ReceiptTimer', 'Receipt', 'StopReceiptTimer', 'AddressTimer', 'PublishAddress', 'StopAddressTimer', 'AttesterTimer', 'AttesterAutoStart', 'AttesterAutoStartOff', 'StopAttesterTimer')]
    [string]$Action = 'Start',
    [switch]$NoBrowser,
    [switch]$Quiet
)

# Started from PowerShell 7 (or anything it launched, such as an installer),
# Windows PowerShell 5.1 inherits a PSModulePath that points at PowerShell 7's
# modules and then cannot load its own Get-Acl, Invoke-WebRequest, ... Use the
# Windows PowerShell module path before the first cmdlet runs.
if ($PSVersionTable.PSEdition -ne 'Core') {
    $env:PSModulePath = @(
        "$env:USERPROFILE\Documents\WindowsPowerShell\Modules",
        "$env:ProgramFiles\WindowsPowerShell\Modules",
        "$env:SystemRoot\System32\WindowsPowerShell\v1.0\Modules"
    ) -join ';'
}

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

$AppDir    = Split-Path -Parent $MyInvocation.MyCommand.Path
$Exe       = Join-Path $AppDir 'launch_api.exe'
$Config    = Join-Path $AppDir 'chain-888.conf'
$Root      = Join-Path $env:LOCALAPPDATA 'WRFCoin'
$DataDir   = Join-Path $Root 'data'
$LogDir    = Join-Path $Root 'logs'
$LogFile   = Join-Path $LogDir 'node.log'
$LauncherLog = Join-Path $LogDir 'launcher.log'
$TokenFile = Join-Path $Root 'operator.token'
$ServerKeyFile = Join-Path $Root 'server.key'
$ReachableConfig = Join-Path $Root 'reachable.conf'
$AttesterExe = Join-Path $AppDir 'oracle_attester.exe'
$CliExe = Join-Path $AppDir 'wrfcoin.exe'
$AttesterConfig = Join-Path $AppDir 'attester-888.conf'
$AttesterUserConfig = Join-Path $Root 'attester.conf'
$OracleKeyTmp = Join-Path $Root 'oracle.key.tmp'
# attester.conf keys the operator may set (without the WRFCOIN_ATTESTER_ prefix).
# The refused public keys, node URL and token always come from the package.
$AttesterUserKeys = @('REFERENCE_MODE', 'MAX_REFERENCE_KM', 'NWS_STATION', 'DEM_ELEVATIONS_FILE', 'POLL_SECS',
    'MAX_AGE_SECS', 'MAX_FUTURE_SKEW_SECS', 'REFERENCE_MAX_PAIR_SECS', 'REFERENCE_LOOKUP_TIMEOUT_SECS',
    'MAX_REFERENCE_STATIONS_PER_CYCLE') | ForEach-Object { "WRFCOIN_ATTESTER_$_" }
$OracleKeyFile = Join-Path $Root 'oracle.key'
$OracleTokenFile = Join-Path $Root 'oracle.token'
$OraclePublicFile = Join-Path $Root 'oracle-public.txt'
$AttesterLog = Join-Path $LogDir 'attester.log'
$AttesterStartSeconds = 15
# The only chain-888.conf keys a user may override (case-sensitive). The two
# bonded-mode keys are checked together by Resolve-NodeMode; the two ports by
# Read-LocalNodeConfig (they must differ).
$ReachableKeys = @('WRFCOIN_P2P_HOST', 'WRFCOIN_P2P_EXTERNAL_ADDR', 'WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS', 'WRFCOIN_SERVICE_ID', 'WRFCOIN_API_PORT', 'WRFCOIN_P2P_PORT')
# Of those, the local ports: every action that talks to the node uses them.
$PortKeys = @('WRFCOIN_API_PORT', 'WRFCOIN_P2P_PORT')
$StopWaitSeconds = 120
# Bonded node receipts (testnet): -Action ReceiptTimer / Receipt / StopReceiptTimer.
$BondConfig = Join-Path $Root 'bond.conf'
$BondKeys = @('WRFCOIN_BOND_DEPLOYMENT_ID', 'WRFCOIN_BOND_OWNER_ADDRESS', 'WRFCOIN_CLAIM_MAILBOX_URL')
$OwnerKeyFile = Join-Path $Root 'owner.key'
$ReceiptStateDir = Join-Path $Root 'receipts'
$ReceiptLog = Join-Path $LogDir 'receipt.log'
$ReceiptLogMaxBytes = 1MB
$ReceiptTaskPath = '\WRFCoin\'
$ReceiptTaskName = 'WRFCoin receipt'
$ReceiptIntervalMinutes = 30
# Bonded node address record (testnet): -Action AddressTimer / PublishAddress / StopAddressTimer.
$AddressConfig = Join-Path $Root 'address.conf'
$AddressKeys = @('WRFCOIN_BOND_DEPLOYMENT_ID', 'WRFCOIN_ADDRESS_MAILBOX_URL')
$AddressStateDir = Join-Path $Root 'addresses'
$AddressLog = Join-Path $LogDir 'address.log'
$AddressLogMaxBytes = 1MB
$AddressTaskPath = '\WRFCoin\'
$AddressTaskName = 'WRFCoin address'
$AddressIntervalMinutes = 60
# Attester auto-start (testnet): -Action AttesterTimer / AttesterAutoStart / AttesterAutoStartOff / StopAttesterTimer.
$AttesterTaskPath = '\WRFCoin\'
$AttesterTaskName = 'WRFCoin Attester'
$AttesterIntervalMinutes = 5
$AttesterSeatFile = Join-Path $Root 'attester-seat.json'
$AttesterOptOutFile = Join-Path $Root 'attester-autostart.off'
$AttesterAutoLog = Join-Path $LogDir 'attester-auto.log'
$AttesterAutoLogMaxBytes = 1MB
# The set change that seats the oracle must be at least this many blocks deep.
$AttesterSeatDepthBlocks = 3
# Set by a pass that must log a refusal instead of exiting through Fail.
$script:FailThrows = $false

function Show-Message([string]$Text, [string]$Icon = 'Information') {
    # Stop detaches from its own console to signal the node's, so writing to
    # the host can fail afterwards; the message box is what the user sees.
    try { Write-Output $Text } catch { }
    if ($Quiet) { return }
    try {
        Add-Type -AssemblyName PresentationFramework
        [System.Windows.MessageBox]::Show($Text, 'WRFCoin Node', 'OK', $Icon) | Out-Null
    } catch { }
}

function Fail([string]$Message) {
    # The auto-start pass turns a refusal into one deduplicated log line.
    if ($script:FailThrows) { throw [System.InvalidOperationException]::new($Message) }
    try {
        if (Test-Path -LiteralPath $LogDir) {
            Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) $Action failed: $Message"
        }
    } catch { }
    Show-Message "WRFCoin node: $Message" 'Error'
    exit 1
}

function Protect-UserOnly([string]$Path) {
    # Exactly one entry: full control for the current user, by SID (works on
    # non-English Windows and for domain/Microsoft accounts), with inheritance
    # from the parent cut off. Any other entry - inherited or explicit, such as
    # SYSTEM or Administrators copied in by a profile's defaults - is dropped by
    # writing a fresh DACL rather than editing the existing one.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $isDir = [System.IO.Directory]::Exists($Path)
    $current = if ($isDir) { [System.IO.Directory]::GetAccessControl($Path) } else { [System.IO.File]::GetAccessControl($Path) }
    $rules = @($current.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if ($current.AreAccessRulesProtected -and $rules.Count -eq 1 -and $rules[0].IdentityReference -eq $sid `
            -and -not $rules[0].IsInherited) { return }   # already locked: do not re-walk a large tree
    $full = [System.Security.AccessControl.FileSystemRights]::FullControl
    $allow = [System.Security.AccessControl.AccessControlType]::Allow
    $none = [System.Security.AccessControl.PropagationFlags]::None
    if ($isDir) {
        $acl = New-Object System.Security.AccessControl.DirectorySecurity
        $inherit = [System.Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
        $acl.SetAccessRuleProtection($true, $false)
        $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($sid, $full, $inherit, $none, $allow)))
        [System.IO.Directory]::SetAccessControl($Path, $acl)
    } else {
        $acl = New-Object System.Security.AccessControl.FileSecurity
        $acl.SetAccessRuleProtection($true, $false)
        $acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($sid, $full, $allow)))
        [System.IO.File]::SetAccessControl($Path, $acl)
    }
}

function Initialize-UserState {
    foreach ($dir in @($Root, $DataDir, $LogDir)) {
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    }
    # Lock the root first: everything created below it afterwards inherits the
    # user-only entry, including peer_identity.key, which the node writes later.
    Protect-UserOnly $Root
    if (-not (Test-Path -LiteralPath $TokenFile)) {
        [System.IO.File]::WriteAllText($TokenFile, (New-RandomHex))
    }
    Protect-UserOnly $TokenFile
    $identity = Join-Path $DataDir 'peer_identity.key'
    if (Test-Path -LiteralPath $identity) { Protect-UserOnly $identity }
    if (Test-Path -LiteralPath $ServerKeyFile) { Protect-UserOnly $ServerKeyFile }
    if (Test-OracleKey) {
        Protect-UserOnly $OracleKeyFile
        # The Oracle-role token exists only for a node that has an attester key.
        if (-not (Test-Path -LiteralPath $OracleTokenFile)) {
            [System.IO.File]::WriteAllText($OracleTokenFile, '')
            Protect-UserOnly $OracleTokenFile
            [System.IO.File]::WriteAllText($OracleTokenFile, (New-RandomHex))
        }
        Protect-UserOnly $OracleTokenFile
    }
}

function Test-OracleKey {
    # A zero-length oracle.key (an interrupted create) counts as no key.
    $item = Get-Item -LiteralPath $OracleKeyFile -ErrorAction SilentlyContinue
    return ($null -ne $item -and $item.Length -gt 0)
}

function New-RandomHex {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    -join ($bytes | ForEach-Object { $_.ToString('x2') })
}

function New-ServerKey {
    # Testnet only: the node refuses a key file on any other network.
    $cfg = Read-NodeConfig $Config
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'a software server key is for testnet only' }
    if (Test-Path -LiteralPath $ServerKeyFile) {
        Protect-UserOnly $ServerKeyFile
        Show-Message "The server key already exists: $ServerKeyFile (not replaced)."
        return
    }
    # Create the file empty, lock it, then write the seed: it is never readable
    # by anyone else, not even for a moment.
    [System.IO.File]::WriteAllText($ServerKeyFile, '')
    Protect-UserOnly $ServerKeyFile
    [System.IO.File]::WriteAllText($ServerKeyFile, (New-RandomHex))
    Show-Message "Created the node's server key: $ServerKeyFile. Restart the node; its log shows the public key."
}

function Get-OraclePublicKey([string]$KeyFile) {
    # Only the public half comes back; wrfcoin.exe opens the key file through
    # the owner-only check and never prints the seed.
    if (-not (Test-Path -LiteralPath $CliExe)) { Fail "missing $CliExe - reinstall the package" }
    # A native command's stderr must not become a terminating error here; the exit code decides.
    $ErrorActionPreference = 'Continue'
    $out = & $CliExe --output json oracle public-key --key-file $KeyFile 2>$null | Out-String
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) { Fail "wrfcoin.exe could not read the public key of $KeyFile (exit $LASTEXITCODE)" }
    $pub = "$((ConvertFrom-Json $out).data.publicKey)".ToLower()
    if ($pub -notmatch '^[0-9a-f]{64}$') { Fail "wrfcoin.exe returned no public key for $KeyFile" }
    return $pub
}

function Get-ServerPublicKey {
    # A native command's stderr must not become a terminating error here; the exit code decides.
    $ErrorActionPreference = 'Continue'
    $out = & $CliExe --output json node bond key-info --key-file $ServerKeyFile 2>$null | Out-String
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) { Fail "wrfcoin.exe could not read the public key of $ServerKeyFile (exit $LASTEXITCODE)" }
    $pub = "$((ConvertFrom-Json $out).data.public_key)".ToLower()
    if ($pub -notmatch '^[0-9a-f]{64}$') { Fail "wrfcoin.exe returned no public key for $ServerKeyFile" }
    return $pub
}

function Write-OraclePublic([string]$Public) {
    [System.IO.File]::WriteAllText($OraclePublicFile, "$Public`r`n")
    $desktop = [Environment]::GetFolderPath('Desktop')
    if ($desktop -and (Test-Path -LiteralPath $desktop)) {
        [System.IO.File]::WriteAllText((Join-Path $desktop 'wrfcoin-oracle-public.txt'), "$Public`r`n")
    }
}

function New-OracleKey {
    # Testnet only: the attester refuses a key file on any other network.
    $cfg = Read-NodeConfig $Config
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'a software oracle key is for testnet only' }
    if (Test-OracleKey) {
        Protect-UserOnly $OracleKeyFile
        $public = Get-OraclePublicKey $OracleKeyFile
        Write-OraclePublic $public
        Show-Message "The attester key already exists: $OracleKeyFile (not replaced). Oracle public key: $public"
        return
    }
    # Write the seed to oracle.key.tmp (created empty and locked first, so it is
    # never readable by anyone else), then rename it into place: an interrupted
    # run leaves no half-written oracle.key. The seed is never displayed.
    foreach ($stale in $OracleKeyTmp, $OracleKeyFile) {
        if (Test-Path -LiteralPath $stale) { Remove-Item -LiteralPath $stale -Force }   # leftovers of an interrupted create
    }
    try {
        [System.IO.File]::WriteAllText($OracleKeyTmp, '')
        Protect-UserOnly $OracleKeyTmp
        [System.IO.File]::WriteAllText($OracleKeyTmp, (New-RandomHex))
        [System.IO.File]::Move($OracleKeyTmp, $OracleKeyFile)
    } finally {
        # On any failure no partial seed file stays behind (after a successful move it no longer exists).
        if (Test-Path -LiteralPath $OracleKeyTmp) { Remove-Item -LiteralPath $OracleKeyTmp -Force -ErrorAction SilentlyContinue }
    }
    Protect-UserOnly $OracleKeyFile
    Initialize-UserState   # creates and locks oracle.token now that the key exists
    $public = Get-OraclePublicKey $OracleKeyFile
    Write-OraclePublic $public
    Show-Message "Created the attester key: $OracleKeyFile. Oracle public key: $public (also in $OraclePublicFile). Restart the node so it picks up the attester token."
}

function Read-AttesterConfig {
    # The shipped attester-888.conf (flat KEY=VALUE, only WRFCOIN_ATTESTER_*
    # keys), then the operator's attester.conf (like reachable.conf: comments
    # and blank lines allowed, only $AttesterUserKeys) layered on top. Upgrades
    # replace the shipped file and never touch the operator's.
    if (-not (Test-Path -LiteralPath $AttesterConfig)) { Fail "missing $AttesterConfig - reinstall the package" }
    $pairs = [ordered]@{}
    $n = 0
    foreach ($line in [System.IO.File]::ReadAllLines($AttesterConfig)) {
        $n++
        if ($line -eq '') { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${AttesterConfig} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq)
        if ($key -cnotmatch '^WRFCOIN_ATTESTER_[A-Z0-9_]+$') { Fail "${AttesterConfig} line ${n}: only WRFCOIN_ATTESTER_* keys may be set there" }
        if ($pairs.Contains($key)) { Fail "${AttesterConfig} defines $key twice" }
        $pairs[$key] = $line.Substring($eq + 1)
    }
    foreach ($key in 'WRFCOIN_ATTESTER_FORBID_GOVERNANCE_PUBKEY', 'WRFCOIN_ATTESTER_FORBID_PRODUCER_PUBKEY', 'WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY') {
        if (-not $pairs.Contains($key)) { Fail "${AttesterConfig} is missing $key" }
    }
    if (Test-Path -LiteralPath $AttesterUserConfig) {
        $seen = @{}
        $n = 0
        foreach ($raw in [System.IO.File]::ReadAllLines($AttesterUserConfig)) {
            $n++
            $line = $raw.Trim()
            if ($line -eq '' -or $line.StartsWith('#')) { continue }
            $eq = $line.IndexOf('=')
            if ($eq -lt 1) { Fail "${AttesterUserConfig} line ${n} is not KEY=VALUE" }
            $key = $line.Substring(0, $eq).Trim()
            if ($AttesterUserKeys -cnotcontains $key) { Fail "${AttesterUserConfig} line ${n}: $key cannot be set there (only $($AttesterUserKeys -join ', '))" }
            if ($seen.Contains($key)) { Fail "${AttesterUserConfig} defines $key twice" }
            $seen[$key] = $true
            $pairs[$key] = $line.Substring($eq + 1).Trim()
        }
    }
    # Settings the operator must choose; the message names the file to edit.
    $mode = if ($pairs.Contains('WRFCOIN_ATTESTER_REFERENCE_MODE')) { [string]$pairs['WRFCOIN_ATTESTER_REFERENCE_MODE'] } else { '' }
    if ($mode -eq '') {
        Fail "set WRFCOIN_ATTESTER_REFERENCE_MODE (fixed, shadow or per_station) in ${AttesterUserConfig}"
    }
    if (@('fixed', 'shadow', 'per_station') -cnotcontains $mode) {
        Fail "${AttesterUserConfig}: WRFCOIN_ATTESTER_REFERENCE_MODE must be fixed, shadow or per_station (got '$mode')"
    }
    if ($mode -ne 'fixed') {
        $km = if ($pairs.Contains('WRFCOIN_ATTESTER_MAX_REFERENCE_KM')) { [string]$pairs['WRFCOIN_ATTESTER_MAX_REFERENCE_KM'] } else { '' }
        $kmValue = 0.0
        if (-not [double]::TryParse($km, [System.Globalization.NumberStyles]::Float, [System.Globalization.CultureInfo]::InvariantCulture, [ref]$kmValue) -or $kmValue -le 0) {
            Fail "reference mode $mode needs WRFCOIN_ATTESTER_MAX_REFERENCE_KM (a positive number of km) in ${AttesterUserConfig}"
        }
    }
    return $pairs
}

function Get-AttesterProcess {
    # Only this installation's attester.
    $target = [System.IO.Path]::GetFullPath($AttesterExe)
    @(Get-Process -Name oracle_attester -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -ieq $target) } catch { $false }
    })
}

function Start-Attester {
    $cfg = Read-LocalNodeConfig
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'the software oracle key is for testnet only' }
    $api = Get-NodeApiUrl $cfg
    if (@(Get-AttesterProcess).Count -gt 0) { Show-Message 'The WRFCoin attester is already running.'; return }
    if (-not (Test-Path -LiteralPath $AttesterExe)) { Fail "missing $AttesterExe - reinstall the package" }
    if (-not (Test-OracleKey)) { Fail "no attester key yet: use 'Create attester key' first" }
    $att = Read-AttesterConfig
    if (@(Get-NodeProcess).Count -eq 0) { Fail "the node is not running: start it first ('WRFCoin Node')" }
    $token = [System.IO.File]::ReadAllText($OracleTokenFile).Trim()
    # The node accepts the token only if it was started after the key existed.
    try {
        $null = Invoke-WebRequest -Uri "$api/transactions/oracle/pending" -UseBasicParsing -TimeoutSec 10 -Headers @{ Authorization = "Bearer $token" }
    } catch {
        $code = 0
        try { $code = [int]$_.Exception.Response.StatusCode } catch { }
        if ($code -eq 401 -or $code -eq 403) { Fail "the node does not accept the attester token yet: stop and start the node, then start the attester" }
        Fail "the node did not answer $api/transactions/oracle/pending: $($_.Exception.Message)"
    }

    foreach ($key in $cfg.Keys) { [Environment]::SetEnvironmentVariable($key, $cfg[$key], 'Process') }
    foreach ($key in $att.Keys) { [Environment]::SetEnvironmentVariable($key, $att[$key], 'Process') }
    if ($att['WRFCOIN_ATTESTER_NWS_STATION'] -eq 'PHYSICS_ONLY') {
        [Environment]::SetEnvironmentVariable('WRFCOIN_ATTESTER_NWS_STATION', $null, 'Process')
    }
    if (Test-Path -LiteralPath $ServerKeyFile) {
        # This node's own server key is a snapshot key too: refuse it as the oracle key.
        $snap = "$($att['WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY']),$(Get-ServerPublicKey)"
        [Environment]::SetEnvironmentVariable('WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY', $snap, 'Process')
    }
    [Environment]::SetEnvironmentVariable('WRFCOIN_ATTESTER_NODE_URL', $api, 'Process')
    [Environment]::SetEnvironmentVariable('WRFCOIN_ATTESTER_API_TOKEN', $token, 'Process')
    [Environment]::SetEnvironmentVariable('WRFCOIN_TESTNET_ORACLE_KEY_FILE', $OracleKeyFile, 'Process')
    # Nothing else from the node's own secrets goes to the attester.
    foreach ($key in 'WRFCOIN_OPERATOR_TOKEN', 'WRFCOIN_SNAPSHOT_SERVER_KEY_FILE', 'WRFCOIN_ORACLE_TOKEN') {
        [Environment]::SetEnvironmentVariable($key, $null, 'Process')
    }

    if (Test-Path -LiteralPath $AttesterLog) { Move-Item -LiteralPath $AttesterLog -Destination "$AttesterLog.1" -Force }
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $psi.Arguments = "/d /c `"`"$AttesterExe`" >> `"$AttesterLog`" 2>&1`""
    $psi.WorkingDirectory = $Root
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $null = [System.Diagnostics.Process]::Start($psi)

    # The attester checks its configuration and key at startup and exits on a
    # refusal; report that with the end of its log.
    $deadline = (Get-Date).AddSeconds($AttesterStartSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 1
        if (@(Get-AttesterProcess).Count -eq 0) {
            Start-Sleep -Seconds 1
            $tail = if (Test-Path -LiteralPath $AttesterLog) { (Get-Content -LiteralPath $AttesterLog -Tail 15) -join "`n" } else { '' }
            Fail "the attester stopped during startup. Last lines of ${AttesterLog}:`n`n$tail"
        }
    }
    Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) StartAttester: running"
    Show-Message "The WRFCoin attester is running. Log: $AttesterLog"
}

function Read-NodeConfig([string]$Path) {
    # Flat KEY=VALUE lines, no comments or quoting (see gen_chain888_config.py).
    # Split on the first '=' only and keep the rest byte for byte: the genesis
    # sets are JSON, full of quotes that no shell may reinterpret.
    $pairs = [ordered]@{}
    $n = 0
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $n++
        if ($line -eq '') { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${Path} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq)
        if ($key -notmatch '^[A-Z][A-Z0-9_]*$') { Fail "${Path} line ${n} has an invalid key" }
        if ($pairs.Contains($key)) { Fail "${Path} defines $key twice" }
        $pairs[$key] = $line.Substring($eq + 1)
    }
    if ($pairs['WRFCOIN_CHAIN_ID'] -ne '888' -or $pairs['WRFCOIN_SYNC_ONLY'] -ne '1') {
        Fail "${Path} is not the chain-888 sync-only config"
    }
    return $pairs
}

function Test-Ipv4([string]$Text) {
    if ($Text -notmatch '^\d{1,3}(\.\d{1,3}){3}$') { return $false }
    foreach ($octet in $Text.Split('.')) { if ([int]$octet -gt 255) { return $false } }
    return $true
}

function Test-PortValue([string]$Value) {
    if ($Value -cnotmatch '^[1-9][0-9]{3,4}$') { return $false }
    $port = [int]$Value
    return ($port -ge 1024 -and $port -le 65535)
}

function Test-ReachableValue([string]$Key, [string]$Value) {
    switch -CaseSensitive ($Key) {
        'WRFCOIN_P2P_HOST' { return (Test-Ipv4 $Value) }
        'WRFCOIN_P2P_EXTERNAL_ADDR' {
            if ($Value -cnotmatch '^/(ip4|dns4)/([^/\s]+)/tcp/(\d{1,5})$') { return $false }
            $hostPart = $Matches[2]
            $port = [int]$Matches[3]
            if ($port -lt 1 -or $port -gt 65535) { return $false }
            if ($Matches[1] -ceq 'ip4') { return (Test-Ipv4 $hostPart) }
            return ($hostPart.Length -le 253 -and $hostPart -match '^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)+$')
        }
        # Opt-in only: '1' turns it on; to turn it off, remove the line.
        'WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS' { return ($Value -ceq '1') }
        # The bond's service alias (chain limit 128 bytes), plain ASCII only.
        'WRFCOIN_SERVICE_ID' { return ($Value -cmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') }
        # Local ports: plain decimal, no sign or leading zero, 1024-65535.
        { $PortKeys -ccontains $_ } { return (Test-PortValue $Value) }
    }
    return $false
}

function Read-ReachableConfig {
    # Optional, per user, inside the user-only folder. Only $ReachableKeys may
    # appear; everything else about the node stays as chain-888.conf says.
    $pairs = [ordered]@{}
    if (-not (Test-Path -LiteralPath $ReachableConfig)) { return $pairs }
    $n = 0
    foreach ($raw in [System.IO.File]::ReadAllLines($ReachableConfig)) {
        $n++
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${ReachableConfig} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq).Trim()
        $value = $line.Substring($eq + 1).Trim()
        if ($ReachableKeys -cnotcontains $key) {
            Fail "${ReachableConfig} line ${n}: $key cannot be set there (only $($ReachableKeys -join ', '))"
        }
        if ($pairs.Contains($key)) { Fail "${ReachableConfig} defines $key twice" }
        if (-not (Test-ReachableValue $key $value)) { Fail "${ReachableConfig} line ${n}: '$value' is not a valid $key" }
        $pairs[$key] = $value
    }
    return $pairs
}

function Read-LocalNodeConfig($Reach = $null) {
    # chain-888.conf with the operator's port choices from reachable.conf
    # (defaults 8556/9000 when it sets none). Every action that talks to the
    # local node builds its URL from this, so they all follow the same port.
    $cfg = Read-NodeConfig $Config
    if ($null -eq $Reach) { $Reach = Read-ReachableConfig }
    foreach ($key in $PortKeys) {
        if ($Reach.Contains($key)) { $cfg[$key] = $Reach[$key] }
    }
    if ([string]$cfg['WRFCOIN_API_PORT'] -ceq [string]$cfg['WRFCOIN_P2P_PORT']) {
        Fail "${ReachableConfig}: WRFCOIN_API_PORT and WRFCOIN_P2P_PORT are both $($cfg['WRFCOIN_API_PORT']); the API and P2P ports must differ"
    }
    return $cfg
}

function Get-NodeApiUrl($Cfg) {
    return "http://$($Cfg['WRFCOIN_API_HOST']):$($Cfg['WRFCOIN_API_PORT'])"
}

function Resolve-NodeMode($Reach) {
    # 'sync' (the default) or 'bonded'. Bonded mode is an explicit opt-in and
    # all or nothing: a half-configured bonded node would start, look fine and
    # never be paid, so every missing piece stops the start instead.
    $serve = $Reach.Contains('WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS')
    if (-not $serve) {
        if ($Reach.Contains('WRFCOIN_SERVICE_ID')) {
            Fail "${ReachableConfig}: WRFCOIN_SERVICE_ID is only used in bonded mode. Add WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=1 or remove it"
        }
        return 'sync'
    }
    if (-not $Reach.Contains('WRFCOIN_SERVICE_ID')) {
        Fail "${ReachableConfig}: bonded mode needs WRFCOIN_SERVICE_ID=<your bond's service alias>"
    }
    $p2pHost = if ($Reach.Contains('WRFCOIN_P2P_HOST')) { [string]$Reach['WRFCOIN_P2P_HOST'] } else { '' }
    if ($p2pHost -eq '' -or $p2pHost.StartsWith('127.')) {
        Fail "${ReachableConfig}: bonded mode needs WRFCOIN_P2P_HOST=0.0.0.0 so other nodes can connect (see Open your port)"
    }
    if (-not $Reach.Contains('WRFCOIN_P2P_EXTERNAL_ADDR')) {
        $p2pPort = if ($Reach.Contains('WRFCOIN_P2P_PORT')) { [string]$Reach['WRFCOIN_P2P_PORT'] } else { '9000' }
        Fail "${ReachableConfig}: bonded mode needs WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<your public address>/tcp/$p2pPort (see Open your port)"
    }
    if (-not (Test-Path -LiteralPath $ServerKeyFile)) {
        Fail "bonded mode needs the server key ${ServerKeyFile}: run wrfcoin-node.ps1 -Action ServerKey first"
    }
    return 'bonded'
}

function Get-NodeProcess {
    # Only this installation's node, not some other launch_api.exe.
    $target = [System.IO.Path]::GetFullPath($Exe)
    @(Get-Process -Name launch_api -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -ieq $target) } catch { $false }
    })
}

function Test-Url([string]$Url) {
    try { (Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 3).StatusCode -eq 200 } catch { $false }
}

function Get-StatusPageUrl([string]$Api, [bool]$HasDashboard) {
    # /dashboard is the node's own status page (#3599); older builds lack it.
    if (-not $HasDashboard) { return "$Api/network/info" }
    # The token goes in the FRAGMENT only (never a query string): it is not
    # sent to the node, not logged, and the page clears it from the address bar.
    $token = ''
    try { $token = [System.IO.File]::ReadAllText($TokenFile).Trim() } catch { }
    if ($token -match '^[0-9a-f]{64}$') { return "$Api/dashboard#operator=$token" }
    return "$Api/dashboard"
}

function Open-StatusPage([string]$Api) {
    if ($NoBrowser) { return }
    Start-Process (Get-StatusPageUrl $Api (Test-Url "$Api/dashboard"))
}

function Show-StatusPage {
    # The Start-menu "WRFCoin Node Status" entry: the status page of this
    # installation's node, on the API port reachable.conf chooses.
    $api = Get-NodeApiUrl (Read-LocalNodeConfig)
    if (@(Get-NodeProcess).Count -eq 0) { Show-Message "The WRFCoin node is not running: start it with 'WRFCoin Node'."; return }
    Open-StatusPage $api
}

function Start-Node {
    $cfg = Read-NodeConfig $Config
    $reach = Read-ReachableConfig
    $api = Get-NodeApiUrl (Read-LocalNodeConfig $reach)
    if (@(Get-NodeProcess).Count -gt 0) { Open-StatusPage $api; return }
    if (-not (Test-Path -LiteralPath $Exe)) { Fail "missing $Exe - reinstall the package" }
    # Something else (another node on this PC?) already answers on the API
    # port: this node could not bind it, and the status page would be the
    # other program's. Stop with a message that says how to move this one.
    if (Test-Url "$api/health") {
        Fail "another program already answers on $api (another node on this PC?). Stop it, or give this node its own ports in ${ReachableConfig}, e.g. WRFCOIN_API_PORT=8566 and WRFCOIN_P2P_PORT=9010"
    }

    $mode = Resolve-NodeMode $reach
    foreach ($key in $cfg.Keys) { [Environment]::SetEnvironmentVariable($key, $cfg[$key], 'Process') }
    foreach ($key in $reach.Keys) { [Environment]::SetEnvironmentVariable($key, $reach[$key], 'Process') }
    [Environment]::SetEnvironmentVariable('WRFCOIN_DATA_DIR', $DataDir, 'Process')
    [Environment]::SetEnvironmentVariable('WRFCOIN_OPERATOR_TOKEN', ([System.IO.File]::ReadAllText($TokenFile).Trim()), 'Process')
    if (Test-Path -LiteralPath $ServerKeyFile) {
        # Locked to this user by Initialize-UserState; the node checks it again.
        [Environment]::SetEnvironmentVariable('WRFCOIN_SNAPSHOT_SERVER_KEY_FILE', $ServerKeyFile, 'Process')
    }
    # Only the PUBLIC half, for the status page's attester rung; the node reads
    # the file on each request, so a key created later shows without a restart.
    [Environment]::SetEnvironmentVariable('WRFCOIN_ORACLE_PUBLIC_KEY_FILE', $OraclePublicFile, 'Process')
    if ((Test-OracleKey) -and (Test-Path -LiteralPath $OracleTokenFile)) {
        # Oracle role for the local attester only; locked by Initialize-UserState.
        [Environment]::SetEnvironmentVariable('WRFCOIN_ORACLE_TOKEN', ([System.IO.File]::ReadAllText($OracleTokenFile).Trim()), 'Process')
    }
    if ($mode -eq 'bonded') {
        Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) Start: bonded mode, service id $($reach['WRFCOIN_SERVICE_ID'])"
    }

    if (Test-Path -LiteralPath $LogFile) { Move-Item -LiteralPath $LogFile -Destination "$LogFile.1" -Force }

    # cmd.exe only redirects the node's output to the log file. CreateNoWindow
    # gives the node a console that is never shown, so Stop can still deliver
    # Ctrl+C to it. No new process group: that would disable Ctrl+C.
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $psi.Arguments = "/d /c `"`"$Exe`" >> `"$LogFile`" 2>&1`""
    $psi.WorkingDirectory = $Root   # relative paths land in the per-user folder
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $null = [System.Diagnostics.Process]::Start($psi)

    # Wait for the API before opening the page. A node that dies at startup is
    # reported with the end of its log instead of a browser error.
    $deadline = (Get-Date).AddSeconds(90)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        if (Test-Url "$api/health") { Open-StatusPage $api; return }
        if (@(Get-NodeProcess).Count -eq 0) {
            Start-Sleep -Seconds 1
            $tail = if (Test-Path -LiteralPath $LogFile) { (Get-Content -LiteralPath $LogFile -Tail 15) -join "`n" } else { '' }
            Fail "the node stopped during startup. Last lines of ${LogFile}:`n`n$tail"
        }
    }
    # Still starting (slow disk, first run): open the page anyway; it refreshes.
    Open-StatusPage $api
}

$ConsoleCtrl = @'
using System;
using System.Runtime.InteropServices;
public static class WrfcoinConsoleCtrl {
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool FreeConsole();
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AttachConsole(uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint group);
    // Ctrl+C to every process on pid's console (the node and its cmd.exe
    // wrapper) while this process ignores it. Returns a Win32 error; 0 = sent.
    public static int SendCtrlC(uint pid) {
        FreeConsole();
        if (!AttachConsole(pid)) return Marshal.GetLastWin32Error();
        SetConsoleCtrlHandler(IntPtr.Zero, true);
        int err = GenerateConsoleCtrlEvent(0, 0) ? 0 : Marshal.GetLastWin32Error();
        System.Threading.Thread.Sleep(500);
        FreeConsole();
        return err;
    }
}
'@

function Stop-Attester([bool]$Report = $true) {
    # $Report = $false is the node-stop path (Stop, uninstall, upgrade): it must
    # never block, so an attester that does not stop on Ctrl+C within 30 s is
    # logged and force-stopped there instead of failing.
    $procs = @(Get-AttesterProcess)
    if ($procs.Count -eq 0) {
        if ($Report) { Show-Message 'The WRFCoin attester is not running.' }
        return
    }
    if (-not ('WrfcoinConsoleCtrl' -as [type])) { Add-Type -TypeDefinition $ConsoleCtrl }
    foreach ($proc in $procs) {
        $err = [WrfcoinConsoleCtrl]::SendCtrlC([uint32]$proc.Id)
        if ($err -ne 0) {
            if ($Report) { Fail "could not signal the attester (process $($proc.Id), Win32 error $err)" }
            # Node-stop path: no point waiting 30 s for a signal that never arrived.
            try { Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) Stop: Ctrl+C to the attester failed (Win32 error $err); force-stopping it" } catch { }
            $proc | Stop-Process -Force -ErrorAction SilentlyContinue
        }
    }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and @(Get-AttesterProcess).Count -gt 0) { Start-Sleep -Seconds 1 }
    $left = @(Get-AttesterProcess)
    if ($left.Count -gt 0) {
        if ($Report) { Fail "the attester is still running after 30 s; see $AttesterLog" }
        try { Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) Stop: the attester did not stop on Ctrl+C within 30 s; force-stopping it" } catch { }
        $left | Stop-Process -Force -ErrorAction SilentlyContinue
        return
    }
    if ($Report) { Show-Message 'The WRFCoin attester has stopped.' }
}

function Stop-Node {
    # The attester needs the node; stop it first (also lets an upgrade replace its file).
    Stop-Attester $false
    $nodes = @(Get-NodeProcess)
    if ($nodes.Count -eq 0) { Show-Message 'The WRFCoin node is not running.'; return }
    if (-not ('WrfcoinConsoleCtrl' -as [type])) { Add-Type -TypeDefinition $ConsoleCtrl }
    foreach ($node in $nodes) {
        $err = [WrfcoinConsoleCtrl]::SendCtrlC([uint32]$node.Id)
        if ($err -ne 0) { Fail "could not signal the node (process $($node.Id), Win32 error $err)" }
    }
    # Wait for the graceful shutdown (P2P disconnect, checkpoint, HTTP stop).
    $deadline = (Get-Date).AddSeconds($StopWaitSeconds)
    while ((Get-Date) -lt $deadline -and @(Get-NodeProcess).Count -gt 0) { Start-Sleep -Seconds 1 }
    if (@(Get-NodeProcess).Count -gt 0) {
        Fail "the node is still shutting down after $StopWaitSeconds s. Try again in a minute; see $LogFile"
    }
    Show-Message 'The WRFCoin node has stopped.'
}

function Read-BondConfig {
    # Like reachable.conf: per user, comments and blank lines allowed, only
    # $BondKeys, each exactly once. All three are required.
    if (-not (Test-Path -LiteralPath $BondConfig)) {
        Fail "missing $BondConfig. A bonded node needs it to be paid: see windows-node.md, 'Receipts'."
    }
    $pairs = [ordered]@{}
    $n = 0
    foreach ($raw in [System.IO.File]::ReadAllLines($BondConfig)) {
        $n++
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${BondConfig} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq).Trim()
        $value = $line.Substring($eq + 1).Trim()
        if ($BondKeys -cnotcontains $key) { Fail "${BondConfig} line ${n}: $key cannot be set there (only $($BondKeys -join ', '))" }
        if ($pairs.Contains($key)) { Fail "${BondConfig} defines $key twice" }
        $ok = switch ($key) {
            'WRFCOIN_BOND_DEPLOYMENT_ID' { $value -cmatch '^[0-9a-f]{64}$' }
            'WRFCOIN_BOND_OWNER_ADDRESS' { $value -cmatch '^0x[0-9a-f]{40}$' }
            'WRFCOIN_CLAIM_MAILBOX_URL' { $value -cmatch '^https://[A-Za-z0-9.-]+(:[0-9]{1,5})?(/[A-Za-z0-9._~/-]*)?$' }
        }
        if (-not $ok) { Fail "${BondConfig} line ${n}: '$value' is not a valid $key" }
        $pairs[$key] = $value
    }
    foreach ($key in $BondKeys) {
        if (-not $pairs.Contains($key)) { Fail "${BondConfig} is missing $key" }
    }
    return $pairs
}

function Assert-ReceiptReady {
    # Everything a receipt pass needs, checked before a task is registered.
    $cfg = Read-LocalNodeConfig
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'unattended receipts are for testnet only' }
    $bond = Read-BondConfig
    if (-not (Test-Path -LiteralPath $OwnerKeyFile)) { Fail "missing $OwnerKeyFile (the bond owner's key)" }
    Protect-UserOnly $OwnerKeyFile
    if (-not (Test-Path -LiteralPath $CliExe)) { Fail "missing $CliExe - reinstall the package" }
    return @{ Node = $cfg; Bond = $bond }
}

function Write-ReceiptLog([string]$Line) {
    if ((Test-Path -LiteralPath $ReceiptLog) -and (Get-Item -LiteralPath $ReceiptLog).Length -gt $ReceiptLogMaxBytes) {
        Move-Item -LiteralPath $ReceiptLog -Destination "$ReceiptLog.1" -Force
    }
    Add-Content -LiteralPath $ReceiptLog -Value "$(Get-Date -Format o) $Line"
}

function Get-JsonField($Object, [string]$Name) {
    # StrictMode-safe read of one property of a ConvertFrom-Json object ($null if absent).
    if ($null -eq $Object) { return $null }
    $prop = $Object.PSObject.Properties[$Name]
    if ($prop) { return $prop.Value }
    return $null
}

function Invoke-Receipt {
    $ready = Assert-ReceiptReady
    $cfg = $ready.Node; $bond = $ready.Bond
    if (-not (Test-Path -LiteralPath $ReceiptStateDir)) { New-Item -ItemType Directory -Path $ReceiptStateDir | Out-Null }
    Protect-UserOnly $ReceiptStateDir
    $api = Get-NodeApiUrl $cfg
    # The key and token go by file path only; wrfcoin.exe checks both are
    # owner-only and never prints them. The mailbox gets no credential.
    $cliArgs = @('--output', 'json', 'node', 'receipt', 'auto',
        '--state-dir', $ReceiptStateDir,
        '--mailbox-url', $bond['WRFCOIN_CLAIM_MAILBOX_URL'],
        '--deployment-id', $bond['WRFCOIN_BOND_DEPLOYMENT_ID'],
        '--sender', $bond['WRFCOIN_BOND_OWNER_ADDRESS'],
        '--genesis-hash', $cfg['WRFCOIN_GENESIS_HASH'],
        '--chain-id', $cfg['WRFCOIN_CHAIN_ID'],
        '--network-id', $cfg['WRFCOIN_NETWORK_ID'],
        '--api-url', $api,
        '--key-file', $OwnerKeyFile,
        '--operator-token-file', $TokenFile)
    # A native command's stderr must not become a terminating error here; the exit code decides.
    $ErrorActionPreference = 'Continue'
    $out = & $CliExe @cliArgs 2>&1 | Out-String
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    # The CLI's last line is its JSON envelope: keep only status fields.
    $status = ''
    $last = @($out -split "`r?`n" | Where-Object { $_.Trim().StartsWith('{') }) | Select-Object -Last 1
    if ($last) {
        try {
            $j = ConvertFrom-Json $last
            $data = Get-JsonField $j 'data'
            $err = Get-JsonField (Get-JsonField $j 'error') 'message'
            if (Get-JsonField $data 'status') { $status = "status=$(Get-JsonField $data 'status') epoch=$(Get-JsonField $data 'epoch')" }
            if ($err) { $status = "error=$("$err".Substring(0, [Math]::Min(300, "$err".Length)))" }
        } catch { }
    }
    if ($out -match 'under 0\.05 WRF') { $status += ' LOW_OWNER_BALANCE (top up the owner account: each receipt costs a fee)' }
    $meaning = switch ($code) { 0 { 'done' } 2 { 'posted, retry later' } default { 'FAILED' } }
    Write-ReceiptLog "receipt exit=$code ($meaning) $status"
    # 0 = done or nothing to do, 2 = run again later: both are normal for the timer.
    if ($code -ne 0 -and $code -ne 2) { exit 1 }
}

function Register-ReceiptTimer {
    [void](Assert-ReceiptReady)
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $argLine = "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Action Receipt -Quiet"
    $taskAction = New-ScheduledTaskAction -Execute $ps -Argument $argLine -WorkingDirectory $AppDir
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) `
        -RepetitionInterval (New-TimeSpan -Minutes $ReceiptIntervalMinutes)
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 20)
    $me = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $principal = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskPath $ReceiptTaskPath -TaskName $ReceiptTaskName -Action $taskAction -Trigger $trigger `
        -Settings $settings -Principal $principal -Description 'WRFCoin bonded node: sign and submit this epoch''s receipt' -Force | Out-Null
    Show-Message "Receipts are on: every $ReceiptIntervalMinutes minutes while you are signed in. Log: $ReceiptLog"
}

function Unregister-ReceiptTimer {
    $task = Get-ScheduledTask -TaskPath $ReceiptTaskPath -TaskName $ReceiptTaskName -ErrorAction SilentlyContinue
    if (-not $task) { Show-Message 'The WRFCoin receipt timer is not set up.'; return }
    Unregister-ScheduledTask -TaskPath $ReceiptTaskPath -TaskName $ReceiptTaskName -Confirm:$false
    Show-Message 'The WRFCoin receipt timer is removed.'
}

function Read-AddressConfig {
    # Like bond.conf: per user, comments and blank lines allowed, only
    # $AddressKeys, each exactly once. Both are required.
    if (-not (Test-Path -LiteralPath $AddressConfig)) {
        Fail "missing $AddressConfig. Publishing a bonded node's address needs it: see windows-node.md, 'Address record'."
    }
    $pairs = [ordered]@{}
    $n = 0
    foreach ($raw in [System.IO.File]::ReadAllLines($AddressConfig)) {
        $n++
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${AddressConfig} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq).Trim()
        $value = $line.Substring($eq + 1).Trim()
        if ($AddressKeys -cnotcontains $key) { Fail "${AddressConfig} line ${n}: $key cannot be set there (only $($AddressKeys -join ', '))" }
        if ($pairs.Contains($key)) { Fail "${AddressConfig} defines $key twice" }
        $ok = switch ($key) {
            'WRFCOIN_BOND_DEPLOYMENT_ID' { $value -cmatch '^[0-9a-f]{64}$' }
            'WRFCOIN_ADDRESS_MAILBOX_URL' { $value -cmatch '^https://[A-Za-z0-9.-]+(:[0-9]{1,5})?(/[A-Za-z0-9._~/-]*)?$' }
        }
        if (-not $ok) { Fail "${AddressConfig} line ${n}: '$value' is not a valid $key" }
        $pairs[$key] = $value
    }
    foreach ($key in $AddressKeys) {
        if (-not $pairs.Contains($key)) { Fail "${AddressConfig} is missing $key" }
    }
    return $pairs
}

function Assert-AddressReady {
    # Everything a publish pass needs, checked before a task is registered.
    $reach = Read-ReachableConfig
    $cfg = Read-LocalNodeConfig $reach
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'unattended address records are for testnet only' }
    if ((Resolve-NodeMode $reach) -ne 'bonded') { Fail "the address record is for a bonded node: set up bonded mode in $ReachableConfig first" }
    if ([string]$reach['WRFCOIN_P2P_EXTERNAL_ADDR'] -cnotmatch '^/ip4/') {
        Fail "${ReachableConfig}: the address record needs WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<your public address>/tcp/<port> (a /dns4/ name is not accepted there)"
    }
    $addr = Read-AddressConfig
    Protect-UserOnly $ServerKeyFile
    if (-not (Test-Path -LiteralPath $CliExe)) { Fail "missing $CliExe - reinstall the package" }
    return @{ Node = $cfg; Address = $addr }
}

function Write-AddressLog([string]$Line) {
    # One line per change: a pass whose outcome matches the last line writes
    # nothing, so an hourly "not open yet" never piles up.
    if (Test-Path -LiteralPath $AddressLog) {
        $last = Get-Content -LiteralPath $AddressLog -Tail 1 -ErrorAction SilentlyContinue
        if ($last -and (($last -replace '^\S+ ', '') -ceq $Line)) { return }
        if ((Get-Item -LiteralPath $AddressLog).Length -gt $AddressLogMaxBytes) {
            Move-Item -LiteralPath $AddressLog -Destination "$AddressLog.1" -Force
        }
    }
    Add-Content -LiteralPath $AddressLog -Value "$(Get-Date -Format o) $Line"
}

function Hide-Addresses([string]$Text) {
    # The log never holds this node's public address.
    $Text = $Text -replace '/(ip4|ip6|dns4)/[^/\s]+', '/$1/<addr>'
    return ($Text -replace '\b\d{1,3}(\.\d{1,3}){3}\b', '<addr>')
}

function Invoke-AddressPublish {
    $ready = Assert-AddressReady
    $cfg = $ready.Node; $addr = $ready.Address
    $api = Get-NodeApiUrl $cfg
    # The address the node itself announces, ending in /p2p/<its peer id>.
    $announced = ''
    try {
        $peer = Invoke-RestMethod -Uri "$api/network/peer_id" -TimeoutSec 10
        $announced = [string](Get-JsonField $peer 'external_addr')
    } catch { }
    if (-not $announced) {
        Write-AddressLog 'address skipped (node not running, retry later)'
        return
    }
    if ($announced -cnotmatch '^/ip4/[0-9.]+/tcp/[0-9]{1,5}/p2p/[1-9A-HJ-NP-Za-km-z]{40,60}$') {
        Write-AddressLog 'address skipped (the node announces no /ip4/ address with a peer id; restart it after editing reachable.conf)'
        exit 1
    }
    if (-not (Test-Path -LiteralPath $AddressStateDir)) { New-Item -ItemType Directory -Path $AddressStateDir | Out-Null }
    Protect-UserOnly $AddressStateDir
    # The keys and token go by file path only; wrfcoin.exe checks they are
    # owner-only and never prints them. The mailbox gets no credential.
    $cliArgs = @('--output', 'json', 'node', 'address', 'publish',
        '--deployment-id', $addr['WRFCOIN_BOND_DEPLOYMENT_ID'],
        '--multiaddr', $announced,
        '--mailbox-url', $addr['WRFCOIN_ADDRESS_MAILBOX_URL'],
        '--api-url', $api,
        '--chain-id', $cfg['WRFCOIN_CHAIN_ID'],
        '--network-id', $cfg['WRFCOIN_NETWORK_ID'],
        '--genesis-hash', $cfg['WRFCOIN_GENESIS_HASH'],
        '--state-dir', $AddressStateDir,
        '--server-key-file', $ServerKeyFile,
        '--p2p-key-file', (Join-Path $DataDir 'peer_identity.key'),
        '--operator-token-file', $TokenFile)
    # A native command's stderr must not become a terminating error here; the exit code decides.
    $ErrorActionPreference = 'Continue'
    $out = & $CliExe @cliArgs 2>&1 | Out-String
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    $status = ''
    $last = @($out -split "`r?`n" | Where-Object { $_.Trim().StartsWith('{') }) | Select-Object -Last 1
    if ($last) {
        try {
            $j = ConvertFrom-Json $last
            $data = Get-JsonField $j 'data'
            $err = Get-JsonField (Get-JsonField $j 'error') 'message'
            if (Get-JsonField $data 'status') { $status = "status=$(Get-JsonField $data 'status') epoch=$(Get-JsonField (Get-JsonField $data 'summary') 'epoch')" }
            if ($err -and $code -ne 2) { $status = "error=$(Hide-Addresses ("$err".Substring(0, [Math]::Min(300, "$err".Length))))" }
        } catch { }
    }
    # The mailbox route is switched off server-side until it opens
    # (404 ADDRESSES_DISABLED, or no route at all): wait quietly, no error.
    $meaning = switch ($code) {
        0 { 'done' }
        2 { if ($out -match 'ADDRESSES_DISABLED|RECEIPT_HTTP_404') { 'mailbox not open yet, retry later' } else { 'retry later' } }
        default { 'FAILED' }
    }
    Write-AddressLog ("address exit=$code ($meaning) $status".TrimEnd())
    # 0 = stored or unchanged, 2 = run again later: both are normal for the timer.
    if ($code -ne 0 -and $code -ne 2) { exit 1 }
}

function Register-AddressTimer {
    [void](Assert-AddressReady)
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $argLine = "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Action PublishAddress -Quiet"
    $taskAction = New-ScheduledTaskAction -Execute $ps -Argument $argLine -WorkingDirectory $AppDir
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) `
        -RepetitionInterval (New-TimeSpan -Minutes $AddressIntervalMinutes)
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
    $me = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $principal = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskPath $AddressTaskPath -TaskName $AddressTaskName -Action $taskAction -Trigger $trigger `
        -Settings $settings -Principal $principal -Description 'WRFCoin bonded node: publish this epoch''s signed address record' -Force | Out-Null
    Show-Message "Address publishing is on: every $AddressIntervalMinutes minutes while you are signed in. Log: $AddressLog"
}

function Unregister-AddressTimer {
    $task = Get-ScheduledTask -TaskPath $AddressTaskPath -TaskName $AddressTaskName -ErrorAction SilentlyContinue
    if (-not $task) { Show-Message 'The WRFCoin address timer is not set up.'; return }
    Unregister-ScheduledTask -TaskPath $AddressTaskPath -TaskName $AddressTaskName -Confirm:$false
    Show-Message 'The WRFCoin address timer is removed.'
}

function Write-AttesterAutoLog([string]$Line) {
    # One line per change: a pass whose outcome matches the last line writes
    # nothing, so a five-minute "waiting for your seat" never piles up.
    if (Test-Path -LiteralPath $AttesterAutoLog) {
        $last = Get-Content -LiteralPath $AttesterAutoLog -Tail 1 -ErrorAction SilentlyContinue
        if ($last -and (($last -replace '^\S+ ', '') -ceq $Line)) { return }
        if ((Get-Item -LiteralPath $AttesterAutoLog).Length -gt $AttesterAutoLogMaxBytes) {
            Move-Item -LiteralPath $AttesterAutoLog -Destination "$AttesterAutoLog.1" -Force
        }
    }
    Add-Content -LiteralPath $AttesterAutoLog -Value "$(Get-Date -Format o) $Line"
}

function Get-AttesterOracleId {
    # The PUBLIC id only: oracle-public.txt, else derived from the key by wrfcoin.exe.
    if (Test-Path -LiteralPath $OraclePublicFile) {
        $pub = ([System.IO.File]::ReadAllText($OraclePublicFile)).Trim().ToLower()
        if ($pub -cmatch '^[0-9a-f]{64}$') { return $pub }
    }
    return (Get-OraclePublicKey $OracleKeyFile)
}

function Test-AttesterSeatDepth([string]$OracleId, [bool]$Seated, [long]$Height) {
    # Pure bookkeeping for the seat gate. Returns the number of blocks the seat
    # has been seen for, or -1 while not seated. The first height a pass sees
    # the id seated is kept; the real seating height is never later than it,
    # so "Height - first seen >= 3" means the set change is at least 3 deep.
    if (-not $Seated) {
        if (Test-Path -LiteralPath $AttesterSeatFile) { Remove-Item -LiteralPath $AttesterSeatFile -Force }
        return -1
    }
    $first = $null
    if (Test-Path -LiteralPath $AttesterSeatFile) {
        try {
            $seat = ConvertFrom-Json ([System.IO.File]::ReadAllText($AttesterSeatFile))
            $seen = Get-JsonField $seat 'first_seen_height'
            # A file without a height (or for another key) starts the count again.
            if ("$(Get-JsonField $seat 'oracle_id')" -ceq $OracleId -and "$seen" -cmatch '^[0-9]+$') { $first = [long]$seen }
        } catch { $first = $null }
    }
    if ($null -eq $first -or $first -gt $Height) {
        $first = $Height
        [System.IO.File]::WriteAllText($AttesterSeatFile, (ConvertTo-Json @{ oracle_id = $OracleId; first_seen_height = $Height } -Compress))
    }
    return ($Height - $first)
}

function Invoke-AttesterAutoStart {
    if (Test-Path -LiteralPath $AttesterOptOutFile) { Write-AttesterAutoLog 'auto-start is off (attester-autostart.off)'; return }
    if (-not (Test-OracleKey)) { Write-AttesterAutoLog 'no attester key yet'; return }
    $script:FailThrows = $true
    try {
        $cfg = Read-LocalNodeConfig
    } catch {
        Write-AttesterAutoLog "not started: $($_.Exception.Message)"
        exit 1
    } finally {
        $script:FailThrows = $false
    }
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Write-AttesterAutoLog 'not started: the software oracle key is for testnet only'; exit 1 }
    if (@(Get-NodeProcess).Count -eq 0) { Write-AttesterAutoLog 'waiting (node not running)'; return }
    $api = Get-NodeApiUrl $cfg
    $script:FailThrows = $true
    try {
        $id = Get-AttesterOracleId
    } catch {
        Write-AttesterAutoLog "not started: $($_.Exception.Message)"
        exit 1
    } finally {
        $script:FailThrows = $false
    }
    try {
        # The operator token reads the seated set (a Reader route); it is sent
        # only to this node and never logged.
        $token = [System.IO.File]::ReadAllText($TokenFile).Trim()
        $set = Invoke-RestMethod -Uri "$api/transactions/oracle/config" -TimeoutSec 10 -Headers @{ Authorization = "Bearer $token" }
        $data = Get-JsonField $set 'data'
        if ($null -ne $data) { $set = $data }
        $members = @(Get-JsonField $set 'oracle_set' | ForEach-Object { "$_".ToLower() })
        $info = Invoke-RestMethod -Uri "$api/network/info" -TimeoutSec 10
        $height = [long](Get-JsonField $info 'chain_height')
    } catch {
        Write-AttesterAutoLog 'waiting (the node did not answer; retry later)'
        return
    }
    if ($height -le 0) { Write-AttesterAutoLog 'waiting (the node reported no height; retry later)'; return }
    $short = $id.Substring(0, 8)
    $depth = Test-AttesterSeatDepth $id ($members -ccontains $id) $height
    if ($depth -lt 0) {
        # Never stops an attester: one started by hand keeps running until the
        # operator stops it. Leaving the set only resets the depth count.
        Write-AttesterAutoLog "waiting for your seat (oracle $short is not in the oracle set)"
        return
    }
    if (@(Get-AttesterProcess).Count -gt 0) { Write-AttesterAutoLog "seated (oracle $short): attester running"; return }
    if ($depth -lt $AttesterSeatDepthBlocks) {
        Write-AttesterAutoLog "seated (oracle $short): waiting until the seat is $AttesterSeatDepthBlocks blocks deep"
        return
    }
    $script:FailThrows = $true
    try {
        Start-Attester
    } catch {
        $msg = "$($_.Exception.Message)" -replace '\s+', ' '
        Write-AttesterAutoLog "not started: $($msg.Substring(0, [Math]::Min(300, $msg.Length)))"
        exit 1
    } finally {
        $script:FailThrows = $false
    }
    Write-AttesterAutoLog "seated (oracle $short): attester started"
}

function Register-AttesterTimer([bool]$Automatic = $false) {
    if (-not (Test-OracleKey)) { Fail "no attester key yet: use 'Create attester key' first" }
    $cfg = Read-NodeConfig $Config
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'the software oracle key is for testnet only' }
    if (-not $Automatic -and (Test-Path -LiteralPath $AttesterOptOutFile)) { Remove-Item -LiteralPath $AttesterOptOutFile -Force }
    $ps = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $argLine = "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Action AttesterAutoStart -Quiet"
    $taskAction = New-ScheduledTaskAction -Execute $ps -Argument $argLine -WorkingDirectory $AppDir
    $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) `
        -RepetitionInterval (New-TimeSpan -Minutes $AttesterIntervalMinutes)
    $settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 4)
    $me = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
    $principal = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -Action $taskAction -Trigger $trigger `
        -Settings $settings -Principal $principal -Description 'WRFCoin attester: start the attester once this node''s oracle key is seated' -Force | Out-Null
    if (-not $Automatic) {
        Show-Message "Attester auto-start is on: checked every $AttesterIntervalMinutes minutes while you are signed in; the attester starts once your seat is $AttesterSeatDepthBlocks blocks deep. Log: $AttesterAutoLog"
    }
}

function Enable-AttesterAutoStart {
    # Called by node Start and 'Create Attester Key': registers the task once a
    # key exists, unless the operator turned auto-start off. Never fails the
    # caller; a problem is one launcher.log line.
    try {
        if ((Test-Path -LiteralPath $AttesterOptOutFile) -or -not (Test-OracleKey)) { return }
        if (Get-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -ErrorAction SilentlyContinue) { return }
        $script:FailThrows = $true
        Register-AttesterTimer $true
        Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) attester auto-start task registered"
    } catch {
        try { Add-Content -LiteralPath $LauncherLog -Value "$(Get-Date -Format o) attester auto-start not registered: $($_.Exception.Message)" } catch { }
    } finally {
        $script:FailThrows = $false
    }
}

function Disable-AttesterAutoStart {
    [System.IO.File]::WriteAllText($AttesterOptOutFile, "Attester auto-start was turned off. Remove this file or run -Action AttesterTimer to turn it on.`r`n")
    $task = Get-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -ErrorAction SilentlyContinue
    if ($task) { Unregister-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -Confirm:$false }
    Show-Message 'Attester auto-start is off. A running attester keeps running until you stop it.'
}

function Unregister-AttesterTimer {
    $task = Get-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -ErrorAction SilentlyContinue
    if (-not $task) { Show-Message 'The WRFCoin attester auto-start task is not set up.'; return }
    Unregister-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -Confirm:$false
    Show-Message 'The WRFCoin attester auto-start task is removed.'
}

try {
    if (-not (Test-Path -LiteralPath $Config)) { Fail "missing $Config - reinstall the package" }
    switch ($Action) {
        'Setup' { Initialize-UserState }
        'ServerKey' { Initialize-UserState; New-ServerKey }
        'NewOracleKey' { Initialize-UserState; New-OracleKey; Enable-AttesterAutoStart }
        'StartAttester' { Initialize-UserState; Start-Attester }
        'StopAttester' { Stop-Attester }
        'Start' { Initialize-UserState; Start-Node; Enable-AttesterAutoStart }
        'Stop'  { Stop-Node }
        'Status' { Show-StatusPage }
        'ReceiptTimer' { Initialize-UserState; Register-ReceiptTimer }
        'Receipt' { Initialize-UserState; Invoke-Receipt }
        'StopReceiptTimer' { Unregister-ReceiptTimer }
        'AddressTimer' { Initialize-UserState; Register-AddressTimer }
        'PublishAddress' { Initialize-UserState; Invoke-AddressPublish }
        'StopAddressTimer' { Unregister-AddressTimer }
        'AttesterTimer' { Initialize-UserState; Register-AttesterTimer }
        'AttesterAutoStart' { Initialize-UserState; Invoke-AttesterAutoStart }
        'AttesterAutoStartOff' { Initialize-UserState; Disable-AttesterAutoStart }
        'StopAttesterTimer' { Unregister-AttesterTimer }
    }
} catch {
    Fail "$($_.Exception.Message) ($($_.InvocationInfo.PositionMessage))"
}
exit 0
