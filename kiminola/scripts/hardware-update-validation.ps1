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
. (Join-Path $PSScriptRoot 'hardware-recovery.ps1')

if ($env:KIMINOLA_HARDWARE_RUNNER -ne '1') {
    throw 'Refusing destructive installer validation outside a dedicated Kimi Nola hardware runner.'
}

$appRoot = Join-Path $env:LOCALAPPDATA 'Kimi Nola'
$userRoot = Join-Path $env:LOCALAPPDATA 'Kiminola'
$dataRoot = Join-Path $userRoot 'data'
$modelRoot = Join-Path $userRoot 'models\nemotron'
$installedExecutable = Join-Path $appRoot 'kiminola.exe'
$databasePath = Join-Path $dataRoot 'kiminola.db'
$provisionedSpeechFixture = Join-Path $userRoot 'hardware\speech-test.wav'
$provisionedExpectedTranscript = Join-Path $userRoot 'hardware\speech-test.txt'
$fixtureRoot = Join-Path $env:TEMP "kiminola-hardware-update-$([guid]::NewGuid().ToString('N'))"
$postInstallSpeechFixture = Join-Path $fixtureRoot 'speech-test.wav'
$postInstallExpectedTranscript = Join-Path $fixtureRoot 'speech-test.txt'
$recoveryRoot = Join-Path $env:LOCALAPPDATA 'KiminolaHardwareRecovery'
$recoveryJournal = Join-Path $recoveryRoot 'active-transaction.json'


if ([string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
    throw 'GH_TOKEN is required to download the previous release.'
}
if ([string]::IsNullOrWhiteSpace($env:GITHUB_REPOSITORY)) {
    throw 'GITHUB_REPOSITORY is required to locate the previous release.'
}
$currentInstallerPath = (Resolve-Path -LiteralPath $CurrentInstaller).Path
$cargoManifestFullPath = (Resolve-Path -LiteralPath $CargoManifestPath).Path

$modelHashes = @{}
foreach ($file in @('encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt')) {
    $path = Join-Path $modelRoot $file
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "The provisioned ASR model pack is missing $file."
    }
    $modelHashes[$file] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}
foreach ($fixture in @($provisionedSpeechFixture, $provisionedExpectedTranscript)) {
    if (-not (Test-Path -LiteralPath $fixture -PathType Leaf)) {
        throw "The provisioned post-install ASR fixture is missing: $fixture."
    }
}

$active = @(Get-Process -Name 'kiminola' -ErrorAction SilentlyContinue)
if ($active.Count -gt 0) {
    throw "Refusing to replace an active Kimi Nola process on the hardware runner: $($active.Id -join ', ')."
}

$transactionId = [guid]::NewGuid().ToString('N')
$transactionRoot = Join-Path $recoveryRoot $transactionId
$stateSet = New-KiminolaHardwareStateSet -TransactionRoot $transactionRoot
$appState = $stateSet.App
$userState = $stateSet.User
$startMenuState = $stateSet.StartMenu
$desktopState = $stateSet.Desktop
$uninstallRegistryState = $stateSet.UninstallRegistry
$installRegistryState = $stateSet.InstallRegistry
Test-HardwareStatePlan -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates

function Assert-InstalledArchitecture {
    $paths = @(
        $installedExecutable,
        (Join-Path $appRoot 'onnxruntime.dll'),
        (Join-Path $appRoot 'onnxruntime_providers_shared.dll'),
        (Join-Path $appRoot 'sherpa-onnx-c-api.dll'),
        (Join-Path $appRoot 'sherpa-onnx-cxx-api.dll')
    )
    & (Join-Path $PSScriptRoot 'verify-pe-architecture.ps1') -Target $Target -Paths $paths
}

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

function Invoke-InstalledAsrFixture {
    $env:KIMINOLA_ASR_MODEL_DIR = $modelRoot
    $env:KIMINOLA_TEST_SPEECH_WAV = $postInstallSpeechFixture
    $env:KIMINOLA_REQUIRE_ASR_FIXTURE = '1'
    $env:KIMINOLA_EXPECTED_SPEECH_TEXT = (Get-Content -Raw -LiteralPath $postInstallExpectedTranscript).Trim()
    try {
        cargo test --locked --lib `
            --manifest-path $cargoManifestFullPath `
            --target $Target `
            asr::tests::session_transcribes_speech_wav `
            -- --exact
        if ($LASTEXITCODE -ne 0) {
            throw 'ASR failed after candidate installation.'
        }
    } finally {
        Remove-Item Env:KIMINOLA_ASR_MODEL_DIR -ErrorAction SilentlyContinue
        Remove-Item Env:KIMINOLA_TEST_SPEECH_WAV -ErrorAction SilentlyContinue
        Remove-Item Env:KIMINOLA_REQUIRE_ASR_FIXTURE -ErrorAction SilentlyContinue
        Remove-Item Env:KIMINOLA_EXPECTED_SPEECH_TEXT -ErrorAction SilentlyContinue
    }
}

New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null
try {
    Copy-Item -LiteralPath $provisionedSpeechFixture -Destination $postInstallSpeechFixture
    Copy-Item -LiteralPath $provisionedExpectedTranscript -Destination $postInstallExpectedTranscript
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
    $migrationUri = "https://api.github.com/repos/$env:GITHUB_REPOSITORY/contents/kiminola/src-tauri/migrations?ref=$encodedTag"
    $migrationEntries = @(Invoke-RestMethod -Method Get -Uri $migrationUri -Headers $headers)
    $expectedPreviousMigrationCount = @($migrationEntries | Where-Object {
        $_.type -eq 'file' -and $_.name -match '^\d+_.+\.sql$'
    }).Count
    if ($expectedPreviousMigrationCount -le 0) {
        throw "Previous release '$PreviousTag' exposes no migration files in tagged source."
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
    $rawPreviousSignaturePath = "$previousSignaturePath.raw"
    [IO.File]::WriteAllBytes(
        $rawPreviousSignaturePath,
        [Convert]::FromBase64String((Get-Content -Raw -LiteralPath $previousSignaturePath).Trim())
    )
    & $minisign -V -q -p $publicKeyPath -m $previousInstallerPath -x $rawPreviousSignaturePath
    if ($LASTEXITCODE -ne 0) {
        throw "Updater signature verification failed for '$($installerAssets[0].name)'."
    }
} catch {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    throw
}
Remove-Item Env:GH_TOKEN

New-Item -ItemType Directory -Path $transactionRoot -Force | Out-Null
Write-HardwareRecoveryJournal `
    -JournalPath $recoveryJournal `
    -TransactionId $transactionId `
    -PathStates $stateSet.PathStates `
    -RegistryStates $stateSet.RegistryStates
try {
    Export-HardwareRegistryKey -State $uninstallRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Export-HardwareRegistryKey -State $installRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates

    Backup-HardwarePath -State $appState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Backup-HardwarePath -State $userState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Backup-HardwarePath -State $startMenuState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Backup-HardwarePath -State $desktopState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Start-HardwareRegistryRemoval -State $uninstallRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Remove-ExportedHardwareRegistryKey -State $uninstallRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Start-HardwareRegistryRemoval -State $installRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates
    Remove-ExportedHardwareRegistryKey -State $installRegistryState
    Write-HardwareRecoveryJournal -JournalPath $recoveryJournal -TransactionId $transactionId `
        -PathStates $stateSet.PathStates -RegistryStates $stateSet.RegistryStates

    if ($userState.SnapshotComplete) {
        $backedUpModelRoot = Join-Path $userState.Backup 'models\nemotron'
        if (Test-Path -LiteralPath $backedUpModelRoot -PathType Container) {
            New-Item -ItemType Directory -Path (Split-Path -Parent $modelRoot) -Force | Out-Null
            Copy-Item -LiteralPath $backedUpModelRoot -Destination $modelRoot -Recurse
        }
    }

    $previousInstall = Start-Process -FilePath $previousInstallerPath -ArgumentList '/S' -PassThru -Wait
    if ($previousInstall.ExitCode -ne 0) { throw "Previous installer exited with $($previousInstall.ExitCode)." }
    if (-not (Test-Path -LiteralPath $installedExecutable -PathType Leaf)) {
        throw "Previous installer did not create $installedExecutable."
    }
    Assert-InstalledArchitecture

    & (Join-Path $PSScriptRoot 'legacy-startup.test.ps1') `
        -Executable $installedExecutable `
        -DatabasePath $databasePath `
        -Target $Target `
        -CargoManifestPath $cargoManifestFullPath `
        -ExpectedMigrationCount $expectedPreviousMigrationCount `
        -TimeoutSeconds $StartupTimeoutSeconds
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf) -or (Get-Item -LiteralPath $databasePath).Length -eq 0) {
        throw 'The previous release did not initialize a non-empty SQLite database.'
    }

    $databaseMarker = "hardware-upgrade-$PreviousTag-$Arch-$([guid]::NewGuid().ToString('N'))"
    Invoke-DatabaseFixture -Mode seed -Marker $databaseMarker

    $currentInstall = Start-Process -FilePath $currentInstallerPath -ArgumentList '/S' -PassThru -Wait
    if ($currentInstall.ExitCode -ne 0) { throw "Current installer exited with $($currentInstall.ExitCode)." }
    Assert-InstalledArchitecture
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf) -or (Get-Item -LiteralPath $databasePath).Length -eq 0) {
        throw 'SQLite database was lost during update installation.'
    }
    & (Join-Path $PSScriptRoot 'startup-behavior.test.ps1') -Executable $installedExecutable -TimeoutSeconds $StartupTimeoutSeconds
    Invoke-DatabaseFixture -Mode verify -Marker $databaseMarker
    foreach ($file in $modelHashes.Keys) {
        $path = Join-Path $modelRoot $file
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $modelHashes[$file]) {
            throw "ASR model hashes changed during update installation: $file."
        }
    }
    Invoke-InstalledAsrFixture

    Write-Host "PASS: $PreviousTag -> current $Arch installer preserved the complete regression fixture and model pack, launched, and revalidated ASR."
} finally {
    $restoreErrors = @()
    try {
        Restore-KiminolaHardwareStateSet -StateSet $stateSet
    } catch {
        $restoreErrors += $_.Exception.Message
    }
    if ($restoreErrors.Count -eq 0) {
        try {
            Write-HardwareRecoveryJournal `
                -JournalPath $recoveryJournal `
                -TransactionId $transactionId `
                -PathStates $stateSet.PathStates `
                -RegistryStates $stateSet.RegistryStates `
                -RestoreComplete $true
            Remove-Item -LiteralPath $transactionRoot -Recurse -Force -ErrorAction Stop
            Remove-Item -LiteralPath $recoveryJournal -Force -ErrorAction Stop
            Remove-Item -LiteralPath "$recoveryJournal.previous" -Force -ErrorAction SilentlyContinue
        } catch {
            $restoreErrors += "remove completed recovery transaction: $($_.Exception.Message)"
        }
    }
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    if ($restoreErrors.Count -gt 0) {
        throw "Hardware runner restoration failed; recovery data remains under '$recoveryRoot': $($restoreErrors -join '; ')"
    }
}
