<#
.SYNOPSIS
  ComTap installer and updater for Windows.
.DESCRIPTION
  Downloads ComTap from GitHub, verifies its checksum and installs it to
  C:\Program Files\ComTap.
  - New PC: opens ComTap's setup screen, which sets up the com0com driver, the
    virtual COM ports and the Windows service.
  - ComTap already set up: stops the service, swaps the program, then runs
    `comtap install`, which keeps your devices and settings and starts the
    service again.
  ComTap installs a driver and a service, so this needs administrator rights:
  run without them, it asks Windows (UAC). The in-app updater re-runs this
  script.
.EXAMPLE
  irm https://brumley.info/comtap/install.ps1 | iex
#>

param(
  [string]$Version = "",
  [switch]$InsecureSkipVerify,
  [switch]$NoRelaunch,
  # Set when this script restarts itself as administrator.
  [switch]$Elevated
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # Invoke-WebRequest is far slower with the progress bar.

$Repo = "jackbrumley/comtap"
$Site = "https://brumley.info/comtap"
$InstallDir = Join-Path $env:ProgramFiles "ComTap"
$Exe = Join-Path $InstallDir "comtap.exe"
$TempDir = Join-Path $env:TEMP "comtap-install"

$GitHubApiHeaders = @{ "Accept" = "application/vnd.github+json" }
if ($env:COMTAP_GITHUB_TOKEN) {
  $GitHubApiHeaders["Authorization"] = "Bearer $env:COMTAP_GITHUB_TOKEN"
}

$IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $IsAdmin) {
  Write-Host "[comtap] ComTap installs a driver and a Windows service, so it needs administrator rights."
  Write-Host "[comtap] Asking Windows for permission (UAC)..."
  # Piped through `irm | iex` there is no script file to restart, so fetch one.
  $Self = $PSCommandPath
  if (-not $Self) {
    $Self = Join-Path $env:TEMP "comtap-install.ps1"
    Invoke-RestMethod -Uri "$Site/install.ps1" -OutFile $Self
  }
  $ArgList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$Self`"", "-Elevated")
  if ($Version) { $ArgList += @("-Version", $Version) }
  if ($InsecureSkipVerify) { $ArgList += "-InsecureSkipVerify" }
  if ($NoRelaunch) { $ArgList += "-NoRelaunch" }
  try {
    # Not -Wait: it also waits for everything the child starts, so this window
    # would stay open until the ComTap screen the installer opens is closed.
    # Reading .Handle first keeps the exit code readable after the wait.
    $Child = Start-Process -FilePath "powershell.exe" -ArgumentList $ArgList -Verb RunAs -PassThru
    $null = $Child.Handle
    $Child.WaitForExit()
  } catch {
    Write-Host "[comtap] ERROR: Windows did not give administrator rights, so nothing was installed." -ForegroundColor Red
    exit 1
  }
  if ($Child.ExitCode -ne 0) { Write-Host "[comtap] The install failed. See the log in $env:ProgramData\ComTap\logs\update.log" -ForegroundColor Red }
  exit $Child.ExitCode
}

# ComTap keeps its files in C:\ProgramData\ComTap (its service runs as SYSTEM).
$LogFile = $null
try {
  $LogDir = Join-Path $env:ProgramData "ComTap\logs"
  New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
  $LogFile = Join-Path $LogDir "update.log"
} catch {}

function Log($Message) {
  Write-Host "[comtap] $Message"
  if ($LogFile) { Add-Content -Path $LogFile -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] [comtap] $Message" -ErrorAction SilentlyContinue }
}

function Fail($Message) {
  Write-Host "[comtap] ERROR: $Message" -ForegroundColor Red
  if ($LogFile) { Add-Content -Path $LogFile -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] [comtap] ERROR: $Message" -ErrorAction SilentlyContinue }
  # A window opened just for the elevated install would close before it could be read.
  if ($Elevated) { Read-Host "Press Enter to close" | Out-Null }
  exit 1
}

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
  Fail "Only 64-bit x64 Windows is supported (this PC is $env:PROCESSOR_ARCHITECTURE)."
}

if ([string]::IsNullOrEmpty($Version)) {
  try {
    $ReleaseTag = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $GitHubApiHeaders).tag_name
  } catch {
    Fail "Could not get the latest release from GitHub (you may be rate limited; try again in a few minutes): $_"
  }
} else {
  $ReleaseTag = $Version
}
$VersionNumber = $ReleaseTag.TrimStart('v')
$AssetName = "comtap-$VersionNumber-windows-x64.exe"

Log "Release: $ReleaseTag"
Log "Package: $AssetName"

try {
  $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$ReleaseTag" -Headers $GitHubApiHeaders
} catch {
  Fail "Could not read release $ReleaseTag from GitHub: $_"
}
$Asset = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
if (-not $Asset) { Fail "$AssetName was not found in release $ReleaseTag." }

if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$Download = Join-Path $TempDir $AssetName

Log "Downloading $($Asset.browser_download_url)"
try {
  Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $Download -UseBasicParsing
} catch {
  Fail "Download failed: $_"
}

if ($InsecureSkipVerify) {
  Log "WARNING: checksum verification disabled"
} else {
  $Digest = [string]$Asset.digest
  if (-not $Digest.StartsWith("sha256:")) {
    Fail "GitHub did not provide a checksum for $AssetName. Re-run with -InsecureSkipVerify to bypass verification."
  }
  $Expected = $Digest.Substring(7).ToLower()
  $Actual = (Get-FileHash -Path $Download -Algorithm SHA256).Hash.ToLower()
  if ($Expected -ne $Actual) { Fail "Checksum mismatch (expected $Expected, got $Actual)" }
  Log "Checksum verified"
}

$Service = Get-Service -Name "ComTap" -ErrorAction SilentlyContinue
if ($Service) {
  Log "Stopping the ComTap service"
  try { Stop-Service -Name "ComTap" -Force } catch { Fail "Could not stop the ComTap service: $_" }
}
if (Get-Process -Name "comtap" -ErrorAction SilentlyContinue) {
  Log "Closing open ComTap windows"
  Stop-Process -Name "comtap" -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
}

Log "Installing to $Exe"
try {
  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  Copy-Item -Path $Download -Destination $Exe -Force
} catch {
  Fail "Could not copy ComTap to ${InstallDir}: $_"
}
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue

if ($Service) {
  # Same steps as the setup screen's Install: unpack the driver files from the
  # new exe, repair the ports, update and start the service. Settings stay.
  Log "Updating the installed setup (devices and settings stay as they are)"
  & $Exe install
  if ($LASTEXITCODE -ne 0) { Fail "comtap install failed (exit $LASTEXITCODE). The ComTap screen and its log folder say why." }
  Log "Update complete: ComTap $VersionNumber is running."
} else {
  Log "ComTap $VersionNumber is copied. Its setup screen sets up your first device and installs the service."
}

if (-not $NoRelaunch -and $env:COMTAP_NO_RELAUNCH -ne "1") {
  Log "Opening ComTap"
  Start-Process -FilePath $Exe
}
if ($Elevated) { Start-Sleep -Seconds 3 }
