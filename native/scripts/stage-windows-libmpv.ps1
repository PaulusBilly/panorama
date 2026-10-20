#requires -Version 7
# Downloads the pinned libmpv archive from native/libmpv/windows.json, verifies it, and stages
# its headers and DLL in .cache/panorama/windows-libmpv/current/. Needs 7-Zip (7z.exe).
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$native = Split-Path -Parent $PSScriptRoot
$repo = Split-Path -Parent $native
$manifest = Get-Content -Raw (Join-Path $repo 'native/libmpv/windows.json') | ConvertFrom-Json

function Assert-SafePath([string]$value, [string]$label) {
    $n = $value.Replace('\', '/')
    if (-not $n -or $n.StartsWith('/') -or $n -match '^[a-z]:/' -or ($n.Split('/') | Where-Object { $_ -in '', '.', '..' })) {
        throw "Unsafe libmpv $label path: $value"
    }
}

if ($manifest.schemaVersion -ne 1 -or $manifest.architecture -ne 'x64') { throw 'Unsupported libmpv manifest' }
$url = [Uri]$manifest.source.url
if ($url.Scheme -ne 'https' -or $url.AbsolutePath -match '(^|[/-])latest([/-]|$)') { throw 'The libmpv source URL must be immutable HTTPS' }
$hash = $manifest.source.sha256.ToLowerInvariant()
if ($hash -notmatch '^[a-f0-9]{64}$') { throw 'Invalid libmpv SHA-256' }
$required = @($manifest.files.headers) + @($manifest.files.runtime) + @($manifest.files.rejectedLinkInputs)
foreach ($entry in $required) { Assert-SafePath $entry 'inventory' }
Assert-SafePath $manifest.noticesDirectory 'notices'

$sevenZip = (Get-Command 7z -ErrorAction SilentlyContinue)?.Source ?? (Join-Path $env:ProgramFiles '7-Zip/7z.exe')
if (-not (Test-Path -LiteralPath $sevenZip)) { throw '7-Zip is required: install it or put 7z.exe on PATH' }

$cache = Join-Path $repo '.cache/panorama/windows-libmpv'
$root = Join-Path $cache $hash
$archive = Join-Path $root 'libmpv.7z'
$extracted = Join-Path $root 'extracted'
$marker = Join-Path $root '.complete.json'
$current = Join-Path $cache 'current'
New-Item -ItemType Directory -Force $root | Out-Null

if (-not (Test-Path -LiteralPath $archive)) {
    $response = Invoke-WebRequest -Uri $url -OutFile "$archive.download" -PassThru
    $final = $response.BaseResponse.RequestMessage.RequestUri
    if ($final.Scheme -ne 'https' -or $final.AbsolutePath -match '(^|[/-])latest([/-]|$)') { throw 'The resolved libmpv source URL must be immutable HTTPS' }
    Move-Item -Force "$archive.download" $archive
}
if ((Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant() -ne $hash) { throw 'libmpv SHA-256 mismatch' }

function Assert-Runtime {
    foreach ($entry in $required) {
        if (-not (Test-Path -LiteralPath (Join-Path $extracted $entry))) { throw "Missing libmpv file: $entry" }
    }
    foreach ($entry in $manifest.files.runtime) {
        $b = [IO.File]::ReadAllBytes((Join-Path $extracted $entry))
        $pe = if ($b.Length -ge 64 -and $b[0] -eq 0x4d -and $b[1] -eq 0x5a) { [BitConverter]::ToUInt32($b, 0x3c) } else { 0 }
        if ($pe -lt 0x40 -or $pe + 6 -gt $b.Length -or [BitConverter]::ToUInt32($b, $pe) -ne 0x4550) { throw "Invalid Windows PE runtime: $entry" }
        if ([BitConverter]::ToUInt16($b, $pe + 4) -ne 0x8664) { throw "Windows native runtime must be x64: $entry" }
    }
}

$cached = $false
try {
    $m = Get-Content -Raw $marker | ConvertFrom-Json
    if ($m.schemaVersion -eq 1 -and $m.sha256 -eq $hash) { Assert-Runtime; $cached = $true }
} catch { $cached = $false }

if (-not $cached) {
    $listing = & $sevenZip l -slt $archive
    if ($LASTEXITCODE -ne 0) { throw "7z list failed ($LASTEXITCODE)" }
    $members = $listing | Select-Object -Skip ([Array]::IndexOf($listing, '----------') + 1) |
        Where-Object { $_.StartsWith('Path = ') } | ForEach-Object { $_.Substring(7).Replace('\', '/') }
    foreach ($member in $members) { Assert-SafePath $member 'archive member' }
    foreach ($entry in $required) { if ($entry -notin $members) { throw "Missing libmpv archive member: $entry" } }
    Remove-Item -Recurse -Force $extracted, $marker -ErrorAction SilentlyContinue
    & $sevenZip x -y "-o$extracted" $archive | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "7z extract failed ($LASTEXITCODE)" }
    Assert-Runtime
    Set-Content -NoNewline $marker (@{ schemaVersion = 1; sha256 = $hash } | ConvertTo-Json)
}

Remove-Item -Recurse -Force $current -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $current 'include/mpv') | Out-Null
foreach ($h in $manifest.files.headers) { Copy-Item (Join-Path $extracted $h) (Join-Path $current $h) }
foreach ($r in $manifest.files.runtime) { Copy-Item (Join-Path $extracted $r) (Join-Path $current (Split-Path -Leaf $r)) }
Write-Host "Staged libmpv in $current"
