[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^v\d+\.\d+\.\d+$')]
    [string]$PreviousTag,

    [Parameter(Mandatory = $true)]
    [ValidateSet('x64', 'arm64')]
    [string]$Arch,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$CurrentInstaller,

    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$CargoManifestPath,

    [ValidateRange(10, 120)]
    [int]$StartupTimeoutSeconds = 30
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($env:KIMINOLA_HARDWARE_RUNNER -ne '1') {
    throw 'Refusing destructive installer validation outside a dedicated Kimi Nola hardware runner.'
}
if ([string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
    throw 'GH_TOKEN is required to download the previous release.'
}
if ([string]::IsNullOrWhiteSpace($env:GITHUB_REPOSITORY)) {
    throw 'GITHUB_REPOSITORY is required to locate the previous release.'
}

$currentInstallerPath = (Resolve-Path -LiteralPath $CurrentInstaller).Path
$cargoManifestFullPath = (Resolve-Path -LiteralPath $CargoManifestPath).Path
$appRoot = Join-Path $env:LOCALAPPDATA 'Kimi Nola'
$userRoot = Join-Path $env:LOCALAPPDATA 'Kiminola'
$dataRoot = Join-Path $userRoot 'data'
$modelRoot = Join-Path $userRoot 'models\nemotron'
$installedExecutable = Join-Path $appRoot 'kiminola.exe'
$databasePath = Join-Path $dataRoot 'kiminola.db'
$fixtureRoot = Join-Path $env:TEMP ("kiminola-hardware-update-" + [guid]::NewGuid().ToString('N'))
$appBackup = Join-Path $fixtureRoot 'pre-existing-app'
$dataBackup = Join-Path $fixtureRoot 'pre-existing-data'
$appWasBackedUp = $false
$dataWasBackedUp = $false
$modelHashes = @{}
foreach ($file in @('encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt')) {
    $path = Join-Path $modelRoot $file
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "The provisioned ASR model pack is missing $file."
    }
    $modelHashes[$file] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

function Invoke-DatabaseFixture {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('seed', 'verify')]
        [string]$Mode,

        [Parameter(Mandatory = $true)]
        [string]$Marker
    )

    $env:KIMINOLA_HARDWARE_UPDATE_MODE = $Mode
    $env:KIMINOLA_HARDWARE_UPDATE_MARKER = $Marker
    try {
        cargo test --locked --lib `
            --manifest-path $cargoManifestFullPath `
            --target $Target `
            db::tests::hardware_update_database_record_survives `
            -- --ignored --exact
        if ($LASTEXITCODE -ne 0) {
            throw "Hardware database fixture '$Mode' failed."
        }
    } finally {
        Remove-Item Env:KIMINOLA_HARDWARE_UPDATE_MODE -ErrorAction SilentlyContinue
        Remove-Item Env:KIMINOLA_HARDWARE_UPDATE_MARKER -ErrorAction SilentlyContinue
    }
}

try {
    $active = @(Get-Process -Name 'kiminola' -ErrorAction SilentlyContinue)
    if ($active.Count -gt 0) {
        throw "Refusing to replace an active Kimi Nola process on the hardware runner: $($active.Id -join ', ')."
    }

    if (Test-Path -LiteralPath $appRoot -PathType Container) {
        Move-Item -LiteralPath $appRoot -Destination $appBackup
        $appWasBackedUp = $true
    }
    if (Test-Path -LiteralPath $dataRoot -PathType Container) {
        Move-Item -LiteralPath $dataRoot -Destination $dataBackup
        $dataWasBackedUp = $true
    }

    $headers = @{
        Authorization = "Bearer $env:GH_TOKEN"
        Accept = 'application/vnd.github+json'
        'X-GitHub-Api-Version' = '2022-11-28'
    }
    $encodedTag = [uri]::EscapeDataString($PreviousTag)
    $releaseUri = "https://api.github.com/repos/$env:GITHUB_REPOSITORY/releases/tags/$encodedTag"
    $release = Invoke-RestMethod -Method Get -Uri $releaseUri -Headers $headers
    if ($release.draft -or $release.prerelease) {
        throw "Previous release '$PreviousTag' must be published and stable."
    }
    $installerAssets = @($release.assets | Where-Object { $_.name -match "_${Arch}-setup\.exe$" })
    if ($installerAssets.Count -ne 1) {
        throw "Expected one previous $Arch installer, found $($installerAssets.Count)."
    }
    $signatureAssets = @($release.assets | Where-Object { $_.name -eq "$($installerAssets[0].name).sig" })
    if ($signatureAssets.Count -ne 1) {
        throw "Expected one updater signature for '$($installerAssets[0].name)', found $($signatureAssets.Count)."
    }
    $previousInstallerPath = Join-Path $fixtureRoot $installerAssets[0].name
    $previousSignaturePath = "$previousInstallerPath.sig"
    Invoke-WebRequest -Uri $installerAssets[0].browser_download_url -Headers $headers -OutFile $previousInstallerPath
    Invoke-WebRequest -Uri $signatureAssets[0].browser_download_url -Headers $headers -OutFile $previousSignaturePath

    $minisignArchive = Join-Path $fixtureRoot 'minisign-0.12-win64.zip'
    Invoke-WebRequest `
        -Uri 'https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip' `
        -OutFile $minisignArchive
    $minisignHash = (Get-FileHash -LiteralPath $minisignArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($minisignHash -ne '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479') {
        throw "Minisign archive hash mismatch: $minisignHash."
    }
    $minisignDirectory = Join-Path $fixtureRoot 'minisign-0.12'
    Expand-Archive -LiteralPath $minisignArchive -DestinationPath $minisignDirectory -Force
    $minisignArchitecture = if ($Arch -eq 'arm64') { 'aarch64' } else { 'x86_64' }
    $minisign = Join-Path $minisignDirectory "minisign-win64\$minisignArchitecture\minisign.exe"
    if (-not (Test-Path -LiteralPath $minisign -PathType Leaf)) {
        throw "Minisign executable is missing: $minisign."
    }
    $tauriConfigPath = Join-Path (Split-Path -Parent $cargoManifestFullPath) 'tauri.conf.json'
    $tauriConfig = Get-Content -Raw -LiteralPath $tauriConfigPath | ConvertFrom-Json
    $publicKeyPath = Join-Path $fixtureRoot 'kiminola-minisign.pub'
    [IO.File]::WriteAllBytes($publicKeyPath, [Convert]::FromBase64String($tauriConfig.plugins.updater.pubkey))
    & $minisign -V -q -p $publicKeyPath -m $previousInstallerPath -x $previousSignaturePath
    if ($LASTEXITCODE -ne 0) {
        throw "Updater signature verification failed for '$($installerAssets[0].name)'."
    }
    Remove-Item Env:GH_TOKEN

    $previousInstall = Start-Process -FilePath $previousInstallerPath -ArgumentList '/S' -PassThru -Wait
    if ($previousInstall.ExitCode -ne 0) { throw "Previous installer exited with $($previousInstall.ExitCode)." }
    if (-not (Test-Path -LiteralPath $installedExecutable -PathType Leaf)) {
        throw "Previous installer did not create $installedExecutable."
    }

    & (Join-Path $PSScriptRoot 'startup-behavior.test.ps1') -Executable $installedExecutable -TimeoutSeconds $StartupTimeoutSeconds
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf) -or (Get-Item -LiteralPath $databasePath).Length -eq 0) {
        throw 'The previous release did not initialize a non-empty SQLite database.'
    }

    $databaseMarker = "hardware-upgrade-$PreviousTag-$Arch-$([guid]::NewGuid().ToString('N'))"
    Invoke-DatabaseFixture -Mode seed -Marker $databaseMarker

    $currentInstall = Start-Process -FilePath $currentInstallerPath -ArgumentList '/S' -PassThru -Wait
    if ($currentInstall.ExitCode -ne 0) { throw "Current installer exited with $($currentInstall.ExitCode)." }
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf) -or (Get-Item -LiteralPath $databasePath).Length -eq 0) {
        throw 'SQLite database was lost during update installation.'
    }
    Invoke-DatabaseFixture -Mode verify -Marker $databaseMarker
    foreach ($file in $modelHashes.Keys) {
        $path = Join-Path $modelRoot $file
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $modelHashes[$file]) {
            throw "ASR model hashes changed during update installation: $file."
        }
    }

    & (Join-Path $PSScriptRoot 'startup-behavior.test.ps1') -Executable $installedExecutable -TimeoutSeconds $StartupTimeoutSeconds
    Write-Host "PASS: $PreviousTag -> current $Arch installer preserved a real database record and the ASR model pack, then launched successfully."
} finally {
    Get-Process -Name 'kiminola' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $appRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $dataRoot -Recurse -Force -ErrorAction SilentlyContinue
    if ($appWasBackedUp) {
        Move-Item -LiteralPath $appBackup -Destination $appRoot
    }
    if ($dataWasBackedUp) {
        New-Item -ItemType Directory -Path $userRoot -Force | Out-Null
        Move-Item -LiteralPath $dataBackup -Destination $dataRoot
    }
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
