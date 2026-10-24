$ErrorActionPreference = 'Stop'
# Same signed W7 x64 driver and certificate used by berkowski/tokio-serial.
$base = 'https://raw.githubusercontent.com/Apollon77/SupportingFiles/a14981be47f3be872a4aec2d989ab529ba362f23/appveyor/serial'
$files = @{
    'setup_com0com_W7_x64_signed.exe' = '559bac8091a255dd3ddb3819b9f2c35491eb5915038901604f59b3349372c2c5'
    'com0com.cer' = '2330e22bd8ce84b574742c98a366f7eaf173dfb474444a807b64a7f3818f78e3'
}
foreach ($name in $files.Keys) {
    $path = Join-Path $env:RUNNER_TEMP $name
    Invoke-WebRequest "$base/$name" -OutFile $path
    if ((Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $files[$name]) {
        throw "SHA-256 mismatch: $name"
    }
}
Import-Certificate -FilePath "$env:RUNNER_TEMP/com0com.cer" -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Import-Certificate -FilePath "$env:RUNNER_TEMP/com0com.cer" -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
$env:CNC_INSTALL_CNCA0_CNCB0_PORTS = 'NO'
$installer = Start-Process "$env:RUNNER_TEMP/setup_com0com_W7_x64_signed.exe" -ArgumentList '/S' -Wait -PassThru
if ($installer.ExitCode -ne 0) { throw "com0com installer failed: $($installer.ExitCode)" }
$setup = "${env:ProgramFiles(x86)}\com0com\setupc.exe"
& $setup install 'PortName=COM10,EmuBR=yes' 'PortName=COM11,EmuBR=yes'
if ($LASTEXITCODE -ne 0) { throw 'com0com pair creation failed' }
& $setup list
if ($LASTEXITCODE -ne 0) { throw 'com0com pair enumeration failed' }
'HOLUTO_TEST_COM_PAIR=COM10,COM11' | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
