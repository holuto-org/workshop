# Dot-sourced by verify-windows.ps1 and test-verify-windows.ps1; requires pwsh.

# Stops every process running the smoke's copied EXE, then deletes the copy;
# `$Process` is null when the smoke failed before launching the app.
# Production builds also run a Sentry crash reporter child from the same EXE;
# Windows cannot delete an image until every process using it has exited.
# After a crash the reporter outlives its parent, so `Parent` no longer finds it:
# match the uniquely named copy instead (the leaf survives 8.3 TEMP paths).
function Remove-WorkshopSmoke {
    param([Diagnostics.Process]$Process, [string]$Installed)
    $image = '*\' + (Split-Path -Leaf $Installed) + '\workshop.exe'
    $tree = @($Process | Where-Object { $_ }) + @(Get-Process -Name workshop -ErrorAction Ignore | Where-Object { $_.Id -ne $Process.Id -and $_.Path -like $image })
    foreach ($member in $tree) { if (-not $member.HasExited) { $member.Kill($true) } }
    foreach ($member in $tree) { $member.WaitForExit() }
    Remove-Item -LiteralPath $Installed -Recurse -Force
}
