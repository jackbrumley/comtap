# Release install test: with no software on the COM port, ComTap must stay
# connected to the device, report the port as not open, and drop the data
# quietly (no reopen or reconnect loop). On failure, prints the status and log.
param(
  [Parameter(Mandatory)][string]$Port,
  [int]$Seconds = 8
)

Start-Sleep -Seconds $Seconds
$data = Join-Path $env:ProgramData "ComTap"
$status = Get-Content (Join-Path $data "status.json") -Raw | ConvertFrom-Json
$link = $status.links | Where-Object com -eq $Port
$log = Get-ChildItem (Join-Path $data "logs\comtap-*.log") | Sort-Object LastWriteTime | Select-Object -Last 1
$reopens = @(Select-String -Path $log.FullName -Pattern "Reopening|failed to write")

$problems = @()
if (-not $link) { $problems += "no status for $Port" }
else {
  Write-Host "$Port state=$($link.state) software_open=$($link.software_open) connects=$($link.connects) rx_bytes=$($link.rx_bytes)"
  if ($link.state -ne "connected") { $problems += "state is '$($link.state)', not connected" }
  if ($link.software_open -ne $false) { $problems += "software_open is '$($link.software_open)', not false (is ri=ropen set on side B?)" }
  if ($link.rx_bytes -lt 1) { $problems += "no data came from the device" }
}
if ($reopens.Count) { $problems += "the log shows COM reopen errors" }

if (-not $problems) { Write-Host "$Port idle check passed"; exit 0 }
$problems | ForEach-Object { Write-Host "FAIL: $_" }
Write-Host "`n--- $($log.FullName)"
Get-Content $log.FullName -Tail 40
exit 1
