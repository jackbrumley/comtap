# Release install test: reads one line from a ComTap virtual COM port and checks
# it. On failure, prints ComTap's status, log and traffic log, so the run says why.
param(
  [Parameter(Mandatory)][string]$Port,
  [Parameter(Mandatory)][string]$Expect
)

$exe = Join-Path $env:ProgramFiles "ComTap\comtap.exe"
$line = $null
$serial = [System.IO.Ports.SerialPort]::new($Port)
$serial.ReadTimeout = 20000
try {
  $serial.Open()
  $line = $serial.ReadLine()
} catch {
  Write-Host "Reading ${Port} failed: $_"
} finally {
  $serial.Close()
}
Write-Host "$Port received: $line"
if ($line -match [regex]::Escape($Expect)) { exit 0 }

Write-Host "`n--- comtap status"
& $exe status
$data = Join-Path $env:ProgramData "ComTap"
foreach ($pattern in "logs\comtap-*.log", "traffic\traffic-*.log") {
  foreach ($file in Get-ChildItem (Join-Path $data $pattern) -ErrorAction SilentlyContinue) {
    Write-Host "`n--- $($file.FullName)"
    Get-Content $file.FullName -Tail 40
  }
}
exit 1
