#requires -Version 7
# Builds Panorama-Setup-<version>.exe under target/package/ from the verified zip folder that
# package-windows.ps1 assembles. Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$out = Join-Path $repo 'target/package'
$stage = Join-Path $out 'panorama-windows-x64'
$script = Join-Path $repo 'packaging/windows/panorama.iss'

$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue)?.Source
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe')
    (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe')
    (Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe')
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 is not installed. Run: winget install JRSoftware.InnoSetup' }

& (Join-Path $PSScriptRoot 'package-windows.ps1')
if ($LASTEXITCODE) { throw "package-windows.ps1 failed ($LASTEXITCODE)" }
& (Join-Path $PSScriptRoot 'verify-windows-package.ps1')
if ($LASTEXITCODE) { throw "verify-windows-package.ps1 failed ($LASTEXITCODE)" }

foreach ($file in 'panorama.exe', 'libmpv-2.dll') {
    if (-not (Test-Path -LiteralPath (Join-Path $stage $file))) { throw "The package folder is missing $file" }
}

$version = (Select-String -Path (Join-Path $repo 'crates/panorama-app/Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
& $iscc /Qp "/DAppVersion=$version" "/DSourceDir=$stage" "/DOutputDir=$out" $script
if ($LASTEXITCODE) { throw "Inno Setup failed ($LASTEXITCODE)" }

$setup = Join-Path $out "Panorama-Setup-$version.exe"
$sha = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$setup.sha256", "$sha  $(Split-Path -Leaf $setup)`n", [Text.UTF8Encoding]::new($false))
Write-Host "Installer: $setup ($((Get-Item $setup).Length) bytes)`nsha256 $sha"
