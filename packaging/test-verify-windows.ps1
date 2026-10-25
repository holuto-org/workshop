# Windows pwsh only: Windows refuses to delete an image a process is running.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-smoke.ps1')

function New-SmokeCopy {
    $installed = Join-Path ([IO.Path]::GetTempPath()) ('workshop-verify-' + [Guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($installed)
    $installed
}

# Cleanup must keep the smoke's own failure and delete the copy; $failure names the expected message.
function Assert-Cleanup([string]$Installed, [string]$Failure) {
    $actual = $null
    try {
        try { throw $Failure } finally { Remove-WorkshopSmoke $Installed }
    } catch {
        $actual = $_.Exception.Message
    }
    if ($actual -ne $Failure) { throw "Cleanup replaced the smoke failure: $actual" }
    if (Test-Path -LiteralPath $Installed) { throw 'Smoke copy was not deleted' }
}

# Failure before launch: nothing is running.
Assert-Cleanup (New-SmokeCopy) 'CLI help failed'

# A copied cmd.exe stands in for the crashed app: it starts a same-image child,
# as the app starts its crash reporter, and exits while the child keeps running.
$installed = New-SmokeCopy
$exe = Join-Path $installed 'workshop.exe'
Copy-Item -LiteralPath (Join-Path ([Environment]::SystemDirectory) 'cmd.exe') -Destination $exe
$parent = Start-Process $exe -ArgumentList "/c start `"`" /b `"$exe`" /c ping -n 60 127.0.0.1" -WindowStyle Hidden -PassThru
$parent.WaitForExit()
$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
    $orphans = @(Get-Process -Name workshop -ErrorAction Ignore | Where-Object Path -like ('*\' + (Split-Path -Leaf $installed) + '\workshop.exe'))
    if ($orphans.Count -gt 0) { break }
    [Threading.Thread]::Sleep(50)
} while ([DateTime]::UtcNow -lt $deadline)
if ($orphans.Count -eq 0) { throw 'Fixture did not leave a same-image child running' }
Assert-Cleanup $installed 'Application exited: 1'
foreach ($orphan in $orphans) {
    if (-not $orphan.HasExited) { $orphan.Kill(); throw 'Same-image child survived cleanup' }
}
Write-Host 'Verified smoke cleanup before launch and with an orphaned same-image child'
