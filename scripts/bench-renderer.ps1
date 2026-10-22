[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $BaselineExe,
    [Parameter(Mandatory = $true)]
    [string] $CandidateExe,
    [Parameter(Mandatory = $true)]
    [string] $RuntimeRoot,
    [Parameter(Mandatory = $true)]
    [string] $OutputDir,
    [ValidateRange(1, 3600)]
    [int] $WarmupSeconds = 30,
    [ValidateRange(1, 3600)]
    [int] $Seconds = 20,
    [ValidateRange(1, 20)]
    [int] $Repetitions = 2,
    [ValidatePattern('^[1-9][0-9]*x[1-9][0-9]*$')]
    [string] $Resolution = '1280x720',
    [switch] $Walk
)

$ErrorActionPreference = 'Stop'

function Get-FullExistingPath([string] $Path, [string] $Description) {
    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    if ($resolved.Count -ne 1) { throw "$Description muss genau einen Pfad bezeichnen: $Path" }
    return [System.IO.Path]::GetFullPath($resolved[0].Path).TrimEnd('\')
}

function Get-Median([double[]] $Values) {
    if ($Values.Count -eq 0) { return $null }
    $sortedValues = @($Values | Sort-Object)
    $middle = [int][Math]::Floor($sortedValues.Count / 2)
    if (($sortedValues.Count % 2) -eq 1) { return [double]$sortedValues[$middle] }
    return ([double]$sortedValues[$middle - 1] + [double]$sortedValues[$middle]) / 2.0
}

function Get-Percentile([double[]] $Values, [double] $Percentile) {
    if ($Values.Count -eq 0) { return $null }
    $sortedValues = @($Values | Sort-Object)
    $index = [int][Math]::Round(($sortedValues.Count - 1) * $Percentile, [MidpointRounding]::AwayFromZero)
    return [double]$sortedValues[$index]
}

function Assert-NoBuildOrGameProcess {
    $busyNames = @('cargo', 'rustc', 'gothic-reforged', 'GothicReforged')
    $busy = @(Get-Process -Name $busyNames -ErrorAction SilentlyContinue)
    if ($busy.Count -gt 0) {
        $details = ($busy | ForEach-Object { "$($_.ProcessName) (PID $($_.Id))" }) -join ', '
        throw "Benchmark abgebrochen: laufende Build- oder Gothic-Reforged-Prozesse erkannt: $details"
    }
}

function Get-RendererLogDiagnostics([string[]] $LogPaths) {
    $fatal = [System.Collections.Generic.List[string]]::new()
    $review = [System.Collections.Generic.List[string]]::new()
    $unresolvedImportPattern = '(?i)\b(?:unresolved|could not resolve|failed to resolve|could not find|cannot find|failed to load|unable to load|not found|missing)\b.{0,100}\b(?:shader|wesl|naga|imports?)\b|\b(?:shader|wesl|naga|imports?)\b.{0,100}\b(?:unresolved|not found|missing)\b'
    $shaderFailurePattern = '(?i)(?:\b(?:shader|wesl|naga)\b.{0,120}\b(?:compile|compilation|parse|import|validation)\b.{0,80}\b(?:failed|error|invalid|unresolved|not found|missing)\b|\b(?:failed|error|invalid|unresolved|not found|missing|unable to (?:compile|parse|import|load))\b.{0,100}\b(?:shader|wesl|naga|imports?)\b)'
    foreach ($logPath in $LogPaths) {
        if (-not (Test-Path -LiteralPath $logPath -PathType Leaf)) { continue }
        foreach ($line in [System.IO.File]::ReadLines($logPath)) {
            $lineText = $line.Trim()
            if ([string]::IsNullOrWhiteSpace($lineText)) { continue }
            $unresolvedImport = $lineText -match $unresolvedImportPattern
            $warningLine = $lineText -match '(?i)\b(?:WARN|WARNING)\b'
            $rendererError = $lineText -match '(?i)\b(?:ERROR|FATAL|PANIC)\b.{0,160}\b(?:bevy_render|wgpu|renderer|render[_ ](?:device|pipeline|graph)|shader|naga|wesl)\b|\b(?:bevy_render|wgpu|renderer|render[_ ](?:device|pipeline|graph)|shader|naga|wesl)\b.{0,160}\b(?:ERROR|FATAL|PANIC)\b'
            $validationError = $lineText -match '(?i)\b(?:ValidationRenderError|RenderValidationError|Validation Error)\b'
            $shaderFailure = $lineText -match $shaderFailurePattern
            $entry = "${logPath}: $lineText"
            if ($warningLine -and $unresolvedImport) {
                $review.Add($entry)
            } elseif ($validationError -or $rendererError -or $shaderFailure -or $unresolvedImport) {
                $fatal.Add($entry)
            }
        }
    }
    return [pscustomobject]@{
        Fatal = $fatal.ToArray()
        Review = $review.ToArray()
    }
}

function Assert-CompatibleRuntimeSidecar([string] $Exe, [string] $ExpectedRuntime) {
    $sidecar = Join-Path (Split-Path -Parent $Exe) 'runtime-root.txt'
    if (-not (Test-Path -LiteralPath $sidecar -PathType Leaf)) { return }
    $configured = (Get-Content -LiteralPath $sidecar -Raw -ErrorAction Stop).Trim()
    if ([string]::IsNullOrWhiteSpace($configured)) { throw "Leere Runtime-Konfiguration: $sidecar" }
    if (-not [System.IO.Path]::IsPathRooted($configured)) {
        $configured = Join-Path (Split-Path -Parent $Exe) $configured
    }
    $configured = [System.IO.Path]::GetFullPath($configured).TrimEnd('\')
    if (-not [string]::Equals($configured, $ExpectedRuntime, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "runtime-root.txt neben $Exe zeigt auf '$configured', erwartet wird '$ExpectedRuntime'."
    }
}

$baseline = Get-FullExistingPath $BaselineExe 'BaselineExe'
$candidate = Get-FullExistingPath $CandidateExe 'CandidateExe'
$runtime = Get-FullExistingPath $RuntimeRoot 'RuntimeRoot'
foreach ($exe in @($baseline, $candidate)) {
    if (-not [string]::Equals([System.IO.Path]::GetExtension($exe), '.exe', [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "EXE-Pfad muss auf eine .exe-Datei zeigen: $exe"
    }
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "EXE-Datei fehlt: $exe" }
    Assert-CompatibleRuntimeSidecar $exe $runtime
}
foreach ($name in @('items.json', 'npcs.json')) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtime (Join-Path '.local' $name)) -PathType Leaf)) {
        throw "RuntimeRoot benötigt .local/${name}: $runtime"
    }
}

$outputFull = [System.IO.Path]::GetFullPath($OutputDir)
if (Test-Path -LiteralPath $outputFull) { throw "OutputDir muss neu sein und darf nicht überschrieben werden: $outputFull" }
$outputParent = Split-Path -Parent $outputFull
if ([string]::IsNullOrWhiteSpace($outputParent) -or -not (Test-Path -LiteralPath $outputParent -PathType Container)) {
    throw "Der übergeordnete Ordner von OutputDir muss bereits existieren: $outputParent"
}
$outputParent = Get-FullExistingPath $outputParent 'OutputDir parent'
$outputFull = [System.IO.Path]::Combine($outputParent, (Split-Path -Leaf $outputFull))
$runtimePrefix = $runtime.TrimEnd('\') + '\'
if ($outputFull.StartsWith($runtimePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'OutputDir darf nicht innerhalb von RuntimeRoot liegen.'
}

$parsedResolution = $Resolution -split 'x'
if ([int]$parsedResolution[0] -lt 320 -or [int]$parsedResolution[1] -lt 200) {
    throw "Resolution ist zu klein: $Resolution"
}

# Capture inherited GR_* settings before setting the one benchmark control, GR_RES.
$inheritedGrEnvironment = [ordered]@{}
Get-ChildItem Env: | Where-Object { $_.Name.StartsWith('GR_', [System.StringComparison]::OrdinalIgnoreCase) } | ForEach-Object {
    $inheritedGrEnvironment[$_.Name] = $_.Value
}
$baselineHash = (Get-FileHash -LiteralPath $baseline -Algorithm SHA256).Hash.ToLowerInvariant()
$candidateHash = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()

New-Item -ItemType Directory -Path $outputFull -ErrorAction Stop | Out-Null
$runRows = [System.Collections.Generic.List[object]]::new()
$runIndex = 0
$oldResolution = [Environment]::GetEnvironmentVariable('GR_RES', 'Process')
$hadResolution = $null -ne $oldResolution

try {
    $env:GR_RES = $Resolution
    for ($rep = 1; $rep -le $Repetitions; $rep++) {
        foreach ($variant in @('baseline', 'candidate')) {
            Assert-NoBuildOrGameProcess
            $runIndex++
            $exe = if ($variant -eq 'baseline') { $baseline } else { $candidate }
            $runName = 'bench-{0:D2}-{1}-{2}' -f $runIndex, $variant, ([guid]::NewGuid().ToString('N').Substring(0, 8))
            $stdoutPath = Join-Path $outputFull "$runName.stdout.log"
            $stderrPath = Join-Path $outputFull "$runName.stderr.log"
            $argumentList = @('--play', '--bench', "$Seconds", '--bench-warmup', "$WarmupSeconds", '--bench-name', $runName, '--bench-gpu')
            if ($Walk) { $argumentList += '--bench-walk' }
            $startedAt = [DateTime]::UtcNow
            $process = Start-Process -FilePath $exe -ArgumentList $argumentList -WorkingDirectory $runtime -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
            $process.WaitForExit()
            $process.Refresh()
            $rendererDiagnostics = Get-RendererLogDiagnostics @($stdoutPath, $stderrPath)
            if ($rendererDiagnostics.Fatal.Count -gt 0) {
                $details = ($rendererDiagnostics.Fatal | Select-Object -First 8) -join [Environment]::NewLine
                throw "$variant benchmark wegen Renderer-/Shader-Fehlerlogs verworfen. Logs: $stdoutPath ; $stderrPath$([Environment]::NewLine)$details"
            }
            if ($process.ExitCode -ne 0) {
                throw "$variant benchmark endete mit Exitcode $($process.ExitCode). Logs: $stdoutPath ; $stderrPath"
            }
            if ($rendererDiagnostics.Review.Count -gt 0) {
                Write-Warning "$variant benchmark enthält möglicherweise vorübergehende Shader-Importwarnungen; vor Timing-Aussagen manuell prüfen. Logs: $stdoutPath ; $stderrPath"
            }

            $benchDirectory = Join-Path $runtime '.local\bench'
            $matchingJson = @(Get-ChildItem -LiteralPath $benchDirectory -Filter "$runName-*.json" -File -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTimeUtc -ge $startedAt.AddSeconds(-2) })
            if ($matchingJson.Count -ne 1) {
                throw "Erwartete genau eine neue Benchmark-JSON für $runName, gefunden: $($matchingJson.Count). Logs: $stdoutPath ; $stderrPath"
            }
            $jsonDestination = Join-Path $outputFull "$runName.json"
            Copy-Item -LiteralPath $matchingJson[0].FullName -Destination $jsonDestination -ErrorAction Stop
            $measurement = Get-Content -LiteralPath $jsonDestination -Raw | ConvertFrom-Json -ErrorAction Stop
            foreach ($metric in @('avg_ms', 'p50_ms', 'p95_ms', 'gpu_avg_ms')) {
                if ($measurement.PSObject.Properties.Name -notcontains $metric) { throw "Benchmark-JSON $jsonDestination fehlt Feld '$metric'." }
            }
            if ([double]$measurement.avg_ms -le 0 -or [double]$measurement.p50_ms -le 0 -or [double]$measurement.p95_ms -le 0) {
                throw "Ungültige Frame-Zeit in Benchmark-JSON: $jsonDestination"
            }
            $runRows.Add([pscustomobject]@{
                order = $runIndex
                repetition = $rep
                variant = $variant
                exePath = $exe
                exeSha256 = if ($variant -eq 'baseline') { $baselineHash } else { $candidateHash }
                json = (Split-Path -Leaf $jsonDestination)
                stdoutLog = (Split-Path -Leaf $stdoutPath)
                stderrLog = (Split-Path -Leaf $stderrPath)
                rendererLogReviewRequired = ($rendererDiagnostics.Review.Count -gt 0)
                rendererLogReviewFindings = @($rendererDiagnostics.Review)
                startedUtc = $startedAt.ToString('o')
                avgMs = [double]$measurement.avg_ms
                p50Ms = [double]$measurement.p50_ms
                p95Ms = [double]$measurement.p95_ms
                gpuAvgMs = if ($null -eq $measurement.gpu_avg_ms) { $null } else { [double]$measurement.gpu_avg_ms }
                frames = [int]$measurement.frames
                fps = [double]$measurement.fps
            })
        }
    }
}
finally {
    if ($hadResolution) { [Environment]::SetEnvironmentVariable('GR_RES', $oldResolution, 'Process') }
    else { [Environment]::SetEnvironmentVariable('GR_RES', $null, 'Process') }
}

$baselineRuns = @($runRows | Where-Object variant -eq 'baseline')
$candidateRuns = @($runRows | Where-Object variant -eq 'candidate')
$baselineMedian = Get-Median ([double[]]@($baselineRuns | ForEach-Object { $_.avgMs }))
$candidateMedian = Get-Median ([double[]]@($candidateRuns | ForEach-Object { $_.avgMs }))
$medianChangePct = if ($baselineMedian -gt 0) { (($candidateMedian - $baselineMedian) / $baselineMedian) * 100.0 } else { $null }

function Get-GroupSummary($Rows) {
    $avgValues = [double[]]@($Rows | ForEach-Object { $_.avgMs })
    $p50Values = [double[]]@($Rows | ForEach-Object { $_.p50Ms })
    $p95Values = [double[]]@($Rows | ForEach-Object { $_.p95Ms })
    $gpuValues = [double[]]@($Rows | Where-Object { $null -ne $_.gpuAvgMs } | ForEach-Object { $_.gpuAvgMs })
    return [ordered]@{
        runs = $Rows.Count
        medianRunAvgMs = Get-Median $avgValues
        medianRunP50Ms = Get-Median $p50Values
        medianRunP95Ms = Get-Median $p95Values
        medianRunGpuAvgMs = Get-Median $gpuValues
        meanRunAvgMs = if ($avgValues.Count) { ($avgValues | Measure-Object -Average).Average } else { $null }
    }
}

$summary = [ordered]@{
    createdUtc = [DateTime]::UtcNow.ToString('o')
    resolution = $Resolution
    warmupSeconds = $WarmupSeconds
    measuredSeconds = $Seconds
    repetitionsPerVariant = $Repetitions
    walk = [bool]$Walk
    runtimeRoot = $runtime
    executableSha256 = [ordered]@{ baseline = $baselineHash; candidate = $candidateHash }
    inheritedGrEnvironment = $inheritedGrEnvironment
    environmentNote = 'GR_RES wurde für beide Varianten auf Resolution gesetzt; alle anderen GR_* Prozessvariablen wurden geerbt und hier dokumentiert.'
    order = 'baseline, candidate repeated; runs interleaved A/B/A/B.'
    requiresManualRendererLogReview = [bool]@($runRows | Where-Object { $_.rendererLogReviewRequired }).Count
    baseline = Get-GroupSummary $baselineRuns
    candidate = Get-GroupSummary $candidateRuns
    medianRunAvgChangePercent = $medianChangePct
    interpretation = if (@($runRows | Where-Object { $_.rendererLogReviewRequired }).Count -gt 0) { 'Deskriptiver Vergleich; Renderer-/Shader-Importwarnungen erfordern manuelle Logprüfung vor Timing-Aussagen.' } else { 'Deskriptiver Vergleich der Run-Mediane; wenige Wiederholungen liefern keine hohe statistische Sicherheit.' }
    runs = $runRows.ToArray()
}
$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputFull 'summary.json') -Encoding utf8
Write-Output "Benchmark-Ergebnisse: $outputFull"

