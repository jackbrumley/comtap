<#
.SYNOPSIS
  ComTap uninstaller for Windows.
.DESCRIPTION
  Removes the ComTap service, its virtual COM ports, the desktop shortcut and
  C:\Program Files\ComTap. Leaves C:\ProgramData\ComTap (settings, logs,
  traffic) unless -PurgeData or COMTAP_PURGE_DATA=1, and the com0com driver
  unless -RemoveDriver. Needs administrator rights: run without them, it asks
  Windows (UAC).
.EXAMPLE
  irm https://brumley.info/comtap/uninstall.ps1 | iex
#>

param(
  [switch]$PurgeData,
  [switch]$RemoveDriver,
  # Set when this script restarts itself as administrator.
  [switch]$Elevated
)

$Site = "https://brumley.info/comtap"
$InstallDir = Join-Path $env:ProgramFiles "ComTap"
$Exe = Join-Path $InstallDir "comtap.exe"
$DataDir = Join-Path $env:ProgramData "ComTap"

function Log($Message) { Write-Host "[comtap-uninstall] $Message" }

if (-not $PurgeData -and $env:COMTAP_PURGE_DATA -eq "1") { $PurgeData = $true }

$IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $IsAdmin) {
  Log "Removing a driver's ports and a Windows service needs administrator rights. Asking Windows (UAC)..."
  $Self = $PSCommandPath
  if (-not $Self) {
    $Self = Join-Path $env:TEMP "comtap-uninstall.ps1"
    Invoke-RestMethod -Uri "$Site/uninstall.ps1" -OutFile $Self
  }
  $ArgList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$Self`"", "-Elevated")
  if ($PurgeData) { $ArgList += "-PurgeData" }
  if ($RemoveDriver) { $ArgList += "-RemoveDriver" }
  try {
    # Not -Wait: it also waits for everything the child starts, so this window
    # could stay open after the uninstall has finished.
    # Reading .Handle first keeps the exit code readable after the wait.
    $Child = Start-Process -FilePath "powershell.exe" -ArgumentList $ArgList -Verb RunAs -PassThru
    $null = $Child.Handle
    $Child.WaitForExit()
  } catch {
    Log "Windows did not give administrator rights, so nothing was removed."
    exit 1
  }
  exit $Child.ExitCode
}

Log "Preparing ComTap uninstall"
$Failed = $false

if (Test-Path $Exe) {
  # The app removes its own service, virtual ports and shortcut.
  $UninstallArgs = @("uninstall")
  if ($RemoveDriver) { $UninstallArgs += "--remove-driver" }
  & $Exe @UninstallArgs
  if ($LASTEXITCODE -ne 0) { Log "WARNING: comtap uninstall failed (exit $LASTEXITCODE)"; $Failed = $true }
} elseif (Get-Service -Name "ComTap" -ErrorAction SilentlyContinue) {
  Log "comtap.exe is missing; removing the service directly"
  Stop-Service -Name "ComTap" -Force -ErrorAction SilentlyContinue
  sc.exe delete ComTap | Out-Null
} else {
  Log "No installed copy of ComTap was found."
}

if (Get-Process -Name "comtap" -ErrorAction SilentlyContinue) {
  Log "Closing open ComTap windows"
  Stop-Process -Name "comtap" -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
}

if (Test-Path $InstallDir) {
  Log "Removing $InstallDir"
  Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
  if (Test-Path $InstallDir) { Log "WARNING: could not remove $InstallDir"; $Failed = $true }
}

if (Test-Path $DataDir) {
  if ($PurgeData) {
    Log "Removing settings, logs and traffic: $DataDir"
    Remove-Item -Path $DataDir -Recurse -Force -ErrorAction SilentlyContinue
  } else {
    Log "Settings, logs and traffic left at $DataDir"
  }
}

if ($Failed) {
  Log "Uninstall finished with warnings"
  if ($Elevated) { Read-Host "Press Enter to close" | Out-Null }
  exit 1
}
Log "Uninstall complete"
if ($Elevated) { Start-Sleep -Seconds 3 }
