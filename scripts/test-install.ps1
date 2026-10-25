# Run with Windows PowerShell 5.1 or pwsh; no Pester dependency or installation.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$installer = Join-Path $PSScriptRoot 'install.ps1'
. $installer

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-Throws {
    param([scriptblock]$Action, [string]$Pattern)
    try { & $Action } catch {
        if ($_.Exception.Message -notmatch $Pattern) {
            throw "Unexpected failure: $($_.Exception.Message)"
        }
        return
    }
    throw "Expected failure matching: $Pattern"
}

# Only external platform discovery is replaced; platform decisions stay real.
$script:fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('holuto-dry-run-test-' + [Guid]::NewGuid().ToString('N'))
$script:platform = @{
    Platform = [PlatformID]::Win32NT
    Version = [Version]'10.0.19045'
    NativeArchitectures = @(9)
    ProcessorArchitecture = 'AMD64'
    Wow64Architecture = ''
    IsAdministrator = $false
    LocalAppData = Join-Path $script:fixtureRoot 'LocalAppData'
    Programs = Join-Path $script:fixtureRoot 'Programs'
}
function Get-HolutoPlatform { return $script:platform }
$script:latestRequests = 0
function Get-HolutoLatestRelease { $script:latestRequests++; return 'v1.2.3' }
function Invoke-WebRequest { throw 'Unexpected download during dry-run' }
function Get-Process { throw 'Unexpected process inspection during dry-run' }
function New-Item { throw 'Unexpected write during dry-run' }
function Copy-Item { throw 'Unexpected write during dry-run' }
function Set-Content { throw 'Unexpected write during dry-run' }

# A wrong branch here would download, write, or choose a different asset/tag.
$userPathBefore = [Environment]::GetEnvironmentVariable('Path', 'User')
$processPathBefore = [Environment]::GetEnvironmentVariable('Path', 'Process')
$output = (Invoke-HolutoInstall -Release 'v1.2.3' -DryRun | Out-String)
Assert-True ($output.Contains('https://github.com/holuto-org/workshop/releases/download/v1.2.3/workshop-win-x64.exe')) 'Dry-run did not pin the exact asset URL'
Assert-True ($output.Contains('SHA256SUMS.txt')) 'Dry-run omitted checksum verification'
Assert-True ($output.Contains('--signer-workflow holuto-org/workshop/.github/workflows/release.yml')) 'Dry-run omitted signer restriction'
Assert-True ($output.Contains('workshop.exe')) 'Dry-run omitted install target'
Assert-True ($output.Contains('Holuto Workshop.lnk')) 'Dry-run omitted renamed shortcut'
Assert-True ($output.Contains('user PATH')) 'Dry-run omitted user PATH mutation'
Assert-True ($output.Contains('Zone.Identifier')) 'Dry-run omitted retained Internet zone'
Assert-True (-not (Test-Path -LiteralPath $script:fixtureRoot)) 'Dry-run wrote its install directory'
Assert-True ($script:latestRequests -eq 0) 'Pinned dry-run resolved latest'
$output = (Invoke-HolutoInstall -DryRun | Out-String)
Assert-True ($script:latestRequests -eq 1) 'Default release must resolve latest exactly once'
Assert-True ($output.Contains('/download/v1.2.3/workshop-win-x64.exe')) 'Latest was not pinned'
Assert-True ([Environment]::GetEnvironmentVariable('Path', 'User') -ceq $userPathBefore) 'Dry-run changed user PATH'
Assert-True ([Environment]::GetEnvironmentVariable('Path', 'Process') -ceq $processPathBefore) 'Dry-run changed process PATH'

# Not $release: dot-sourcing left the validated $Release parameter in this scope, and names ignore case.
foreach ($candidate in @('', 'latest', '1.2.3', 'V1.2.3', 'v1.2', 'v01.2.3', 'v1.02.3', 'v1.2.03', 'v1.2.3/other', 'v1.2.3?x=1', 'v1.2.3-rc1', "v1.2.3`n")) {
    Assert-Throws { & $installer -Release $candidate -DryRun } 'Release|pattern|validate'
}
Assert-Throws { & $installer -UnknownOption -DryRun } 'parameter|argument'
Assert-Throws { & $installer 'v1.2.3' -DryRun } 'positional|parameter|argument'

# Each fixture changes one native safety condition, including ARM x64 emulation.
foreach ($case in @(
    @{ Key = 'Platform'; Value = [PlatformID]::Unix; Error = 'Windows' },
    @{ Key = 'Version'; Value = [Version]'6.3.9600'; Error = 'Windows 10' },
    @{ Key = 'NativeArchitectures'; Value = @(12); Error = 'x64' },
    @{ Key = 'NativeArchitectures'; Value = @(0); Error = 'x64' },
    @{ Key = 'NativeArchitectures'; Value = @(); Error = 'x64' },
    @{ Key = 'Wow64Architecture'; Value = 'ARM64'; Error = 'x64' },
    @{ Key = 'ProcessorArchitecture'; Value = 'ARM64'; Error = 'x64' }
)) {
    $saved = $script:platform[$case.Key]
    try {
        $script:platform[$case.Key] = $case.Value
        Assert-Throws { Invoke-HolutoInstall -Release 'v1.2.3' -DryRun } $case.Error
    } finally { $script:platform[$case.Key] = $saved }
}
$script:platform.ProcessorArchitecture = 'x86'
$script:platform.Wow64Architecture = 'AMD64'
Invoke-HolutoInstall -Release 'v1.2.3' -DryRun | Out-Null
$script:platform.ProcessorArchitecture = 'AMD64'
$script:platform.Wow64Architecture = ''
Assert-True (-not (Test-Path -LiteralPath $script:fixtureRoot)) 'Platform tests wrote installation files'
$script:platform.IsAdministrator = $true
try {
    Assert-Throws { Invoke-HolutoInstall -Release 'v1.2.3' } 'administrator|elevated'
    $output = (Invoke-HolutoInstall -Release 'v0.0.0' -DryRun | Out-String)
    Assert-True ($output.Contains('actual installation requires a non-elevated PowerShell')) 'Elevated dry-run omitted non-admin requirement'
} finally { $script:platform.IsAdministrator = $false }

$asset = 'workshop-win-x64.exe'
$digest = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
Assert-True ((Get-HolutoExpectedHash -ManifestText "$digest  $asset`n" -Asset $asset) -ceq $digest) 'Valid manifest entry was rejected'
Assert-True ((Get-HolutoExpectedHash -ManifestText "$($digest.ToUpperInvariant()) *$asset`r`n" -Asset $asset) -ceq $digest) 'Binary CRLF manifest entry was rejected'
foreach ($manifest in @(
    "$digest  other.exe`n",
    "$digest  $asset`n$digest  $asset`n",
    "$($digest.Substring(1))  $asset`n",
    "$('z' * 64)  $asset`n",
    "$digest  ./$asset`n",
    "$digest  $asset.extra`n",
    "$digest  $asset`ninvalid  $asset`n"
)) {
    Assert-Throws { Get-HolutoExpectedHash -ManifestText $manifest -Asset $asset } 'manifest|SHA256SUMS'
}

# Exercise redirect validation without network: only an exact repository tag is trusted.
Assert-True ((Get-HolutoReleaseFromRedirect -Location '/holuto-org/workshop/releases/tag/v2.3.4') -ceq 'v2.3.4') 'Relative GitHub redirect rejected'
Assert-True ((Get-HolutoReleaseFromRedirect -Location 'https://github.com/holuto-org/workshop/releases/tag/v2.3.4') -ceq 'v2.3.4') 'Absolute GitHub redirect rejected'
foreach ($location in @(
    'https://evil.example/holuto-org/workshop/releases/tag/v2.3.4',
    'http://github.com/holuto-org/workshop/releases/tag/v2.3.4',
    '/other/workshop/releases/tag/v2.3.4',
    '/holuto-org/workshop/releases/tag/v2.3.4?extra=1',
    '/holuto-org/workshop/releases/tag/v2.3.4-rc1'
)) {
    Assert-Throws { Get-HolutoReleaseFromRedirect -Location $location } 'redirect|release'
}

# PATH changes preserve the original spelling/order and are idempotent.
$directory = 'C:\Users\Alice\AppData\Local\Programs\Holuto Workshop'
Assert-True ((Get-HolutoPathWithDirectory -Path '' -Directory $directory) -ceq $directory) 'Empty PATH was not initialized'
$original = 'C:\Tools;;C:\Other\;'
Assert-True ((Get-HolutoPathWithDirectory -Path $original -Directory $directory) -ceq ($original + $directory)) 'Existing PATH entries were not preserved'
$existing = 'C:\Tools;"c:\USERS\alice\AppData\Local\Programs\HOLUTO WORKSHOP\";C:\Other'
Assert-True ((Get-HolutoPathWithDirectory -Path $existing -Directory $directory) -ceq $existing) 'Equivalent PATH directory was duplicated'
$once = Get-HolutoPathWithDirectory -Path 'C:\Tools' -Directory $directory
Assert-True ((Get-HolutoPathWithDirectory -Path $once -Directory $directory) -ceq 'C:\Tools;C:\Users\Alice\AppData\Local\Programs\Holuto Workshop') 'Repeated PATH update was not idempotent'

# An update replaces the installed executable; a first install moves the staged one into place.
$exeRoot = Join-Path ([IO.Path]::GetTempPath()) ('holuto-replace-test-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($exeRoot)
try {
    $stagedExe = Join-Path $exeRoot '.workshop-staged.exe'
    $targetExe = Join-Path $exeRoot 'workshop.exe'
    [IO.File]::WriteAllText($stagedExe, 'first')
    Install-HolutoStagedExecutable -Staged $stagedExe -Target $targetExe
    Assert-True ([IO.File]::ReadAllText($targetExe) -ceq 'first' -and -not (Test-Path -LiteralPath $stagedExe)) 'First install did not move the staged executable'
    [IO.File]::WriteAllText($stagedExe, 'update')
    Install-HolutoStagedExecutable -Staged $stagedExe -Target $targetExe
    Assert-True ([IO.File]::ReadAllText($targetExe) -ceq 'update' -and -not (Test-Path -LiteralPath $stagedExe)) 'Update did not replace the installed executable'
    Assert-True (@(Get-ChildItem -LiteralPath $exeRoot -Force).Count -eq 1) 'Replacing the executable left a backup behind'
} finally { Remove-Item -LiteralPath $exeRoot -Recurse -Force }

# Downgrades and installed versions that cannot be compared are refused, as in install.sh.
Assert-HolutoNotDowngrade -InstalledVersion '1.2.3' -Release 'v1.2.3'
Assert-HolutoNotDowngrade -InstalledVersion '1.2.9' -Release 'v1.10.0'
Assert-Throws { Assert-HolutoNotDowngrade -InstalledVersion '1.10.0' -Release 'v1.9.9' } 'newer.*downgrades'
foreach ($installed in @($null, '', '1.2', '1.2.3.0', 'v1.2.3', '1.2.3-beta')) {
    Assert-Throws { Assert-HolutoNotDowngrade -InstalledVersion $installed -Release 'v1.2.3' } 'Cannot read the installed version'
}

if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
    # Windows PowerShell 5.1 must write the zone stream too; .NET Framework rejects "file:stream" paths.
    $zoneUrl = 'https://github.com/holuto-org/workshop/releases/download/v1.2.3/workshop-win-x64.exe'
    $zoneFile = Join-Path ([IO.Path]::GetTempPath()) ('holuto-zone-test-' + [Guid]::NewGuid().ToString('N') + '.exe')
    [IO.File]::WriteAllText($zoneFile, 'fixture')
    try {
        Set-HolutoInternetZone -Path $zoneFile -Url $zoneUrl
        $zone = Microsoft.PowerShell.Management\Get-Content -LiteralPath $zoneFile -Stream Zone.Identifier -Raw
        Assert-True ($zone -ceq "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=$zoneUrl`r`n") 'Internet zone was not written'
    } finally { Remove-Item -LiteralPath $zoneFile -Force }

    # The user PATH keeps unexpanded %VAR% entries and its registry kind.
    $expand = [Microsoft.Win32.RegistryValueKind]::ExpandString
    $keyPath = 'Software\HolutoWorkshopInstallerTest-' + [Guid]::NewGuid().ToString('N')
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($keyPath)
    try {
        Assert-True (Add-HolutoUserPath -Key $key -Directory $directory) 'Absent user PATH was not created'
        Assert-True ($key.GetValueKind('Path') -eq $expand) 'New user PATH is not expandable'
        $key.SetValue('Path', '%SystemRoot%\Tools;C:\Other', $expand)
        Assert-True (Add-HolutoUserPath -Key $key -Directory $directory) 'User PATH was not extended'
        $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        Assert-True ($raw -ceq "%SystemRoot%\Tools;C:\Other;$directory") 'User PATH lost its unexpanded entries'
        Assert-True ($key.GetValueKind('Path') -eq $expand) 'User PATH lost REG_EXPAND_SZ'
        Assert-True (-not (Add-HolutoUserPath -Key $key -Directory $directory)) 'Repeated user PATH update was not idempotent'
        $key.SetValue('Path', '%LOCALAPPDATA%\Programs\Holuto Workshop', $expand)
        Assert-True (-not (Add-HolutoUserPath -Key $key -Directory (Join-Path $env:LOCALAPPDATA 'Programs\Holuto Workshop'))) 'Unexpanded equivalent entry was duplicated'
        $key.SetValue('Path', 'C:\Tools', [Microsoft.Win32.RegistryValueKind]::String)
        [void](Add-HolutoUserPath -Key $key -Directory $directory)
        Assert-True ($key.GetValueKind('Path') -eq [Microsoft.Win32.RegistryValueKind]::String) 'User PATH registry kind was changed'
    } finally {
        $key.Dispose()
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($keyPath)
    }
}

# A real install stops before writing the target when gh is not signed in, the
# checksum does not match, or attestation fails. Only external effects are faked.
$script:gh = @{ Auth = 0; Attestation = 0; Calls = @() }
$fakeGh = [pscustomobject]@{ Source = {
    $script:gh.Calls += ($args -join ' ')
    $global:LASTEXITCODE = if ($args[0] -eq 'auth') { $script:gh.Auth } else { $script:gh.Attestation }
} }
function Get-Command {
    param([string]$Name, $CommandType, $ErrorAction)
    if ($Name -eq 'gh') { return $fakeGh }
    Microsoft.PowerShell.Core\Get-Command $Name
}
$script:downloads = 0
$script:manifestDigest = ''
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, [int]$TimeoutSec, [string]$Uri, [string]$OutFile)
    $script:downloads++
    $content = if ($Uri.EndsWith('/SHA256SUMS.txt')) { "$script:manifestDigest  workshop-win-x64.exe`n" } else { 'fixture exe' }
    [IO.File]::WriteAllText($OutFile, $content)
}
function Get-Process { return @() }
# A regression that slips past verification must not reach the real user PATH.
function Add-HolutoUserPath { throw 'Unexpected user PATH write' }
$exeDigest = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes('fixture exe'))).Replace('-', '').ToLowerInvariant()
$installRoot = Join-Path ([IO.Path]::GetTempPath()) ('holuto-install-test-' + [Guid]::NewGuid().ToString('N'))
$installTemp = Join-Path $installRoot 'Temp'
[void][IO.Directory]::CreateDirectory($installTemp)
$savedTemp = @{ TMP = $env:TMP; TEMP = $env:TEMP; TMPDIR = $env:TMPDIR }
$script:platform.LocalAppData = Join-Path $installRoot 'LocalAppData'
$script:platform.Programs = Join-Path $installRoot 'Programs'
$installedExe = Join-Path $script:platform.LocalAppData 'Programs\Holuto Workshop\workshop.exe'
try {
    $env:TMP = $installTemp; $env:TEMP = $installTemp; $env:TMPDIR = $installTemp
    foreach ($case in @(
        @{ Auth = 1; Attestation = 0; Digest = $exeDigest; Error = 'not signed in.*gh auth login'; Downloads = 0 },
        @{ Auth = 0; Attestation = 0; Digest = '0' * 64; Error = 'does not match SHA256SUMS'; Downloads = 2 },
        @{ Auth = 0; Attestation = 1; Digest = $exeDigest; Error = 'attestation verification failed'; Downloads = 2 }
    )) {
        $script:gh = @{ Auth = $case.Auth; Attestation = $case.Attestation; Calls = @() }
        $script:downloads = 0
        $script:manifestDigest = $case.Digest
        Assert-Throws { Invoke-HolutoInstall -Release 'v1.2.3' | Out-Null } $case.Error
        Assert-True ($script:downloads -eq $case.Downloads) "Unexpected downloads before: $($case.Error)"
        Assert-True (-not (Test-Path -LiteralPath $installedExe)) "Installed despite: $($case.Error)"
        Assert-True (@(Get-ChildItem -LiteralPath $installTemp -Force).Count -eq 0) "Left downloads behind after: $($case.Error)"
    }
    $verified = @($script:gh.Calls | Where-Object { $_ -like 'attestation verify *workshop-win-x64.exe --repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml' })
    Assert-True ($verified.Count -eq 1) 'Attestation was not bound to the release workflow'
} finally {
    $env:TMP = $savedTemp.TMP; $env:TEMP = $savedTemp.TEMP; $env:TMPDIR = $savedTemp.TMPDIR
    Remove-Item -LiteralPath $installRoot -Recurse -Force
}

# Both the renamed binary and the previous app must be shut down, never killed.
$script:processes = @()
function Get-Process { return $script:processes }
foreach ($name in @('workshop', 'workshop-win-x64')) {
    $script:processes = @([pscustomobject]@{ ProcessName = $name })
    Assert-Throws { Assert-HolutoNotRunning } 'running.*Quit'
}
$script:processes = @([pscustomobject]@{ ProcessName = 'unrelated' })
Assert-HolutoNotRunning

Write-Output 'PowerShell installer tests passed (no application run, network, or install writes).'
