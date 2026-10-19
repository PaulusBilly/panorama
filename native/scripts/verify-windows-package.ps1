#requires -Version 7
# Verifies the zip produced by package-windows.ps1, then smoke-runs the packaged exe.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$native = Split-Path -Parent $PSScriptRoot
$repo = Split-Path -Parent $native
$appDir = Join-Path $native 'crates/panorama-app'
$pkgDir = Join-Path $native 'target/package'
$version = (Select-String -Path (Join-Path $appDir 'Cargo.toml') -Pattern '^\s*version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$zip = Join-Path $pkgDir "panorama-$version-windows-x64.zip"
$noticesDir = Join-Path $repo ((Get-Content -Raw (Join-Path $repo 'desktop/native/mpv-host/windows-libmpv.json') | ConvertFrom-Json).noticesDirectory)

# Same stage lookup as package-windows.ps1 (this checkout, else the main checkout of a worktree).
$stageRel = '.cache/panorama/windows-libmpv/current/libmpv-2.dll'
$stagedDll = Join-Path $repo $stageRel
$mainRoot = Split-Path -Parent ([IO.Path]::GetFullPath((git -C $repo rev-parse --git-common-dir), $repo))
if (-not (Test-Path -LiteralPath $stagedDll)) { $stagedDll = Join-Path $mainRoot $stageRel }
foreach ($p in $zip, "$zip.sha256", $stagedDll, $noticesDir) { if (-not (Test-Path -LiteralPath $p)) { throw "Missing: $p" } }

# Expected members; same optional-licence rule as package-windows.ps1.
$expected = [Collections.Generic.List[string]]@('BUILD.txt', 'libmpv-2.dll', 'panorama.exe')
foreach ($f in Get-ChildItem -File -Recurse $noticesDir) { $expected.Add('licenses/libmpv/' + [IO.Path]::GetRelativePath($noticesDir, $f.FullName).Replace('\', '/')) }
if (Test-Path (Join-Path $appDir 'assets/fonts/OFL.txt')) { $expected.Add('licenses/OFL.txt') }
if ((Test-Path (Join-Path $appDir 'assets/icons/LICENSE')) -or (Test-Path (Join-Path $appDir 'assets/tabler/LICENSE'))) { $expected.Add('licenses/tabler-icons-LICENSE') }
if (Test-Path (Join-Path $repo 'LICENSE')) { $expected.Add('licenses/LICENSE') }

function Assert([bool]$ok, [string]$msg) { if (-not $ok) { throw "FAIL: $msg" } }
function Test-Pe64([byte[]]$b) {
    if ($b.Length -lt 0x40 -or $b[0] -ne 0x4D -or $b[1] -ne 0x5A) { return $false }
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    $pe -gt 0 -and $pe + 6 -le $b.Length -and [Text.Encoding]::ASCII.GetString($b, $pe, 4) -eq "PE`0`0" -and [BitConverter]::ToUInt16($b, $pe + 4) -eq 0x8664
}

$zipSha = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
Assert ((Get-Content -Raw "$zip.sha256").Trim().Split(' ')[0] -eq $zipSha) 'zip sha256 does not match .sha256 file'

Add-Type -AssemblyName System.IO.Compression.FileSystem
$tmp = Join-Path ([IO.Path]::GetTempPath()) "panorama-verify-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory $tmp | Out-Null
try {
    $za = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        $names = [string[]]($za.Entries | ForEach-Object FullName)
        foreach ($n in $names) {
            Assert (-not ($n.StartsWith('/') -or $n.Contains('\') -or $n -match '^[A-Za-z]:' -or ($n.Split('/') -contains '..'))) "unsafe member path '$n'"
        }
        Assert ($names.Count -eq @($names | Select-Object -Unique).Count) 'duplicate members'
        $diff = Compare-Object ([string[]]($expected | Sort-Object)) ([string[]]($names | Sort-Object))
        Assert (-not $diff) "member list differs: $($diff | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" })"
        $extract = Join-Path $tmp 'pkg'
        [IO.Compression.ZipFile]::ExtractToDirectory($zip, $extract)
    } finally { $za.Dispose() }

    foreach ($n in 'panorama.exe', 'libmpv-2.dll') { Assert (Test-Pe64 ([IO.File]::ReadAllBytes((Join-Path $extract $n)))) "$n is not a PE x64 image" }
    Assert ((Get-FileHash -Algorithm SHA256 (Join-Path $extract 'libmpv-2.dll')).Hash -eq (Get-FileHash -Algorithm SHA256 $stagedDll).Hash) 'libmpv-2.dll differs from the staged file'
    Write-Host "OK: $($names.Count) members, PE x64, libmpv sha256 matches stage, zip sha256 matches"
    $names | ForEach-Object { Write-Host "  $_" }

    # The release exe is a GUI-subsystem app, so --help output is not capturable; detect the flag in the binary.
    $exePath = Join-Path $extract 'panorama.exe'
    if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exePath)).Contains('--screenshot')) {
        Write-Host 'SKIPPED screenshot smoke: this build of panorama.exe does not support --screenshot.'
    } else {
        $png = Join-Path $tmp 'home.png'
        $p = Start-Process -FilePath $exePath -ArgumentList '--screenshot', 'home', $png -WorkingDirectory $extract -PassThru
        if (-not $p.WaitForExit(60000)) { $p.Kill($true); throw 'FAIL: --screenshot timed out after 60 s' }
        Assert ($p.ExitCode -eq 0) "--screenshot exited with $($p.ExitCode)"
        Assert ((Test-Path $png) -and (Get-Item $png).Length -gt 0) 'screenshot PNG missing or empty'
        Assert (([IO.File]::ReadAllBytes($png)[0..7] -join ',') -eq '137,80,78,71,13,10,26,10') 'screenshot is not a PNG'
        Write-Host "OK: screenshot smoke ($((Get-Item $png).Length) bytes)"
    }
} finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
