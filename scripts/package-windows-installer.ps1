#requires -Version 7
# Builds the Velopack release under target/velopack/ from the verified folder that
# package-windows.ps1 assembles: PanoramaApp-win-Setup.exe, the full .nupkg, a delta from the
# previous release when one is there, and releases.win.json, the feed the app updates from.
# Needs vpk at the same version as the velopack crate:
#   dotnet tool install -g vpk --version 1.3.39-g8d2844f
# With -Publish it first pulls the previous release from GitHub (so a delta can be made) and
# afterwards uploads the new one as a published release, using $env:GITHUB_TOKEN.
param([switch]$Publish)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$stage = Join-Path $repo 'target/package/panorama-windows-x64'
$out = Join-Path $repo 'target/velopack'
$icon = Join-Path $repo 'packaging/windows/panorama.ico'
$repoUrl = 'https://github.com/PaulusBilly/panorama'

if (-not (Get-Command vpk -ErrorAction SilentlyContinue)) {
    throw 'vpk is not installed. Run: dotnet tool install -g vpk --version 1.3.39-g8d2844f'
}

& (Join-Path $PSScriptRoot 'package-windows.ps1')
if ($LASTEXITCODE) { throw "package-windows.ps1 failed ($LASTEXITCODE)" }
& (Join-Path $PSScriptRoot 'verify-windows-package.ps1')
if ($LASTEXITCODE) { throw "verify-windows-package.ps1 failed ($LASTEXITCODE)" }

$version = (Select-String -Path (Join-Path $repo 'crates/panorama-app/Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
New-Item -ItemType Directory -Force $out | Out-Null

if ($Publish) {
    if (-not $env:GITHUB_TOKEN) { throw 'GITHUB_TOKEN is not set' }
    vpk download github --repoUrl $repoUrl --token $env:GITHUB_TOKEN --outputDir $out
    if ($LASTEXITCODE) { throw "vpk download failed ($LASTEXITCODE)" }
}

# The pack id names the install folder, %LocalAppData%\<packId>, which uninstall deletes whole.
# It must not be "Panorama": that folder holds panorama.db, the user's sign-in and history.
vpk pack --packId PanoramaApp --packVersion $version --packTitle Panorama --packAuthors Panorama `
    --packDir $stage --mainExe panorama.exe --icon $icon --runtime win-x64 --outputDir $out
if ($LASTEXITCODE) { throw "vpk pack failed ($LASTEXITCODE)" }

if ($Publish) {
    vpk upload github --repoUrl $repoUrl --token $env:GITHUB_TOKEN --outputDir $out `
        --publish --releaseName "Panorama $version" --tag "v$version" --merge
    if ($LASTEXITCODE) { throw "vpk upload failed ($LASTEXITCODE)" }
}

$setup = Join-Path $out 'PanoramaApp-win-Setup.exe'
Write-Host "Installer: $setup ($((Get-Item $setup).Length) bytes)"
