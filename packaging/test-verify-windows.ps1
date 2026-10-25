# Windows pwsh only: Windows refuses to delete an image a process is running.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-smoke.ps1')

# A copied cmd.exe stands in for the crashed app: it starts a same-image child,
# as the app starts its crash reporter, and exits while the child keeps running.
$installed = Join-Path ([IO.Path]::GetTempPath()) ('workshop-verify-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($installed)
$exe = Join-Path $installed 'workshop.exe'
Copy-Item -LiteralPath (Join-Path ([Environment]::SystemDirectory) 'cmd.exe') -Destination $exe
$process = Start-Process $exe -ArgumentList "/c start `"`" /b `"$exe`" /c ping -n 60 127.0.0.1" -WindowStyle Hidden -PassThru
$process.WaitForExit()
$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
    $orphans = @(Get-Process -Name workshop -ErrorAction Ignore | Where-Object { $_.Path -like ('*\' + (Split-Path -Leaf $installed) + '\workshop.exe') })
    if ($orphans.Count -gt 0) { break }
    [Threading.Thread]::Sleep(50)
} while ([DateTime]::UtcNow -lt $deadline)
if ($orphans.Count -eq 0) { throw 'Fixture did not leave a same-image child running' }

$failure = $null
try {
    try { throw 'Application exited: 1' } finally { Remove-WorkshopSmoke -Process $process -Installed $installed }
} catch {
    $failure = $_.Exception.Message
}
if ($failure -ne 'Application exited: 1') { throw "Cleanup replaced the smoke failure: $failure" }
if (Test-Path -LiteralPath $installed) { throw 'Smoke copy was not deleted' }
foreach ($orphan in $orphans) {
    if (-not $orphan.HasExited) { $orphan.Kill(); throw 'Same-image child survived cleanup' }
}
Write-Host 'Verified smoke cleanup stops a same-image child that outlived its parent'
