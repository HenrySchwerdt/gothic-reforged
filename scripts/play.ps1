param(
    [ValidateSet('play', 'walk')]
    [string]$Mode = 'play',
    [switch]$Fast
)
# Builds an optimized release binary and starts Gothic Reforged.
# -Fast skips link-time optimization (faster build, slightly slower game).
$ErrorActionPreference = 'Continue'
$projectRoot = Split-Path -Parent $PSScriptRoot
$llvmBin = Join-Path $projectRoot '.local\tools\llvm-mingw-20261006-ucrt-x86_64\bin'
$gnuFront = Join-Path $projectRoot '.local\tools\gnu-front'
$dlltool = Join-Path $gnuFront 'dlltool.exe'
$env:PATH = "$gnuFront;$llvmBin;$env:PATH"
$env:RUSTFLAGS = "-C link-self-contained=yes -C dlltool=$dlltool -C target-cpu=native"
$env:CARGO_TARGET_DIR = Join-Path $projectRoot '.local\target-play'
if ($Fast) { $env:CARGO_PROFILE_RELEASE_LTO = 'off' }
Push-Location $projectRoot
try {
    & cargo +stable-x86_64-pc-windows-gnu build --release -p gothic-reforged --features viewer --locked 2>&1 | Where-Object { $_ -match 'error|warning: unused' } | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { throw "Build failed ($LASTEXITCODE)" }
    $exe = Join-Path $env:CARGO_TARGET_DIR 'release\gothic-reforged.exe'
    & $exe "--$Mode"
} finally {
    Pop-Location
}
