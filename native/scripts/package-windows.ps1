#requires -Version 7
# Builds panorama.exe and assembles a deterministic Windows x64 zip under native/target/package/.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$native = Split-Path -Parent $PSScriptRoot
$repo = Split-Path -Parent $native
$manifestPath = Join-Path $repo 'desktop/native/mpv-host/windows-libmpv.json'
$appDir = Join-Path $native 'crates/panorama-app'

function Need([string]$path, [string]$what) {
    if (-not (Test-Path -LiteralPath $path)) { throw "Missing $what`: $path" }
    $path
}

Need $manifestPath 'libmpv manifest' | Out-Null
$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
$noticesDir = Need (Join-Path $repo $manifest.noticesDirectory) 'libmpv notices directory'
$libmpvSha = $manifest.source.sha256

# libmpv: reuse the existing stage (this checkout's, else the main checkout's when run from a git
# worktree); only the stage script is allowed to download.
$stageRel = '.cache/panorama/windows-libmpv/current/libmpv-2.dll'
$stagedDll = Join-Path $repo $stageRel
$mainRoot = Split-Path -Parent ([IO.Path]::GetFullPath((git -C $repo rev-parse --git-common-dir), $repo))
if (-not (Test-Path -LiteralPath $stagedDll) -and (Test-Path -LiteralPath (Join-Path $mainRoot $stageRel))) {
    $stagedDll = Join-Path $mainRoot $stageRel
}
if (-not (Test-Path -LiteralPath $stagedDll)) {
    Write-Host 'Staged libmpv missing; running the stage script.'
    node (Join-Path $repo 'desktop/scripts/stage-windows-libmpv.mjs')
    if ($LASTEXITCODE -ne 0) { throw "stage-windows-libmpv.mjs failed ($LASTEXITCODE)" }
}
Need $stagedDll 'staged libmpv-2.dll' | Out-Null

# OFL.txt and the repository LICENSE do not exist on every base branch yet: they are packaged when
# present and a loud warning is printed when not. verify-windows-package.ps1 applies the same rule.
$licenseFiles = [ordered]@{}
foreach ($c in @(
        @('OFL.txt', (Join-Path $appDir 'assets/fonts/OFL.txt')),
        @('tabler-icons-LICENSE', (Join-Path $appDir 'assets/icons/LICENSE')),
        @('tabler-icons-LICENSE', (Join-Path $appDir 'assets/tabler/LICENSE')),
        @('LICENSE', (Join-Path $repo 'LICENSE')))) {
    if (Test-Path -LiteralPath $c[1]) { $licenseFiles[$c[0]] = $c[1] }
}
foreach ($n in 'OFL.txt', 'LICENSE') {
    if (-not $licenseFiles.Contains($n)) { Write-Warning "$n not found in the repository; it is NOT in this package." }
}

$version = (Select-String -Path (Join-Path $appDir 'Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$commit = (git -C $repo rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'git rev-parse failed' }

Push-Location $native
try { cargo build -p panorama-app --release --locked; if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" } }
finally { Pop-Location }
$exe = Need (Join-Path $native 'target/release/panorama.exe') 'built panorama.exe'

$out = Join-Path $native 'target/package'
$stage = Join-Path $out 'panorama-windows-x64'
$zip = Join-Path $out "panorama-$version-windows-x64.zip"
Remove-Item -Recurse -Force $stage, $zip, "$zip.sha256" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses') | Out-Null

Copy-Item $exe (Join-Path $stage 'panorama.exe')
Copy-Item $stagedDll (Join-Path $stage 'libmpv-2.dll')
foreach ($f in Get-ChildItem -File -Recurse $noticesDir) {
    $rel = [IO.Path]::GetRelativePath($noticesDir, $f.FullName)
    $dest = Join-Path $stage "licenses/libmpv/$rel"
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Copy-Item $f.FullName $dest
}
foreach ($k in $licenseFiles.Keys) { Copy-Item $licenseFiles[$k] (Join-Path $stage "licenses/$k") }
$buildInfo = "commit: $commit`nversion: $version`nlibmpv-source-sha256: $libmpvSha`n"
[IO.File]::WriteAllText((Join-Path $stage 'BUILD.txt'), $buildInfo, [Text.UTF8Encoding]::new($false))

# Deterministic zip: ordinal-sorted names, forward slashes, fixed timestamps.
Add-Type -AssemblyName System.IO.Compression
$byName = @{}
foreach ($f in Get-ChildItem -File -Recurse $stage) { $byName[[IO.Path]::GetRelativePath($stage, $f.FullName).Replace('\', '/')] = $f.FullName }
$names = [string[]]$byName.Keys
[Array]::Sort($names, [StringComparer]::Ordinal)
$fs = [IO.File]::Create($zip)
try {
    $za = [IO.Compression.ZipArchive]::new($fs, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($n in $names) {
            $e = $za.CreateEntry($n, [IO.Compression.CompressionLevel]::Optimal)
            $e.LastWriteTime = [DateTimeOffset]::new(2020, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
            $es = $e.Open()
            try { $src = [IO.File]::OpenRead($byName[$n]); try { $src.CopyTo($es) } finally { $src.Dispose() } } finally { $es.Dispose() }
        }
    } finally { $za.Dispose() }
} finally { $fs.Dispose() }

$zipSha = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$zip.sha256", "$zipSha  $(Split-Path -Leaf $zip)`n", [Text.UTF8Encoding]::new($false))
Write-Host "Packaged $zip ($((Get-Item $zip).Length) bytes)`nsha256 $zipSha"
