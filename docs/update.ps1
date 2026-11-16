# ComTap's in-app updater for Windows.
#
# F1 -> Update and `comtap update` (from 1.1.1 on) download the current copy
# of this script and run it in a new console window, so this file owns the
# whole update. A fix made here reaches every installed version on its next
# update, with no new release. ComTap only starts PowerShell and exits.
#
# Contract with the app; keep it stable, every shipped version relies on it:
#   - Parameters: -BaseUrl (where this script was downloaded from, without a
#     trailing slash) and -Relaunch (the installed comtap.exe, or empty).
#     New parameters must be optional.
#   - The first output line is exactly "[comtap-update] started".
#   - Output goes to the console window and, when it can be written, to
#     C:\ProgramData\ComTap\logs\update.log (install.ps1 writes there too).
#   - The last line is "[comtap-update] finished: updated" (exit 0) or
#     "[comtap-update] finished: failed" (exit 1). On success install.ps1
#     opens the new version; on failure the installed version opens with
#     --update-failed, which shows the user a warning.
#   - COMTAP_NO_RELAUNCH=1 opens nothing afterwards (the release test).
param(
  [Parameter(Mandatory = $true)][string]$BaseUrl,
  [string]$Relaunch = ""
)

Write-Output "[comtap-update] started"

# Best effort: a user who isn't an administrator may not be able to append to
# a log the elevated installer created.
$UpdateLog = Join-Path $env:ProgramData "ComTap\logs\update.log"
function Write-UpdateLog([string]$Message) {
  try {
    New-Item -ItemType Directory -Force -Path (Split-Path $UpdateLog) | Out-Null
    Add-Content -Path $UpdateLog -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] [comtap-update] $Message" -ErrorAction Stop
  } catch {}
}
function Note([string]$Message) {
  Write-Output "[comtap-update] $Message"
  Write-UpdateLog $Message
}

Write-UpdateLog "started"
Note "$(Get-Date -Format o) from $BaseUrl"

# install.ps1 runs from a file rather than through iex, so its `exit 1` on
# failure ends that script, not this one, and sets $LASTEXITCODE. This runs
# in a script block, so read and reset it as $global:.
$installer = Join-Path $env:TEMP 'comtap-update-install.ps1'
try {
  Invoke-RestMethod -Uri "$BaseUrl/install.ps1" -OutFile $installer -ErrorAction Stop
  $global:LASTEXITCODE = 0
  & $installer
  $succeeded = $global:LASTEXITCODE -eq 0
  if (-not $succeeded) { Note "install.ps1 exited with $global:LASTEXITCODE" }
} catch {
  Note "ERROR: $_"
  $succeeded = $false
}
Remove-Item -Path $installer -Force -ErrorAction SilentlyContinue

if ($succeeded) {
  Note "finished: updated"
  exit 0
}

Note "finished: failed"
# ComTap has no single-instance guard, and the screen exited before this
# script started, so the relaunch needs no wait.
if ($Relaunch -and (Test-Path -LiteralPath $Relaunch) -and $env:COMTAP_NO_RELAUNCH -ne "1") {
  Note "Opening the installed version"
  Start-Process -FilePath $Relaunch -ArgumentList '--update-failed'
} elseif (-not $Relaunch -or -not (Test-Path -LiteralPath $Relaunch)) {
  Note "No installed version found to open"
}
exit 1
