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

  -NoBrowser  start without opening the browser (sign-in start, CI).
  -Quiet      never show a message box (installer, CI).

  Per-user state lives in %LOCALAPPDATA%\WRFCoin:
    data\              chain database and peer_identity.key (WRFCOIN_DATA_DIR)
    logs\node.log      node output (previous run: node.log.1)
    logs\launcher.log  this script's own errors
    operator.token     the node's local API operator token
  The folder is restricted to the current user (inheritance removed, one
  full-control entry for the user's SID).
#>
[CmdletBinding()]
param(
    [ValidateSet('Start', 'Stop', 'Setup')]
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
        $bytes = New-Object byte[] 32
        $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
        $token = -join ($bytes | ForEach-Object { $_.ToString('x2') })
        [System.IO.File]::WriteAllText($TokenFile, $token)
    }
    Protect-UserOnly $TokenFile
    $identity = Join-Path $DataDir 'peer_identity.key'
    if (Test-Path -LiteralPath $identity) { Protect-UserOnly $identity }
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

    foreach ($key in $cfg.Keys) { [Environment]::SetEnvironmentVariable($key, $cfg[$key], 'Process') }
    [Environment]::SetEnvironmentVariable('WRFCOIN_DATA_DIR', $DataDir, 'Process')
    [Environment]::SetEnvironmentVariable('WRFCOIN_OPERATOR_TOKEN', ([System.IO.File]::ReadAllText($TokenFile).Trim()), 'Process')

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

function Stop-Node {
    $nodes = @(Get-NodeProcess)
    if ($nodes.Count -eq 0) { Show-Message 'The WRFCoin node is not running.'; return }
    Add-Type -TypeDefinition $ConsoleCtrl
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
        'Start' { Initialize-UserState; Start-Node }
        'Stop'  { Stop-Node }
    }
} catch {
    Fail "$($_.Exception.Message) ($($_.InvocationInfo.PositionMessage))"
}
exit 0
