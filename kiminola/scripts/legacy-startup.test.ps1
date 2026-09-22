param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$DatabasePath,

    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$CargoManifestPath,

    [Parameter(Mandatory = $true)]
    [ValidateRange(1, 10000)]
    [int]$ExpectedMigrationCount,

    [int]$TimeoutSeconds = 90
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$cargoManifestFullPath = (Resolve-Path -LiteralPath $CargoManifestPath).Path
if ($TimeoutSeconds -le 0) {
    throw 'TimeoutSeconds must be greater than zero.'
}
if (Test-Path -LiteralPath $DatabasePath -PathType Leaf) {
    throw "Legacy startup probe requires an absent database, but one already exists at '$DatabasePath'."
}

$process = $null
try {
    $process = Start-Process `
        -FilePath $executablePath `
        -WorkingDirectory (Split-Path -Parent $executablePath) `
        -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "Legacy executable exited before initializing its database (exit code $($process.ExitCode))."
        }
        if (Test-Path -LiteralPath $DatabasePath -PathType Leaf) {
            $database = Get-Item -LiteralPath $DatabasePath
            if ($database.Length -gt 0) {
                $env:KIMINOLA_HARDWARE_UPDATE_MODE = 'legacy-ready'
                $env:KIMINOLA_EXPECTED_PREVIOUS_MIGRATION_COUNT = [string]$ExpectedMigrationCount
                try {
                    cargo test --locked --lib `
                        --manifest-path $cargoManifestFullPath `
                        --target $Target `
                        db::tests::hardware_update_database_record_survives `
                        -- --ignored --exact
                    if ($LASTEXITCODE -eq 0) {
                        $process.Refresh()
                        if ($process.HasExited) {
                            throw "Legacy executable exited during the semantic database probe (exit code $($process.ExitCode))."
                        }
                        Write-Host "PASS: legacy executable remained alive and completed the v0.1.2 database at '$DatabasePath'."
                        return
                    }
                } finally {
                    Remove-Item Env:KIMINOLA_HARDWARE_UPDATE_MODE -ErrorAction SilentlyContinue
                    Remove-Item Env:KIMINOLA_EXPECTED_PREVIOUS_MIGRATION_COUNT -ErrorAction SilentlyContinue
                }
            }
        }
    }
    throw "Legacy executable did not complete a usable v0.1.2 database within $TimeoutSeconds seconds."
} finally {
    if ($null -ne $process) {
        try {
            $process.Refresh()
            if (-not $process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction Stop
                if (-not $process.WaitForExit(10000)) {
                    throw "Legacy executable process $($process.Id) did not stop."
                }
            }
        } finally {
            $process.Dispose()
        }
    }
}
