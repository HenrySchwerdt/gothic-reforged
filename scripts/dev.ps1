param(
    [ValidateSet('test', 'check', 'clippy', 'scan', 'headless', 'viewer', 'smoke', 'ardea', 'ardea-smoke', 'mesh')]
    [string]$Action = 'headless',
    [string]$Installation = 'D:\SteamLibrary\steamapps\common\Gothic 3',
    [string]$MeshFile = '.local\research\raw-0.xcmsh'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$llvmBin = Join-Path $projectRoot '.local\tools\llvm-mingw-20261006-ucrt-x86_64\bin'
if (-not (Test-Path -LiteralPath (Join-Path $llvmBin 'llvm-dlltool.exe'))) {
    throw 'Local LLVM-MinGW toolchain missing. See docs/HANDOFF_CLAUDE.md for prerequisites, or use normal cargo with a complete system toolchain.'
}
$previousPath = $env:PATH
$previousFlags = $env:RUSTFLAGS
Push-Location $projectRoot
try {
    $gnuFront=Join-Path $projectRoot '.local\tools\gnu-front'
    New-Item -ItemType Directory -Force $gnuFront | Out-Null
    $rustSysroot=& rustc +stable-x86_64-pc-windows-gnu --print sysroot
    $dlltool=Join-Path $gnuFront 'dlltool.exe'
    if (-not (Test-Path -LiteralPath $dlltool)) {
        Copy-Item -LiteralPath (Join-Path $rustSysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained\dlltool.exe') -Destination $dlltool
    }
    $asProxy=Join-Path $gnuFront 'as.exe'
    if (-not (Test-Path -LiteralPath $asProxy)) {
        & rustc +stable-x86_64-pc-windows-gnu --edition=2024 (Join-Path $PSScriptRoot 'gnu-as-proxy.rs') -o $asProxy
        if ($LASTEXITCODE -ne 0) { throw 'Building assembler adapter failed' }
    }
    $env:PATH = "$gnuFront;$llvmBin;$previousPath"
    $env:RUSTFLAGS = "$previousFlags -C link-self-contained=yes -C dlltool=$dlltool".Trim()
    New-Item -ItemType Directory -Force (Join-Path $projectRoot '.local') | Out-Null
    $sceneFile = Join-Path $projectRoot '.local\ardea.node'
    $meshArchive = Join-Path $Installation 'Data\_compiledMesh.pak'
    if ($Action -in @('ardea','ardea-smoke') -and -not (Test-Path -LiteralPath $sceneFile)) {
        $worldArchive = Join-Path $Installation 'Data\Projects_compiled.pak'
        $worldMember = 'G3_World_01/Myrtana/Ardea_City/G3_Myrtana_01_Ardea_Dynamic_Objects_01/G3_Myrtana_01_Ardea_Dynamic_Objects_01_SHyb.node'
        & cargo +stable-x86_64-pc-windows-gnu run -p gothic-inspector --locked -- extract $worldArchive $worldMember $sceneFile
        if ($LASTEXITCODE -ne 0) { throw 'Extracting local read-only scene sample failed' }
    }
    $cargoArgs = switch ($Action) {
        'test' { @('test', '--workspace', '--locked') }
        'check' { @('check', '--workspace', '--all-features', '--locked') }
        'clippy' { @('clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings') }
        'scan' { @('run', '-p', 'gothic-inspector', '--locked', '--', 'scan', $Installation) }
        'headless' { @('run', '-p', 'gothic-reforged', '--locked') }
        'viewer' { @('run', '-p', 'gothic-reforged', '--features', 'viewer', '--locked') }
        'smoke' { @('run', '-p', 'gothic-reforged', '--features', 'viewer', '--locked', '--', '--smoke') }
        'ardea' { @('run','-p','gothic-reforged','--features','viewer','--locked','--','--world',$sceneFile,'--mesh-archive',$meshArchive) }
        'ardea-smoke' { @('run','-p','gothic-reforged','--features','viewer','--locked','--','--smoke','--world',$sceneFile,'--mesh-archive',$meshArchive) }
        'mesh' { @('run','-p','gothic-reforged','--features','viewer','--locked','--','--mesh',$MeshFile) }
    }
    & cargo +stable-x86_64-pc-windows-gnu @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo $Action failed with exit code $LASTEXITCODE" }
} finally {
    $env:PATH = $previousPath
    $env:RUSTFLAGS = $previousFlags
    Pop-Location
}
