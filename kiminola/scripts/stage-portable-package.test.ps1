[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$fixtureRoot = Join-Path $env:TEMP ("kiminola-portable-test-" + [guid]::NewGuid().ToString('N'))
$nativeRoot = Join-Path $fixtureRoot 'native'
$appPath = Join-Path $fixtureRoot 'built-kiminola.exe'
$archivePath = Join-Path $fixtureRoot 'Kimi-Nola-x64-portable.zip'
$requiredDlls = @(
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-c-api.dll',
    'sherpa-onnx-cxx-api.dll'
)

try {
    New-Item -ItemType Directory -Path $nativeRoot -Force | Out-Null
    [IO.File]::WriteAllText($appPath, 'synthetic executable')
    foreach ($name in $requiredDlls) {
        [IO.File]::WriteAllText((Join-Path $nativeRoot $name), "synthetic $name")
    }

    & (Join-Path $PSScriptRoot 'stage-portable-package.ps1') `
        -Executable $appPath `
        -NativeDirectory $nativeRoot `
        -OutputPath $archivePath

    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
        throw 'Portable package script did not create an archive.'
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        $names = @($archive.Entries | ForEach-Object FullName | Sort-Object)
    } finally {
        $archive.Dispose()
    }
    $expected = @('kiminola.exe') + $requiredDlls | Sort-Object
    if (Compare-Object -ReferenceObject $expected -DifferenceObject $names) {
        throw "Portable archive contents were wrong: $($names -join ', ')."
    }

    Remove-Item -LiteralPath (Join-Path $nativeRoot $requiredDlls[0]) -Force
    $missingDependencyFailed = $false
    try {
        & (Join-Path $PSScriptRoot 'stage-portable-package.ps1') `
            -Executable $appPath `
            -NativeDirectory $nativeRoot `
            -OutputPath (Join-Path $fixtureRoot 'must-not-exist.zip')
    } catch {
        $missingDependencyFailed = $_.Exception.Message -match [regex]::Escape($requiredDlls[0])
    }
    if (-not $missingDependencyFailed) {
        throw 'Portable package script did not reject a missing runtime DLL.'
    }

    Write-Host 'PASS: portable archive includes the executable and every required runtime DLL.'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}