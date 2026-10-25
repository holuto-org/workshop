#Requires -Version 5.1
# Download this script for inspection and verification; do not pipe it into a shell.
# Never changes execution policy.
[CmdletBinding(PositionalBinding = $false)]
param(
    [ValidatePattern('(?-i)\Av(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\z')]
    [string]$Release,
    [switch]$DryRun
)

function Get-HolutoPlatform {
    $platform = [Environment]::OSVersion.Platform
    if ($platform -ne [PlatformID]::Win32NT) { return @{ Platform = $platform } }
    $os = Get-CimInstance -ClassName Win32_OperatingSystem -Property Version -ErrorAction Stop
    $architectures = @(Get-CimInstance -ClassName Win32_Processor -Property Architecture -ErrorAction Stop | ForEach-Object { [int]$_.Architecture })
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $principal = New-Object Security.Principal.WindowsPrincipal($identity)
        $administrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    } finally { $identity.Dispose() }
    return @{
        Platform = $platform
        Version = [Version]$os.Version
        NativeArchitectures = $architectures
        IsAdministrator = $administrator
        LocalAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
        Programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    }
}

function Assert-HolutoPlatform {
    param([hashtable]$Platform, [switch]$DryRun)
    if ($Platform.Platform -ne [PlatformID]::Win32NT) { throw 'This installer supports Windows only.' }
    if ($Platform.Version -lt [Version]'10.0.10240') { throw 'Windows 10 or newer is required.' }
    # CIM reports the native CPU (9 = x64), unlike PROCESSOR_ARCHITECTURE under emulation.
    if (@($Platform.NativeArchitectures).Count -eq 0 -or @($Platform.NativeArchitectures | Where-Object { $_ -ne 9 }).Count -ne 0) {
        throw 'Only native Windows x64 is supported; ARM and x64 emulation on ARM are not supported.'
    }
    if ($Platform.IsAdministrator -and -not $DryRun) { throw 'Run this per-user installer from a non-elevated PowerShell, not as administrator.' }
    if (-not $Platform.LocalAppData -or -not $Platform.Programs) {
        throw 'Windows did not provide per-user installation and Start Menu paths.'
    }
}

function Get-HolutoReleaseFromRedirect {
    param([string]$Location)
    # Only the tag is used; downloads always go to the fixed repository URL.
    if ($Location -cnotmatch '\A(?:https://github\.com)?/holuto-org/workshop/releases/tag/(v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))\z') {
        throw 'Unexpected latest-release redirect; specify -Release vX.Y.Z explicitly.'
    }
    return $Matches[1]
}

function Get-HolutoLatestRelease {
    # One metadata request, no automatic redirect and no downloaded release content.
    $request = [Net.HttpWebRequest]::Create('https://github.com/holuto-org/workshop/releases/latest')
    $request.Method = 'HEAD'
    $request.AllowAutoRedirect = $false
    $request.Timeout = 30000
    $request.UserAgent = 'Holuto-Workshop-Installer'
    $response = $request.GetResponse()
    try {
        if ([int]$response.StatusCode -notin @(301, 302, 303, 307, 308)) { throw 'GitHub did not return a latest-release redirect.' }
        return Get-HolutoReleaseFromRedirect -Location $response.Headers['Location']
    } finally { $response.Close() }
}

function Get-HolutoExpectedHash {
    param([string]$ManifestText, [string]$Asset)
    $escaped = [Regex]::Escape($Asset)
    $entries = @($ManifestText -split '\r?\n' | Where-Object { $_ -cmatch ('(?:\A|[ \t*])' + $escaped + '\z') })
    if ($entries.Count -ne 1 -or $entries[0] -cnotmatch ('\A([0-9a-fA-F]{64}) [ *]' + $escaped + '\z')) {
        throw "SHA256SUMS.txt must contain exactly one valid manifest entry for $Asset."
    }
    return $Matches[1].ToLowerInvariant()
}

function Assert-HolutoNotRunning {
    $running = @(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -in @('workshop', 'workshop-win-x64') })
    if ($running.Count -ne 0) { throw 'Holuto Workshop is running. Quit it from its tray menu, then run the installer again. No process was stopped.' }
}

function Get-HolutoPathWithDirectory {
    param([string]$Path, [string]$Directory)
    foreach ($entry in ($Path -split ';')) {
        $expanded = [Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"')).TrimEnd([char[]]'\/')
        if ([StringComparer]::OrdinalIgnoreCase.Equals($expanded, $Directory.TrimEnd([char[]]'\/'))) { return $Path }
    }
    if (-not $Path) { return $Directory }
    if ($Path.EndsWith(';')) { return $Path + $Directory }
    return $Path + ';' + $Directory
}

function Set-HolutoInternetZone {
    param([string]$Path, [string]$Url)
    # .NET Framework (Windows PowerShell 5.1) rejects "file:stream" paths; the provider's -Stream works in 5.1 and pwsh.
    Microsoft.PowerShell.Management\Set-Content -LiteralPath $Path -Stream Zone.Identifier -Encoding Ascii -NoNewline `
        -Value "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=$Url`r`n"
}

function Add-HolutoUserPath {
    param([Microsoft.Win32.RegistryKey]$Key, [string]$Directory)
    # The Environment API's User scope reads Path expanded and writes REG_SZ, freezing every %VAR% entry.
    $current = [string]$Key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $updated = Get-HolutoPathWithDirectory -Path $current -Directory $Directory
    if ($updated -ceq $current) { return $false }
    $kind = if ($Key.GetValueNames() -contains 'Path') { $Key.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
    $Key.SetValue('Path', $updated, $kind)
    return $true
}

function Update-HolutoUserPath {
    param([string]$Directory)
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try {
        if (Add-HolutoUserPath -Key $key -Directory $Directory) { Send-HolutoEnvironmentChange }
    } finally { $key.Dispose() }
}

function New-HolutoShortcut {
    param([string]$Path, [string]$Target)
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
    $shortcut.TargetPath = $Target
    $shortcut.WorkingDirectory = [IO.Path]::GetDirectoryName($Target)
    $shortcut.Description = 'Holuto Workshop'
    $shortcut.Save()
}

function Send-HolutoEnvironmentChange {
    # The broadcast Environment.SetEnvironmentVariable sends after a User write, so new terminals see the PATH.
    $native = Add-Type -Namespace Holuto -Name EnvironmentBroadcast -PassThru -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
'@
    $result = [UIntPtr]::Zero
    # HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG, 1 s per window.
    [void]$native::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 1000, [ref]$result)
}

function Assert-HolutoNotDowngrade {
    param([string]$InstalledVersion, [string]$Release)
    # Same rule as install.sh: an installed version that cannot be compared is never replaced.
    if ($InstalledVersion -cnotmatch '\A(?:0|[1-9][0-9]{0,8})\.(?:0|[1-9][0-9]{0,8})\.(?:0|[1-9][0-9]{0,8})\z') {
        throw 'Cannot read the installed version; refusing to replace it.'
    }
    $version = $Release.Substring(1)
    if ([Version]$InstalledVersion -gt [Version]$version) {
        throw "Installed version $InstalledVersion is newer than $version; downgrades are not supported."
    }
}

function Install-HolutoStagedExecutable {
    param([string]$Staged, [string]$Target)
    # PowerShell passes $null to a .NET string parameter as "", which File.Replace rejects as a backup path.
    if (Test-Path -LiteralPath $Target) { [IO.File]::Replace($Staged, $Target, [NullString]::Value) }
    else { [IO.File]::Move($Staged, $Target) }
}

function Invoke-HolutoInstall {
    [CmdletBinding(PositionalBinding = $false)]
    param(
        [string]$Release,
        [switch]$DryRun
    )
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 redraws progress per chunk, slowing Invoke-WebRequest by an order of magnitude.
    $ProgressPreference = 'SilentlyContinue'
    $platform = Get-HolutoPlatform
    Assert-HolutoPlatform -Platform $platform -DryRun:$DryRun
    $directory = Join-Path (Join-Path $platform.LocalAppData 'Programs') 'Holuto Workshop'
    $target = Join-Path $directory 'workshop.exe'
    $shortcutPath = Join-Path $platform.Programs 'Holuto Workshop.lnk'
    Write-Output "Install target: $target"
    Write-Output "Start Menu shortcut: $shortcutPath"
    Write-Output "User PATH: add $directory if absent."
    if ($platform.IsAdministrator) { Write-Output 'Dry-run only: actual installation requires a non-elevated PowerShell, not administrator.' }
    $previousProtocol = [Net.ServicePointManager]::SecurityProtocol
    $temporary = $null
    $staged = $null
    try {
        # Windows PowerShell 5.1 otherwise inherits legacy HTTPS protocol defaults.
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        if (-not $Release) { $Release = Get-HolutoLatestRelease }
        $asset = 'workshop-win-x64.exe'
        $base = "https://github.com/holuto-org/workshop/releases/download/$Release"
        Write-Output "Release: $Release"
        if ($DryRun) {
            Write-Output "DRY RUN: Invoke-WebRequest $base/$asset -OutFile <unique-temp>\$asset"
            Write-Output "DRY RUN: Invoke-WebRequest $base/SHA256SUMS.txt -OutFile <unique-temp>\SHA256SUMS.txt"
            Write-Output 'DRY RUN: Require one exact 64-hex manifest entry and matching Get-FileHash -Algorithm SHA256.'
            Write-Output "DRY RUN: If gh exists: gh attestation verify <unique-temp>\$asset --repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml --source-ref refs/heads/master (failure is fatal)."
            Write-Output 'DRY RUN: Only if gh is absent, warn and use weaker checksum-only integrity.'
            Write-Output "DRY RUN: Refuse a running app or a newer installed version; stage the verified EXE with an Internet Zone.Identifier and atomically replace $target; create shortcut with WScript.Shell."
            Write-Output "DRY RUN: Append $directory to user PATH only if absent, preserving existing entries."
            Write-Output 'DRY RUN: No downloads or installation writes; the app will not be launched.'
            return
        }
        Assert-HolutoNotRunning
        if (Test-Path -LiteralPath $target -PathType Leaf) {
            Assert-HolutoNotDowngrade -InstalledVersion (Get-Item -LiteralPath $target).VersionInfo.ProductVersion -Release $Release
        }
        $gh = Get-Command gh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('workshop-install-' + [Guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($temporary)
        $artifact = Join-Path $temporary $asset
        $manifest = Join-Path $temporary 'SHA256SUMS.txt'
        # pwsh has no default timeout; 7.4+ also aborts stalled reads. Windows PowerShell 5.1 already aborts those.
        $download = @{ UseBasicParsing = $true; TimeoutSec = 30 }
        if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey('OperationTimeoutSeconds')) { $download.OperationTimeoutSeconds = 60 }
        Invoke-WebRequest @download -Uri "$base/$asset" -OutFile $artifact
        Invoke-WebRequest @download -Uri "$base/SHA256SUMS.txt" -OutFile $manifest
        $expected = Get-HolutoExpectedHash -ManifestText ([IO.File]::ReadAllText($manifest)) -Asset $asset
        if ((Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) {
            throw 'Downloaded executable SHA-256 does not match SHA256SUMS.txt. Nothing was installed.'
        }
        if ($gh) {
            # The signer workflow path alone also matches a modified release.yml pushed to any other branch.
            & $gh.Source attestation verify $artifact --repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml --source-ref refs/heads/master
            if ($LASTEXITCODE -ne 0) { throw 'GitHub attestation verification failed (gh must be 2.68.0 or newer and signed in to github.com). Nothing was installed.' }
        } else {
            Write-Warning 'gh is absent: using weaker checksum-only integrity. This does not independently authenticate the release or its build provenance.'
        }
        [void][IO.Directory]::CreateDirectory($directory)
        $staged = Join-Path $directory ('.workshop-' + [Guid]::NewGuid().ToString('N') + '.exe')
        [IO.File]::Copy($artifact, $staged, $false)
        # Invoke-WebRequest does not add MOTW. Keep Internet-zone protection on the installed EXE.
        Set-HolutoInternetZone -Path $staged -Url "$base/$asset"
        Assert-HolutoNotRunning
        Install-HolutoStagedExecutable -Staged $staged -Target $target
        $staged = $null
        New-HolutoShortcut -Path $shortcutPath -Target $target
        Update-HolutoUserPath -Directory $directory
        Write-Output 'Installation complete. The app was not launched. Open Holuto Workshop from the Start Menu, or run workshop in a new terminal, when ready.'
        Write-Output 'Internet-zone protection is retained. If SmartScreen appears, review the publisher and approve manually only if you trust this release; do not disable protection.'
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $previousProtocol
        if ($staged -and [IO.File]::Exists($staged)) { [IO.File]::Delete($staged) }
        if ($temporary -and [IO.Directory]::Exists($temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction Stop }
    }
}

# Dot-sourcing exposes functions for the focused tests without doing an install.
if ($MyInvocation.InvocationName -ne '.') { Invoke-HolutoInstall @PSBoundParameters }
