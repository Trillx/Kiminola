[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Container })]
    [string]$NativeDirectory,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('\.zip$')]
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$requiredDlls = @(
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-c-api.dll',
    'sherpa-onnx-cxx-api.dll'
)
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$nativeRoot = (Resolve-Path -LiteralPath $NativeDirectory).Path
$outputFullPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $outputFullPath
$stagingDirectory = Join-Path $env:TEMP ("kiminola-portable-" + [guid]::NewGuid().ToString('N'))

if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
}
foreach ($name in $requiredDlls) {
    $path = Join-Path $nativeRoot $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Portable package requires runtime dependency '$path'."
    }
}

try {
    New-Item -ItemType Directory -Path $stagingDirectory -Force | Out-Null
    Copy-Item -LiteralPath $executablePath -Destination (Join-Path $stagingDirectory 'kiminola.exe')
    foreach ($name in $requiredDlls) {
        Copy-Item -LiteralPath (Join-Path $nativeRoot $name) -Destination (Join-Path $stagingDirectory $name)
    }

    Remove-Item -LiteralPath $outputFullPath -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $stagingDirectory '*') -DestinationPath $outputFullPath -CompressionLevel Optimal

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($outputFullPath)
    try {
        $actual = @($archive.Entries | ForEach-Object FullName | Sort-Object)
    } finally {
        $archive.Dispose()
    }
    $expected = @('kiminola.exe') + $requiredDlls | Sort-Object
    if (Compare-Object -ReferenceObject $expected -DifferenceObject $actual) {
        throw "Portable archive contents were incomplete: $($actual -join ', ')."
    }
} finally {
    Remove-Item -LiteralPath $stagingDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "Created complete portable package: $outputFullPath"
