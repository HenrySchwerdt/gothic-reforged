[CmdletBinding()]
param(
    [string] $SdkRoot = $(if (-not [string]::IsNullOrWhiteSpace($env:ANDROID_HOME)) { $env:ANDROID_HOME } elseif (-not [string]::IsNullOrWhiteSpace($env:ANDROID_SDK_ROOT)) { $env:ANDROID_SDK_ROOT } else { '' }),
    [string] $NdkRoot = '',
    [string] $OpenXrLoader = '',
    [string] $NativeLibrary = '',
    [string] $JdkRoot = $(if (-not [string]::IsNullOrWhiteSpace($env:JAVA_HOME)) { $env:JAVA_HOME } else { '' }),
    [string] $QmineManifest = '',
    [string] $QmineLock = '',
    [string] $StagedAndroidManifest = '',
    [string] $CanonicalAndroidManifest = ''
)

$ErrorActionPreference = 'Stop'
$repo = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if ([string]::IsNullOrWhiteSpace($QmineManifest)) { $QmineManifest = Join-Path $repo '.local/qmine-codex/Cargo.toml' }
if ([string]::IsNullOrWhiteSpace($QmineLock)) { $QmineLock = Join-Path $repo '.local/qmine-codex/Cargo.lock' }
if ([string]::IsNullOrWhiteSpace($StagedAndroidManifest)) { $StagedAndroidManifest = Join-Path $repo '.local/qmine-codex/android/AndroidManifest.xml' }
if ([string]::IsNullOrWhiteSpace($CanonicalAndroidManifest)) { $CanonicalAndroidManifest = Join-Path $repo 'apps/quest/android/AndroidManifest.xml' }

$checks = [System.Collections.Generic.List[object]]::new()
function Test-FullyAbsolute([string] $Path) {
    return (($Path -match '^[A-Za-z]:[\\/]') -or ($Path -match '^(?:\\\\[^\\/]+[\\/][^\\/]+|//[^/]+/[^/]+)(?:[\\/].*)?$'))
}
function Add-PathCheck([string] $Name, [string] $Path, [string] $RequiredType) {
    $fullPath = $null
    $exists = $false
    $reason = $null
    if ([string]::IsNullOrWhiteSpace($Path)) {
        $reason = 'not supplied'
    } elseif (-not (Test-FullyAbsolute $Path)) {
        $reason = 'path must be absolute; relative paths are not resolved against the current directory'
    } else {
        try {
            $fullPath = [System.IO.Path]::GetFullPath($Path)
            $exists = Test-Path -LiteralPath $fullPath -PathType $RequiredType
            if (-not $exists) { $reason = 'missing or wrong filesystem type' }
        } catch {
            $reason = 'invalid path'
            $fullPath = $null
        }
    }
    [void]$checks.Add([pscustomobject]@{
        name = $Name
        path = $fullPath
        requiredType = $RequiredType
        exists = $exists
        reason = $reason
    })
}

function Get-Sha256([string] $Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($Path)
    try { return [System.BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
}

function Get-AbsoluteRoot([string] $Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-FullyAbsolute $Path)) { return $null }
    try { return [System.IO.Path]::GetFullPath($Path) } catch { return $null }
}

Add-PathCheck 'sdkRoot' $SdkRoot 'Container'
Add-PathCheck 'ndkRoot' $NdkRoot 'Container'
Add-PathCheck 'openXrLoader' $OpenXrLoader 'Leaf'
Add-PathCheck 'nativeLibrary' $NativeLibrary 'Leaf'
Add-PathCheck 'jdkRoot' $JdkRoot 'Container'
Add-PathCheck 'qmineManifest' $QmineManifest 'Leaf'
Add-PathCheck 'qmineLock' $QmineLock 'Leaf'
Add-PathCheck 'stagedAndroidManifest' $StagedAndroidManifest 'Leaf'
Add-PathCheck 'canonicalAndroidManifest' $CanonicalAndroidManifest 'Leaf'

$sdk = Get-AbsoluteRoot $SdkRoot
$ndk = Get-AbsoluteRoot $NdkRoot
$jdk = Get-AbsoluteRoot $JdkRoot
if ($null -ne $sdk) {
    Add-PathCheck 'aapt2' (Join-Path $sdk 'build-tools/35.0.0/aapt2.exe') 'Leaf'
    Add-PathCheck 'zipalign' (Join-Path $sdk 'build-tools/35.0.0/zipalign.exe') 'Leaf'
    Add-PathCheck 'apksigner' (Join-Path $sdk 'build-tools/35.0.0/apksigner.bat') 'Leaf'
    Add-PathCheck 'androidJar' (Join-Path $sdk 'platforms/android-34/android.jar') 'Leaf'
} else {
    foreach ($name in @('aapt2', 'zipalign', 'apksigner', 'androidJar')) { Add-PathCheck $name '' 'Leaf' }
}
if ($null -ne $ndk) {
    Add-PathCheck 'llvmStrip' (Join-Path $ndk 'toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe') 'Leaf'
    Add-PathCheck 'libcxxShared' (Join-Path $ndk 'toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so') 'Leaf'
} else {
    foreach ($name in @('llvmStrip', 'libcxxShared')) { Add-PathCheck $name '' 'Leaf' }
}
if ($null -ne $jdk) {
    Add-PathCheck 'java' (Join-Path $jdk 'bin/java.exe') 'Leaf'
    Add-PathCheck 'keytool' (Join-Path $jdk 'bin/keytool.exe') 'Leaf'
} else {
    foreach ($name in @('java', 'keytool')) { Add-PathCheck $name '' 'Leaf' }
}

$canonicalPath = Get-AbsoluteRoot $CanonicalAndroidManifest
$stagedPath = Get-AbsoluteRoot $StagedAndroidManifest
$manifestComparison = [pscustomobject]@{
    canonicalSha256 = $null
    stagedSha256 = $null
    match = $null
    status = 'not checked; one or both manifest files are missing or invalid'
}
if ($null -ne $canonicalPath -and $null -ne $stagedPath -and
    (Test-Path -LiteralPath $canonicalPath -PathType Leaf) -and
    (Test-Path -LiteralPath $stagedPath -PathType Leaf)) {
    $canonicalHash = Get-Sha256 $canonicalPath
    $stagedHash = Get-Sha256 $stagedPath
    $manifestComparison = [pscustomobject]@{
        canonicalSha256 = $canonicalHash
        stagedSha256 = $stagedHash
        match = ($canonicalHash -ceq $stagedHash)
        status = if ($canonicalHash -ceq $stagedHash) { 'match' } else { 'mismatch' }
    }
}

$missing = @($checks | Where-Object { -not $_.exists })
$passed = ($missing.Count -eq 0) -and ($manifestComparison.match -eq $true)
$result = [ordered]@{
    check = 'file-inputs-only'
    pass = $passed
    scope = 'File existence, filesystem type, and canonical/staged AndroidManifest SHA-256 equality only. No binaries or scripts were executed.'
    scriptExpectations = [ordered]@{
        androidBuildTools = '35.0.0 (as referenced by scripts/quest/build-apk.sh; not a platform-policy claim)'
        androidPlatform = 'android-34 (as referenced by scripts/quest/build-apk.sh; not a platform-policy claim)'
        ndkLayout = 'toolchains/llvm/prebuilt/windows-x86_64 and Android arm64 sysroot paths'
    }
    checks = @($checks)
    manifestComparison = $manifestComparison
}
Write-Output ($result | ConvertTo-Json -Depth 7 -Compress)
if ($passed) { exit 0 } else { exit 2 }
