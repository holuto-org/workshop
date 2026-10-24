# Download this script for inspection and verification; do not pipe it into a shell.
# Requires Windows PowerShell 5.1+ or pwsh. Never changes execution policy.
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
        ProcessorArchitecture = $env:PROCESSOR_ARCHITECTURE
        Wow64Architecture = $env:PROCESSOR_ARCHITEW6432
        IsAdministrator = $administrator
        LocalAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
        Programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    }
}

function Assert-HolutoPlatform {
    param([hashtable]$Platform, [switch]$DryRun)
    if ($Platform.Platform -ne [PlatformID]::Win32NT) { throw 'This installer supports Windows only.' }
    if ($Platform.Version -lt [Version]'10.0.10240') { throw 'Windows 10 or newer is required.' }
    # CIM reports the native CPU, unlike PROCESSOR_ARCHITECTURE under emulation.
    if (@($Platform.NativeArchitectures).Count -eq 0 -or
        @($Platform.NativeArchitectures | Where-Object { $_ -ne 9 }).Count -ne 0 -or
        ($Platform.Wow64Architecture -and $Platform.Wow64Architecture -ne 'AMD64') -or
        ($Platform.ProcessorArchitecture -ne 'AMD64' -and
            -not ($Platform.ProcessorArchitecture -eq 'x86' -and $Platform.Wow64Architecture -eq 'AMD64'))) {
        throw 'Only native Windows x64 is supported; ARM and x64 emulation on ARM are not supported.'
    }
    if ($Platform.IsAdministrator -and -not $DryRun) { throw 'Run this per-user installer from a non-elevated PowerShell, not as administrator.' }
    if (-not $Platform.LocalAppData -or -not $Platform.Programs -or
        -not [IO.Path]::IsPathRooted($Platform.LocalAppData) -or -not [IO.Path]::IsPathRooted($Platform.Programs)) {
        throw 'Windows did not provide absolute per-user installation and Start Menu paths.'
    }
}

function Get-HolutoReleaseFromRedirect {
    param([string]$Location)
    $uri = New-Object Uri([Uri]'https://github.com', $Location)
    if ($uri.Scheme -ne 'https' -or $uri.Host -cne 'github.com' -or
        -not $uri.IsDefaultPort -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or
        $uri.AbsolutePath -cnotmatch '\A/holuto-org/workshop/releases/tag/(v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))\z') {
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
    param([AllowEmptyString()][string]$ManifestText, [string]$Asset)
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

function Assert-HolutoPlainPath {
    param([string]$Path)
    # Never write through an existing junction/symlink, including a parent directory.
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force -ErrorAction Stop
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing installation through a reparse point: $current"
            }
        }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}

function Get-HolutoPathWithDirectory {
    param([AllowNull()][AllowEmptyString()][string]$Path, [string]$Directory)
    foreach ($entry in ($Path -split ';')) {
        $expanded = [Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"')).TrimEnd([char[]]'\/')
        if ([StringComparer]::OrdinalIgnoreCase.Equals($expanded, $Directory.TrimEnd([char[]]'\/'))) { return $Path }
    }
    if (-not $Path) { return $Directory }
    if ($Path.EndsWith(';')) { return $Path + $Directory }
    return $Path + ';' + $Directory
}

function Invoke-HolutoInstall {
    [CmdletBinding(PositionalBinding = $false)]
    param(
        [ValidatePattern('(?-i)\Av(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\z')]
        [string]$Release,
        [switch]$DryRun
    )
    $ErrorActionPreference = 'Stop'
    if ($PSVersionTable.PSVersion -lt [Version]'5.1') { throw 'PowerShell 5.1 or newer is required.' }
    $platform = Get-HolutoPlatform
    Assert-HolutoPlatform -Platform $platform -DryRun:$DryRun
    $directory = Join-Path (Join-Path $platform.LocalAppData 'Programs') 'Holuto Workshop'
    $target = Join-Path $directory 'workshop.exe'
    $shortcutPath = Join-Path $platform.Programs 'Holuto Workshop.lnk'
    Write-Output "Install target: $target"
    Write-Output "Start Menu shortcut: $shortcutPath"
    Write-Output "User PATH: add $directory if absent (also update this PowerShell process)."
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
            Write-Output "DRY RUN: If gh exists: gh attestation verify <unique-temp>\$asset --repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml (failure is fatal)."
            Write-Output 'DRY RUN: Only if gh is absent, warn and use weaker checksum-only integrity.'
            Write-Output "DRY RUN: Refuse a running app; retain Internet Zone.Identifier, stage verified EXE, skip equal hash or atomically replace $target; create shortcut with WScript.Shell."
            Write-Output "DRY RUN: Append $directory to user PATH and process PATH only if absent, preserving existing entries."
            Write-Output 'DRY RUN: No downloads or installation writes; the app will not be launched.'
            return
        }
        Assert-HolutoNotRunning
        Assert-HolutoPlainPath -Path $target
        Assert-HolutoPlainPath -Path $shortcutPath
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('workshop-install-' + [Guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($temporary)
        $artifact = Join-Path $temporary $asset
        $manifest = Join-Path $temporary 'SHA256SUMS.txt'
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $artifact
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt" -OutFile $manifest
        $expected = Get-HolutoExpectedHash -ManifestText ([IO.File]::ReadAllText($manifest)) -Asset $asset
        if ((Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) {
            throw 'Downloaded executable SHA-256 does not match SHA256SUMS.txt. Nothing was installed.'
        }
        $gh = Get-Command gh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($gh) {
            & $gh.Source attestation verify $artifact --repo holuto-org/workshop --signer-workflow holuto-org/workshop/.github/workflows/release.yml
            if ($LASTEXITCODE -ne 0) { throw 'GitHub attestation verification failed. Nothing was installed; no checksum-only fallback is allowed when gh is installed.' }
        } else {
            Write-Warning 'gh is absent: using weaker checksum-only integrity. This does not independently authenticate the release or its build provenance.'
        }
        # Invoke-WebRequest does not add MOTW. Keep Internet-zone protection on the installed EXE.
        $zone = "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=$base/$asset`r`n"
        [IO.File]::WriteAllText($artifact + ':Zone.Identifier', $zone, [Text.Encoding]::ASCII)
        Assert-HolutoNotRunning
        Assert-HolutoPlainPath -Path $target
        Assert-HolutoPlainPath -Path $shortcutPath
        $same = (Test-Path -LiteralPath $target -PathType Leaf) -and
            ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $expected)
        if ($same) {
            Write-Output 'The installed executable already has the verified SHA-256; no executable replacement needed.'
        } else {
            [void][IO.Directory]::CreateDirectory($directory)
            $staged = Join-Path $directory ('.workshop-' + [Guid]::NewGuid().ToString('N') + '.exe')
            [IO.File]::Copy($artifact, $staged, $false)
            # Explicitly preserve the ADS even if a filesystem/copy implementation omits streams.
            [IO.File]::WriteAllText($staged + ':Zone.Identifier', [IO.File]::ReadAllText($artifact + ':Zone.Identifier'), [Text.Encoding]::ASCII)
            if ((Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) {
                throw 'Staged executable SHA-256 mismatch; existing installation was not replaced.'
            }
            Assert-HolutoNotRunning
            if (Test-Path -LiteralPath $target) { [IO.File]::Replace($staged, $target, $null) }
            else { [IO.File]::Move($staged, $target) }
            $staged = $null
            Write-Output 'Installed the verified executable.'
        }
        [void][IO.Directory]::CreateDirectory($platform.Programs)
        $shell = New-Object -ComObject WScript.Shell
        try {
            $shortcut = $shell.CreateShortcut($shortcutPath)
            try {
                $shortcut.TargetPath = $target
                $shortcut.WorkingDirectory = $directory
                $shortcut.Description = 'Holuto Workshop'
                $shortcut.Save()
            } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut) }
        } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) }
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $updatedUserPath = Get-HolutoPathWithDirectory -Path $userPath -Directory $directory
        if ($updatedUserPath -cne $userPath) { [Environment]::SetEnvironmentVariable('Path', $updatedUserPath, 'User') }
        $processPath = [Environment]::GetEnvironmentVariable('Path', 'Process')
        $updatedProcessPath = Get-HolutoPathWithDirectory -Path $processPath -Directory $directory
        if ($updatedProcessPath -cne $processPath) { [Environment]::SetEnvironmentVariable('Path', $updatedProcessPath, 'Process') }
        Write-Output 'Installation complete. The app was not launched. Open Holuto Workshop from the Start Menu, or run workshop, when ready.'
        Write-Output 'Internet-zone protection is retained. If SmartScreen appears, review the publisher and approve manually only if you trust this release; do not disable protection.'
        Write-Output 'If script signing policy prevents this installer, use the verified release EXE and create a per-user shortcut manually; do not bypass execution policy.'
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $previousProtocol
        if ($staged -and [IO.File]::Exists($staged)) { [IO.File]::Delete($staged) }
        if ($temporary -and [IO.Directory]::Exists($temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction Stop }
    }
}

# Dot-sourcing exposes functions for the focused tests without doing an install.
if ($MyInvocation.InvocationName -ne '.') { Invoke-HolutoInstall @PSBoundParameters }
