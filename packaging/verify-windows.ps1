$ErrorActionPreference = 'Stop'
$files = @(Get-ChildItem -Path 'release/workshop-*.exe')
if ($files.Count -ne 1) { throw 'Expected exactly one release EXE' }
$bytes = [IO.File]::ReadAllBytes($files[0].FullName)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
$optional = $pe + 24
if ([BitConverter]::ToUInt16($bytes, $optional + 68) -ne 2) { throw 'EXE must use the GUI subsystem' }
$sections = [BitConverter]::ToUInt16($bytes, $pe + 6)
$sectionTable = $optional + [BitConverter]::ToUInt16($bytes, $pe + 20)
function Get-Offset([uint32]$rva) {
    for ($i = 0; $i -lt $sections; $i++) {
        $section = $sectionTable + 40 * $i
        $start = [BitConverter]::ToUInt32($bytes, $section + 12)
        $size = [Math]::Max([BitConverter]::ToUInt32($bytes, $section + 8), [BitConverter]::ToUInt32($bytes, $section + 16))
        if ($rva -ge $start -and $rva -lt $start + $size) {
            return [int]($rva - $start + [BitConverter]::ToUInt32($bytes, $section + 20))
        }
    }
    throw "Unmapped PE RVA: $rva"
}
$imports = Get-Offset ([BitConverter]::ToUInt32($bytes, $optional + 120))
$dlls = @()
for ($entry = $imports; ; $entry += 20) {
    $nameRva = [BitConverter]::ToUInt32($bytes, $entry + 12)
    if ($nameRva -eq 0) { break }
    $offset = Get-Offset $nameRva
    $end = $offset
    while ($bytes[$end] -ne 0) { $end++ }
    $dlls += [Text.Encoding]::ASCII.GetString($bytes, $offset, $end - $offset)
}
if ($dlls.Count -eq 0) { throw 'PE import inspection found no DLLs' }
if ($dlls -match '^(vcruntime|msvcp|msvcr|ucrtbase|api-ms-win-crt)') {
    throw "Dynamic VC++ runtime import: $($dlls -join ', ')"
}
Write-Host "Verified static CRT; imported DLLs: $($dlls -join ', ')"
# build.rs writes these resources itself, so check each one: exact UTF-16 strings from the
# Polish string table and the icon Explorer extracts, pixel for pixel against the source file.
$metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$version = ($metadata.packages | Where-Object name -eq 'workshop').version
$info = $files[0].VersionInfo
$expected = @{ ProductVersion = $version; FileVersion = $version; ProductName = 'Holuto Workshop'; FileDescription = 'Holuto Workshop · drukarki fiskalne' }
foreach ($field in $expected.Keys) {
    if ($info.$field -cne $expected[$field]) { throw "Wrong $field resource: $($info.$field)" }
}
Add-Type -AssemblyName System.Drawing
$extracted = [Drawing.Icon]::ExtractAssociatedIcon($files[0].FullName)
$size = $extracted.Size
$source = New-Object Drawing.Icon -ArgumentList (Resolve-Path 'crates/desktop/assets/icon.ico').Path, $size
$actual = $extracted.ToBitmap()
$reference = $source.ToBitmap()
try {
    if ($actual.Width -ne $reference.Width -or $actual.Height -ne $reference.Height) { throw "Icon size differs: $($actual.Width)x$($actual.Height)" }
    for ($y = 0; $y -lt $actual.Height; $y++) {
        for ($x = 0; $x -lt $actual.Width; $x++) {
            if ($actual.GetPixel($x, $y).ToArgb() -ne $reference.GetPixel($x, $y).ToArgb()) { throw "EXE icon differs from assets/icon.ico at $x,$y" }
        }
    }
} finally {
    $actual.Dispose(); $reference.Dispose(); $extracted.Dispose(); $source.Dispose()
}
Write-Host "Verified version resources and the ${size} icon"
# Known Folders are not overridden by environment variables: never use saved state.
$local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
if (Test-Path -LiteralPath (Join-Path $local 'workshop')) { throw 'Package smoke requires a clean Windows profile: workshop state exists' }
$installed = Join-Path ([IO.Path]::GetTempPath()) ('workshop-verify-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($installed)
try {
    $exe = Join-Path $installed 'workshop.exe'
    Copy-Item -LiteralPath $files[0].FullName -Destination $exe
    $help = Start-Process $exe -ArgumentList '--help' -Wait -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $installed 'help.txt')
    if ($help.ExitCode -ne 0 -or -not ([IO.File]::ReadAllText((Join-Path $installed 'help.txt'))).Contains('workshop')) { throw 'CLI help failed' }
    $check = Start-Process $exe -ArgumentList 'check', '--json' -Wait -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $installed 'check.json')
    $result = Get-Content -Raw (Join-Path $installed 'check.json') | ConvertFrom-Json
    if ($check.ExitCode -ne 3 -or $result.ok -ne $false -or $result.configured -ne $false -or $result.appRunning -ne $false -or $result.errors.Count -eq 0) { throw 'Fresh configuration CLI check failed' }
    # CLI dispatch precedes telemetry initialization, including in production packages.
    $optOut = Start-Process $exe -ArgumentList 'config', 'set', '--diagnostics', 'off' -Wait -PassThru -NoNewWindow
    if ($optOut.ExitCode -ne 0) { throw 'Diagnostics opt-out failed' }
    $process = Start-Process $exe -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        $process.Refresh()
        if ($process.HasExited) { throw "Application exited: $($process.ExitCode)" }
        if ($process.MainWindowTitle -eq 'Holuto Workshop') { break }
        if ($process.MainWindowTitle -match 'błąd') { throw 'Startup error window appeared' }
        [Threading.Thread]::Sleep(50)
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($process.MainWindowTitle -ne 'Holuto Workshop') { throw "Main window missing: $($process.MainWindowTitle)" }
    Write-Host "Verified main window: $($process.MainWindowTitle)"
} finally {
    # Production builds start a Sentry crash reporter from the same EXE that can outlive the app,
    # and Windows cannot delete a running image. Match the unique directory: TEMP may be an 8.3 path.
    $copies = @(Get-Process -Name workshop -ErrorAction Ignore | Where-Object Path -like ('*\' + (Split-Path -Leaf $installed) + '\workshop.exe'))
    foreach ($copy in $copies) { if (-not $copy.HasExited) { $copy.Kill($true) } }
    foreach ($copy in $copies) { $copy.WaitForExit() }
    Remove-Item -LiteralPath $installed -Recurse -Force
    # Absent before launch (checked above), so this is only the state this smoke created.
    $state = Join-Path $local 'workshop'
    if (Test-Path -LiteralPath $state) { Remove-Item -LiteralPath $state -Recurse -Force }
}
