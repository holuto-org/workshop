# Dot-sourced by verify-windows.ps1 and test-verify-windows.ps1; requires pwsh.

# Stops every process running the smoke's copied EXE, then deletes the copy.
# Production builds start a Sentry crash reporter from the same EXE that can outlive the app,
# and Windows cannot delete a running image. Match the copy's unique directory: TEMP may be an 8.3 path.
function Remove-WorkshopSmoke([string]$Installed) {
    $copies = @(Get-Process -Name workshop -ErrorAction Ignore | Where-Object Path -like ('*\' + (Split-Path -Leaf $Installed) + '\workshop.exe'))
    foreach ($copy in $copies) { if (-not $copy.HasExited) { $copy.Kill($true) } }
    foreach ($copy in $copies) { $copy.WaitForExit() }
    Remove-Item -LiteralPath $Installed -Recurse -Force
}
