[CmdletBinding()]
param(
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = 'aarch64-pc-windows-msvc',

    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version = '1.13.5',

    [switch]$WriteGithubEnv
)

$ErrorActionPreference = 'Stop'

$arch = if ($Target -eq 'aarch64-pc-windows-msvc') { 'arm64' } else { 'x64' }
$assetName = "sherpa-onnx-v$Version-win-$arch-shared-MD-Release-lib.tar.bz2"
$extractName = "sherpa-onnx-v$Version-win-$arch-shared-MD-Release-lib"
$appRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $appRoot 'src-tauri'
$archivePath = Join-Path $tauriRoot $assetName
$extractRoot = Join-Path $tauriRoot $extractName
$libDir = Join-Path $extractRoot 'lib'
$releaseResourceDir = Join-Path $tauriRoot 'target\release'
$releaseBaseUrl = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$Version"
$archiveUrl = "$releaseBaseUrl/$assetName"
$releaseApiUrl = "https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/v$Version"

$runtimeNames = @(
    'onnxruntime.dll',
    'onnxruntime_providers_shared.dll',
    'sherpa-onnx-c-api.dll',
    'sherpa-onnx-cxx-api.dll'
)

function Set-BuildEnvironment {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Value
    )

    Set-Item -Path "Env:$Name" -Value $Value

    if ($WriteGithubEnv) {
        if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
            throw 'GITHUB_ENV is not available, but -WriteGithubEnv was requested.'
        }

        "$Name=$Value" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
    }
}

function Add-BuildPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $pathParts = @($env:PATH -split ';')
    if ($pathParts -notcontains $Path) {
        $env:PATH = "$Path;$env:PATH"
    }

    if ($WriteGithubEnv) {
        if ([string]::IsNullOrWhiteSpace($env:GITHUB_PATH)) {
            throw 'GITHUB_PATH is not available, but -WriteGithubEnv was requested.'
        }

        $Path | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
    }
}

function Get-ExpectedArchiveHash {
    $headers = @{
        Accept = 'application/vnd.github+json'
        'User-Agent' = 'Kiminola-release-build'
    }
    if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) {
        $headers.Authorization = "Bearer $env:GITHUB_TOKEN"
    }

    $release = Invoke-RestMethod -Uri $releaseApiUrl -Headers $headers
    $asset = @($release.assets | Where-Object { $_.name -eq $assetName }) | Select-Object -First 1
    if ($null -eq $asset) {
        throw "The GitHub release v$Version does not contain $assetName."
    }
    if ($asset.digest -notmatch '^sha256:(?<hash>[0-9a-fA-F]{64})$') {
        throw "The GitHub release asset $assetName has no published SHA-256 digest."
    }

    return [PSCustomObject]@{
        Hash = $matches.hash.ToUpperInvariant()
        Size = [int64]$asset.size
    }
}

function Download-NativeArchive {
    if (-not (Get-Command tar.exe -ErrorAction SilentlyContinue)) {
        throw 'tar.exe is required to extract the sherpa-onnx release archive.'
    }

    Write-Host "Downloading $assetName"
    Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath -UseBasicParsing

    $assetMetadata = Get-ExpectedArchiveHash
    if ((Get-Item -LiteralPath $archivePath).Length -ne $assetMetadata.Size) {
        throw "Downloaded size mismatch for $assetName. Expected $($assetMetadata.Size) bytes."
    }

    $expectedHash = $assetMetadata.Hash
    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToUpperInvariant()

    if ($actualHash -ne $expectedHash) {
        throw "SHA-256 mismatch for $assetName. Expected $expectedHash, got $actualHash."
    }

    Write-Host "Verified $assetName ($actualHash)"
    & tar.exe -xjf $archivePath -C $tauriRoot
    if ($LASTEXITCODE -ne 0) {
        throw "tar.exe failed while extracting $assetName (exit code $LASTEXITCODE)."
    }
}

$llvmBin = 'C:\Program Files\LLVM\bin'
$clangPath = Join-Path $llvmBin 'clang.exe'
$clangClPath = Join-Path $llvmBin 'clang-cl.exe'
if (-not (Test-Path -LiteralPath $clangPath)) {
    $clangCommand = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($null -ne $clangCommand) {
        $llvmBin = Split-Path -Parent $clangCommand.Source
        $clangPath = Join-Path $llvmBin 'clang.exe'
        $clangClPath = Join-Path $llvmBin 'clang-cl.exe'
    }
}

if (-not (Test-Path -LiteralPath $clangPath)) {
    throw 'LLVM clang.exe was not found. Install LLVM and put its bin directory on PATH.'
}

Add-BuildPath -Path $llvmBin
Set-BuildEnvironment -Name 'LIBCLANG_PATH' -Value $llvmBin
if ($Target -eq 'aarch64-pc-windows-msvc' -and (Test-Path -LiteralPath $clangClPath)) {
    Set-BuildEnvironment -Name 'CC_aarch64-pc-windows-msvc' -Value $clangClPath
    Set-BuildEnvironment -Name 'CXX_aarch64-pc-windows-msvc' -Value $clangClPath
}

$requiredLib = Join-Path $libDir 'sherpa-onnx-c-api.lib'
if (-not (Test-Path -LiteralPath $requiredLib)) {
    Download-NativeArchive
}

foreach ($runtimeName in $runtimeNames) {
    $runtimePath = Join-Path $libDir $runtimeName
    if (-not (Test-Path -LiteralPath $runtimePath)) {
        throw "The extracted sherpa-onnx package is missing $runtimeName."
    }
}

# tauri.conf.json intentionally stages these four runtime DLLs from target/release
# so cross-target bundles receive the DLLs for the selected architecture.
New-Item -ItemType Directory -Path $releaseResourceDir -Force | Out-Null
foreach ($runtimeName in $runtimeNames) {
    Copy-Item -LiteralPath (Join-Path $libDir $runtimeName) -Destination $releaseResourceDir -Force
}

Set-BuildEnvironment -Name 'SHERPA_ONNX_LIB_DIR' -Value $libDir
Set-BuildEnvironment -Name 'ORT_LIB_PATH' -Value $libDir
Set-BuildEnvironment -Name 'ORT_PREFER_DYNAMIC_LINK' -Value '1'

Write-Host "Native dependencies ready for ${Target}: $libDir"
