[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ExePath,
    [string] $SourceRuntimeRoot = 'C:\Users\PowerTower\dev\gothic-reforged',
    [string] $SourceCodeRoot = (Split-Path -Parent $PSScriptRoot),
    [Parameter(Mandatory = $true)]
    [string] $OutputDir,
    [string] $BuildProfile = 'release-thin'
)

$ErrorActionPreference = 'Stop'

function Normalize-FullPath([string] $Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($full)
    if ([string]::Equals($full, $root, [System.StringComparison]::OrdinalIgnoreCase)) { return $full }
    return $full.TrimEnd('\')
}

function Get-CanonicalExistingPath([string] $Path, [string] $Description) {
    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    if ($resolved.Count -ne 1) { throw "$Description muss genau einen Pfad bezeichnen: $Path" }
    return Normalize-FullPath $resolved[0].Path
}

function Test-PathAtOrBelow([string] $Candidate, [string] $Parent) {
    $candidateFull = Normalize-FullPath $Candidate
    $parentFull = Normalize-FullPath $Parent
    if ([string]::Equals($candidateFull, $parentFull, [System.StringComparison]::OrdinalIgnoreCase)) { return $true }
    return $candidateFull.StartsWith(($parentFull.TrimEnd('\') + '\'), [System.StringComparison]::OrdinalIgnoreCase)
}

$exeFull = Get-CanonicalExistingPath $ExePath 'ExePath'
if (-not [string]::Equals([System.IO.Path]::GetExtension($exeFull), '.exe', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "ExePath muss auf eine .exe-Datei zeigen: $exeFull"
}
$sourceFull = Get-CanonicalExistingPath $SourceRuntimeRoot 'SourceRuntimeRoot'
if (-not (Test-Path -LiteralPath $exeFull -PathType Leaf)) { throw "Die EXE ist keine Datei: $exeFull" }

$outputFull = Normalize-FullPath $OutputDir
if (Test-Path -LiteralPath $outputFull) { throw "OutputDir existiert bereits; es wird nichts überschrieben: $outputFull" }
$outputParent = Split-Path -Parent $outputFull
if ([string]::IsNullOrWhiteSpace($outputParent) -or -not (Test-Path -LiteralPath $outputParent -PathType Container)) {
    throw "Der übergeordnete Ordner von OutputDir muss bereits existieren: $outputParent"
}
$outputParent = Get-CanonicalExistingPath $outputParent 'OutputDir parent'
$outputFull = Normalize-FullPath ([System.IO.Path]::Combine($outputParent, (Split-Path -Leaf $outputFull)))
if ((Test-PathAtOrBelow $outputFull $sourceFull) -or (Test-PathAtOrBelow $sourceFull $outputFull)) {
    throw "OutputDir darf weder SourceRuntimeRoot noch einen Teil davon enthalten: $outputFull"
}

$localSource = Join-Path $sourceFull '.local'
foreach ($name in @('items.json', 'npcs.json', 'objects_world.json')) {
    $requiredPath = Join-Path $localSource $name
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) { throw "Benötigte Runtime-Datei fehlt: $requiredPath" }
}

$exeHashBefore = (Get-FileHash -LiteralPath $exeFull -Algorithm SHA256).Hash.ToLowerInvariant()
$codeFull = Get-CanonicalExistingPath $SourceCodeRoot 'SourceCodeRoot'
$gitHead = (& git -C $codeFull rev-parse HEAD 2>$null | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($gitHead)) { throw "SourceRuntimeRoot ist kein lesbares Git-Repository: $sourceFull" }
$gitStatus = @(& git -C $codeFull status --porcelain 2>$null)
if ($LASTEXITCODE -ne 0) { throw "Git-Status von SourceRuntimeRoot konnte nicht gelesen werden: $sourceFull" }
$sourceDirty = ($gitStatus.Count -gt 0)

# No output is created until every required input and source metadata check has passed.
New-Item -ItemType Directory -Path $outputFull -ErrorAction Stop | Out-Null
$runtimeRoot = Join-Path $outputFull 'runtime'
$runtimeLocal = Join-Path $runtimeRoot '.local'
New-Item -ItemType Directory -Path $runtimeLocal -ErrorAction Stop | Out-Null
New-Item -ItemType Directory -Path (Join-Path $runtimeLocal 'saves') -ErrorAction Stop | Out-Null

$exeDestination = Join-Path $outputFull 'GothicReforged.exe'
Copy-Item -LiteralPath $exeFull -Destination $exeDestination -ErrorAction Stop
$exeHashAfter = (Get-FileHash -LiteralPath $exeDestination -Algorithm SHA256).Hash.ToLowerInvariant()
if ($exeHashBefore -ne $exeHashAfter) { throw 'SHA-256-Prüfung der kopierten EXE fehlgeschlagen.' }

$jsonNames = @('items.json', 'npcs.json', 'objects_world.json', 'objects.json', 'recipes.json', 'spells.json', 'treasure.json', 'layer_flags.json', 'teleports.json', 'touch_triggers.json', 'skills.json')
foreach ($name in $jsonNames) {
    $sourceFile = Join-Path $localSource $name
    if (Test-Path -LiteralPath $sourceFile -PathType Leaf) {
        Copy-Item -LiteralPath $sourceFile -Destination (Join-Path $runtimeLocal $name) -ErrorAction Stop
    }
}

$clipPack = Join-Path $localSource 'er\er_clips.grcp'
if (Test-Path -LiteralPath $clipPack -PathType Leaf) {
    $erDirectory = Join-Path $runtimeLocal 'er'
    New-Item -ItemType Directory -Path $erDirectory -ErrorAction Stop | Out-Null
    Copy-Item -LiteralPath $clipPack -Destination (Join-Path $erDirectory 'er_clips.grcp') -ErrorAction Stop
}

[System.IO.File]::WriteAllText((Join-Path $outputFull 'runtime-root.txt'), 'runtime', [System.Text.UTF8Encoding]::new($false))

$readmeLines = @(
    'Gothic Reforged – lokale PC-Version', '',
    'Start',
    'Doppelklicke GothicReforged.exe. Das Paket braucht die originale Gothic-3-Installation',
    'unter D:\SteamLibrary\steamapps\common\Gothic 3\Data. Die Spieldateien werden direkt',
    'von dort gelesen; dieses Paket enthält keine Original-PAK-Dateien.', '',
    'Steuerung',
    'W/A/S/D: bewegen; Maus: Blickrichtung; Shift: schneller laufen; Leertaste: springen.',
    'I: Inventar. F5: Schnellspeichern; F9: Schnellladen.',
    'F6: manuell speichern; F10: neuesten Spielstand laden. Escape: Menü/Fokus freigeben.', '',
    'Lokale Daten und Spielstände',
    'Runtime-Daten liegen in runtime\.local. Spielstände dieser Paketkopie liegen getrennt',
    'in runtime\.local\saves. Es werden keine Spielstände aus einer anderen Installation',
    'übernommen oder verändert. Sichere runtime\.local\saves vor dem Löschen des Pakets.', '',
    'Leistungsprüfung',
    'Für vergleichbare Messungen starte GothicReforged.exe --play --bench 10 --bench-walk',
    '--bench-name pc-release. Der Benchmark schaltet VSync selbst aus, wärmt die Szene auf',
    'und schreibt Messwerte unter runtime\.local\bench. Vergleiche Messungen mit gleicher',
    'Auflösung und denselben Optionen; ein normales Spielfenster mit VSync eignet sich nicht',
    'zum Vergleich der maximalen Bildrate.', '',
    'Die Laufzeit ist ein getesteter Integrationsstand und kein vollständiger Gothic-3-Port;',
    'Spielinhalte und Funktionen können unvollständig sein.'
)
[System.IO.File]::WriteAllText((Join-Path $outputFull 'LIESMICH.txt'), [string]::Join([Environment]::NewLine, $readmeLines), [System.Text.UTF8Encoding]::new($false))

$buildInfo = [ordered]@{
    exeSha256 = $exeHashAfter
    sourceGitHead = $gitHead
    sourceGitDirty = $sourceDirty
    sourceCodeRoot = $codeFull
    sourceRuntimeRoot = $sourceFull
    createdUtc = [DateTime]::UtcNow.ToString('o')
    buildProfile = $BuildProfile
}
$buildInfo | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputFull 'build-info.json') -Encoding utf8

Write-Output "Paket erstellt: $outputFull"
Write-Output "EXE SHA-256: $exeHashAfter"
