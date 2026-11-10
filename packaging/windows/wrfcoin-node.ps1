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
      in the data folder and on the Desktop as wrfcoin-oracle-public.txt. Once
      oracle.key exists, every node Start passes oracle.token to the node as
      WRFCOIN_ORACLE_TOKEN; restart a running node to pick it up.
  -Action StartAttester
      Starts oracle_attester.exe in the background against the local node,
      with chain-888.conf plus attester-888.conf (installed next to this
      script), oracle.key as WRFCOIN_TESTNET_ORACLE_KEY_FILE and oracle.token as
      its API token. The node must be running. Output goes to logs\attester.log.
      A value in attester-888.conf that still reads REPLACE_ME... stops the
      start. If server.key exists, its public key is added to the attester's
      refused snapshot keys, so the oracle key can never be the server key.
  -Action StopAttester
      Sends the attester Ctrl+C on its hidden console and waits for it to exit.
      -Action Stop stops the attester too, before the node.

  -NoBrowser  start without opening the browser (sign-in start, CI).

  Opening a port (optional): %LOCALAPPDATA%\WRFCoin\reachable.conf may set
  ONLY these keys, one KEY=VALUE per line (blank lines and # comments allowed):
    WRFCOIN_P2P_HOST=0.0.0.0                        an IPv4 address to listen on
    WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<ip>/tcp/<port>  the address other nodes dial
                              (or /dns4/<name>/tcp/<port>)
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
    reachable.conf     optional: open a P2P port (see above)
  The folder is restricted to the current user (inheritance removed, one
  full-control entry for the user's SID).
#>
[CmdletBinding()]
param(
    [ValidateSet('Start', 'Stop', 'Setup', 'ServerKey', 'NewOracleKey', 'StartAttester', 'StopAttester')]
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
$OracleKeyFile = Join-Path $Root 'oracle.key'
$OracleTokenFile = Join-Path $Root 'oracle.token'
$OraclePublicFile = Join-Path $Root 'oracle-public.txt'
$AttesterLog = Join-Path $LogDir 'attester.log'
$AttesterStartSeconds = 15
# The only chain-888.conf keys a user may override (case-sensitive). The last
# two switch on bonded mode and are checked together by Resolve-NodeMode.
$ReachableKeys = @('WRFCOIN_P2P_HOST', 'WRFCOIN_P2P_EXTERNAL_ADDR', 'WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS', 'WRFCOIN_SERVICE_ID')
$StopWaitSeconds = 120

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
    if (Test-Path -LiteralPath $OracleKeyFile) {
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
    if (Test-Path -LiteralPath $OracleKeyFile) {
        Protect-UserOnly $OracleKeyFile
        $public = Get-OraclePublicKey $OracleKeyFile
        Write-OraclePublic $public
        Show-Message "The attester key already exists: $OracleKeyFile (not replaced). Oracle public key: $public"
        return
    }
    # Create the file empty, lock it, then write the seed: it is never readable
    # by anyone else, not even for a moment. The seed is never displayed.
    [System.IO.File]::WriteAllText($OracleKeyFile, '')
    Protect-UserOnly $OracleKeyFile
    [System.IO.File]::WriteAllText($OracleKeyFile, (New-RandomHex))
    Initialize-UserState   # creates and locks oracle.token now that the key exists
    $public = Get-OraclePublicKey $OracleKeyFile
    Write-OraclePublic $public
    Show-Message "Created the attester key: $OracleKeyFile. Oracle public key: $public (also in $OraclePublicFile). Restart the node so it picks up the attester token."
}

function Read-AttesterConfig([string]$Path) {
    # Flat KEY=VALUE lines like chain-888.conf; only WRFCOIN_ATTESTER_* keys.
    # A REPLACE_ME... value is a setting this package could not fill in from
    # public configuration: the attester does not start until it is set.
    if (-not (Test-Path -LiteralPath $Path)) { Fail "missing $Path - reinstall the package" }
    $pairs = [ordered]@{}
    $unset = @()
    $n = 0
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $n++
        if ($line -eq '') { continue }
        $eq = $line.IndexOf('=')
        if ($eq -lt 1) { Fail "${Path} line ${n} is not KEY=VALUE" }
        $key = $line.Substring(0, $eq)
        if ($key -cnotmatch '^WRFCOIN_ATTESTER_[A-Z0-9_]+$') { Fail "${Path} line ${n}: only WRFCOIN_ATTESTER_* keys may be set there" }
        if ($pairs.Contains($key)) { Fail "${Path} defines $key twice" }
        $value = $line.Substring($eq + 1)
        if ($value.StartsWith('REPLACE_ME')) { $unset += $key }
        $pairs[$key] = $value
    }
    if ($unset.Count -gt 0) { Fail "${Path} still has placeholder values for: $($unset -join ', ')" }
    foreach ($key in 'WRFCOIN_ATTESTER_FORBID_GOVERNANCE_PUBKEY', 'WRFCOIN_ATTESTER_FORBID_PRODUCER_PUBKEY', 'WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY') {
        if (-not $pairs.Contains($key)) { Fail "${Path} is missing $key" }
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
    $cfg = Read-NodeConfig $Config
    if ($cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet') { Fail 'the software oracle key is for testnet only' }
    $api = "http://$($cfg['WRFCOIN_API_HOST']):$($cfg['WRFCOIN_API_PORT'])"
    if (@(Get-AttesterProcess).Count -gt 0) { Show-Message 'The WRFCoin attester is already running.'; return }
    if (-not (Test-Path -LiteralPath $AttesterExe)) { Fail "missing $AttesterExe - reinstall the package" }
    if (-not (Test-Path -LiteralPath $OracleKeyFile)) { Fail "no attester key yet: use 'Create attester key' first" }
    $att = Read-AttesterConfig $AttesterConfig
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
        Fail "${ReachableConfig}: bonded mode needs WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/<your public address>/tcp/9000 (see Open your port)"
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

function Start-Node {
    $cfg = Read-NodeConfig $Config
    $api = "http://$($cfg['WRFCOIN_API_HOST']):$($cfg['WRFCOIN_API_PORT'])"
    if (@(Get-NodeProcess).Count -gt 0) { Open-StatusPage $api; return }
    if (-not (Test-Path -LiteralPath $Exe)) { Fail "missing $Exe - reinstall the package" }

    $reach = Read-ReachableConfig
    $mode = Resolve-NodeMode $reach
    foreach ($key in $cfg.Keys) { [Environment]::SetEnvironmentVariable($key, $cfg[$key], 'Process') }
    foreach ($key in $reach.Keys) { [Environment]::SetEnvironmentVariable($key, $reach[$key], 'Process') }
    [Environment]::SetEnvironmentVariable('WRFCOIN_DATA_DIR', $DataDir, 'Process')
    [Environment]::SetEnvironmentVariable('WRFCOIN_OPERATOR_TOKEN', ([System.IO.File]::ReadAllText($TokenFile).Trim()), 'Process')
    if (Test-Path -LiteralPath $ServerKeyFile) {
        # Locked to this user by Initialize-UserState; the node checks it again.
        [Environment]::SetEnvironmentVariable('WRFCOIN_SNAPSHOT_SERVER_KEY_FILE', $ServerKeyFile, 'Process')
    }
    if ((Test-Path -LiteralPath $OracleKeyFile) -and (Test-Path -LiteralPath $OracleTokenFile)) {
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
    $procs = @(Get-AttesterProcess)
    if ($procs.Count -eq 0) {
        if ($Report) { Show-Message 'The WRFCoin attester is not running.' }
        return
    }
    if (-not ('WrfcoinConsoleCtrl' -as [type])) { Add-Type -TypeDefinition $ConsoleCtrl }
    foreach ($proc in $procs) {
        $err = [WrfcoinConsoleCtrl]::SendCtrlC([uint32]$proc.Id)
        if ($err -ne 0) { Fail "could not signal the attester (process $($proc.Id), Win32 error $err)" }
    }
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and @(Get-AttesterProcess).Count -gt 0) { Start-Sleep -Seconds 1 }
    if (@(Get-AttesterProcess).Count -gt 0) { Fail "the attester is still running after 30 s; see $AttesterLog" }
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

try {
    if (-not (Test-Path -LiteralPath $Config)) { Fail "missing $Config - reinstall the package" }
    switch ($Action) {
        'Setup' { Initialize-UserState }
        'ServerKey' { Initialize-UserState; New-ServerKey }
        'NewOracleKey' { Initialize-UserState; New-OracleKey }
        'StartAttester' { Initialize-UserState; Start-Attester }
        'StopAttester' { Stop-Attester }
        'Start' { Initialize-UserState; Start-Node }
        'Stop'  { Stop-Node }
    }
} catch {
    Fail "$($_.Exception.Message) ($($_.InvocationInfo.PositionMessage))"
}
exit 0
