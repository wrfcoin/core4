<#
.SYNOPSIS
  Starts the WRFCoin sync-only node for chain 888 (the Windows package launcher).

.DESCRIPTION
  launch_api.exe is configured only by environment variables. This script sets
  them for this process from chain-888.conf (installed next to it, generated
  from the infra cut pins), points the node at the per-user data folder, and
  runs the node in this console window. Press Ctrl+C to stop it.

  Per-user state lives in %LOCALAPPDATA%\WRFCoin:
    data\              the chain database and peer_identity.key (WRFCOIN_DATA_DIR)
    operator.token     the node's local API operator token, created on first run
  The folder is restricted to the current user (inheritance removed, one
  full-control entry for the user's SID) before the node is started.

  -Setup prepares the folder and token and exits; the installer runs it once.
#>
[CmdletBinding()]
param(
    [switch]$Setup
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

$AppDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$Exe      = Join-Path $AppDir 'launch_api.exe'
$Config   = Join-Path $AppDir 'chain-888.conf'
$Root     = Join-Path $env:LOCALAPPDATA 'WRFCoin'
$DataDir  = Join-Path $Root 'data'
$TokenFile = Join-Path $Root 'operator.token'

function Fail([string]$Message) {
    Write-Host ''
    Write-Host "WRFCoin node: $Message" -ForegroundColor Red
    if (-not $Setup) { Read-Host 'Press Enter to close' | Out-Null }
    exit 1
}

function Protect-UserOnly([string]$Path) {
    # Current user only: no inherited entries (Users, Administrators, SYSTEM).
    # By SID so it works on non-English Windows and for domain/Microsoft accounts.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $isDir = Test-Path -LiteralPath $Path -PathType Container
    # Already locked (one explicit entry, for this user, inheritance off): skip,
    # so a large data folder is not re-walked on every start.
    $acl = Get-Acl -LiteralPath $Path
    $rules = @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if ($acl.AreAccessRulesProtected -and $rules.Count -eq 1 -and $rules[0].IdentityReference.Value -eq $sid `
            -and -not $rules[0].IsInherited) { return }
    $grant = if ($isDir) { "*${sid}:(OI)(CI)F" } else { "*${sid}:F" }
    # icacls reports errors on stderr; under 'Stop', Windows PowerShell 5.1 would
    # throw on the first stderr line instead of letting us report it.
    $ErrorActionPreference = 'Continue'
    $out = & icacls.exe $Path /inheritance:r /grant:r $grant 2>&1
    if ($LASTEXITCODE -ne 0) { Fail "could not restrict access to ${Path}: $out" }
}

function Initialize-UserState {
    foreach ($dir in @($Root, $DataDir)) {
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

if (-not (Test-Path -LiteralPath $Config)) { Fail "missing $Config - reinstall the package" }
Initialize-UserState
if ($Setup) { exit 0 }
if (-not (Test-Path -LiteralPath $Exe)) { Fail "missing $Exe - reinstall the package" }

$cfg = Read-NodeConfig $Config
foreach ($key in $cfg.Keys) { [Environment]::SetEnvironmentVariable($key, $cfg[$key], 'Process') }
[Environment]::SetEnvironmentVariable('WRFCOIN_DATA_DIR', $DataDir, 'Process')
[Environment]::SetEnvironmentVariable('WRFCOIN_OPERATOR_TOKEN', ([System.IO.File]::ReadAllText($TokenFile).Trim()), 'Process')

$api = "http://$($cfg['WRFCOIN_API_HOST']):$($cfg['WRFCOIN_API_PORT'])"
try { $Host.UI.RawUI.WindowTitle = 'WRFCoin Node (chain 888) - Ctrl+C to stop' } catch { }
Write-Host 'WRFCoin node - chain 888, sync-only (outbound connections only)'
Write-Host "  data:    $DataDir"
Write-Host "  status:  $api/dashboard   (or $api/network/info)"
Write-Host '  stop:    press Ctrl+C in this window and wait for it to exit'
Write-Host ''

# Relative paths the node may use land in the per-user folder, never in the
# install folder.
Set-Location -LiteralPath $Root

# Start-Process shares this console, so Ctrl+C and closing the window reach the
# node directly and it shuts down on its own; PowerShell only waits and never
# kills it mid-checkpoint.
$node = Start-Process -FilePath $Exe -WorkingDirectory $Root -NoNewWindow -PassThru
$null = $node.Handle  # keep a handle so ExitCode is available after exit
$node.WaitForExit()
$code = $node.ExitCode
Write-Host ''
Write-Host "WRFCoin node exited (code $code)."
if ($code -ne 0) { Read-Host 'Press Enter to close' | Out-Null }
exit $code
