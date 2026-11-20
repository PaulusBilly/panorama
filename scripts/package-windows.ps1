#requires -Version 7
# Builds panorama.exe and assembles a deterministic Windows x64 zip under target/package/.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repo 'libmpv/windows.json'
$appDir = Join-Path $repo 'crates/panorama-app'

function Need([string]$path, [string]$what) {
    if (-not (Test-Path -LiteralPath $path)) { throw "Missing $what`: $path" }
    $path
}

Need $manifestPath 'libmpv manifest' | Out-Null
$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
$noticesDir = Need (Join-Path $repo $manifest.noticesDirectory) 'libmpv notices directory'
$libmpvSha = $manifest.source.sha256
$vulkanSha = $manifest.vulkanLoader.sha256

$stageFiles = 'libmpv-2.dll', 'vulkan-1.dll', 'vulkan-loader-LICENSE'
function Test-Stage([string]$dir) { -not ($stageFiles | Where-Object { -not (Test-Path -LiteralPath (Join-Path $dir $_)) }) }
$stageRel = '.cache/panorama/windows-libmpv/current'
$stageDir = Join-Path $repo $stageRel
$mainRoot = Split-Path -Parent ([IO.Path]::GetFullPath((git -C $repo rev-parse --git-common-dir), $repo))
if (-not (Test-Stage $stageDir) -and (Test-Stage (Join-Path $mainRoot $stageRel))) { $stageDir = Join-Path $mainRoot $stageRel }
if (-not (Test-Stage $stageDir)) {
    Write-Host 'Staged libmpv or Vulkan loader missing; running the stage script.'
    & (Join-Path $PSScriptRoot 'stage-windows-libmpv.ps1')
    $stageDir = Join-Path $repo $stageRel
}
foreach ($f in $stageFiles) { Need (Join-Path $stageDir $f) "staged $f" | Out-Null }
$stagedDll = Join-Path $stageDir 'libmpv-2.dll'

$licenseFiles = [ordered]@{
    'LICENSE'              = Need (Join-Path $repo 'LICENSE') 'repository LICENSE'
    'OFL.txt'              = Need (Join-Path $appDir 'assets/fonts/OFL.txt') 'Aspekta OFL.txt'
    'tabler-icons-LICENSE' = Need (Join-Path $appDir 'assets/icons/LICENSE') 'Tabler icons LICENSE'
}

$version = (Select-String -Path (Join-Path $appDir 'Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$commit = (git -C $repo rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'git rev-parse failed' }

Push-Location $repo
try { cargo build -p panorama-app --release --locked; if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" } }
finally { Pop-Location }
$exe = Need (Join-Path $repo 'target/release/panorama.exe') 'built panorama.exe'
if ([Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)) -match '(?i)vcruntime140') {
    throw 'panorama.exe imports VCRUNTIME140.dll; build with the static CRT from .cargo/config.toml'
}

$out = Join-Path $repo 'target/package'
$stage = Join-Path $out 'panorama-windows-x64'
$zip = Join-Path $out "panorama-$version-windows-x64.zip"
Remove-Item -Recurse -Force $stage, $zip, "$zip.sha256" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses') | Out-Null

Copy-Item $exe (Join-Path $stage 'panorama.exe')
Copy-Item $stagedDll (Join-Path $stage 'libmpv-2.dll')
Copy-Item (Join-Path $stageDir 'vulkan-1.dll') (Join-Path $stage 'vulkan-1.dll')
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses/vulkan-loader') | Out-Null
Copy-Item (Join-Path $stageDir 'vulkan-loader-LICENSE') (Join-Path $stage 'licenses/vulkan-loader/LICENSE')
foreach ($f in Get-ChildItem -File -Recurse $noticesDir) {
    $rel = [IO.Path]::GetRelativePath($noticesDir, $f.FullName)
    $dest = Join-Path $stage "licenses/libmpv/$rel"
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Copy-Item $f.FullName $dest
}
foreach ($k in $licenseFiles.Keys) { Copy-Item $licenseFiles[$k] (Join-Path $stage "licenses/$k") }
$buildInfo = "commit: $commit`nversion: $version`nlibmpv-source-sha256: $libmpvSha`nvulkan-loader-source-sha256: $vulkanSha`n"
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
