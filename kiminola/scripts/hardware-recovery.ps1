[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'hardware-runner-state.ps1')

if ($env:KIMINOLA_HARDWARE_RUNNER -ne '1') {
    throw 'Refusing hardware-runner recovery outside a dedicated Kimi Nola hardware runner.'
}

function New-KiminolaHardwareStateSet {
    param([Parameter(Mandatory = $true)][string]$TransactionRoot)

    $appRoot = Join-Path $env:LOCALAPPDATA 'Kimi Nola'
    $userRoot = Join-Path $env:LOCALAPPDATA 'Kiminola'
    $startMenuShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Kimi Nola.lnk'
    $desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Kimi Nola.lnk'
    $uninstallRegistryPath = 'Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall\Kimi Nola'
    $installRegistryPath = 'Registry::HKEY_CURRENT_USER\Software\kiminola'

    $app = New-HardwarePathState `
        -Root $appRoot `
        -Backup (Join-Path $TransactionRoot 'app') `
        -PathType Container
    $user = New-HardwarePathState `
        -Root $userRoot `
        -Backup (Join-Path $TransactionRoot 'user') `
        -PathType Container
    $startMenu = New-HardwarePathState `
        -Root $startMenuShortcut `
        -Backup (Join-Path $TransactionRoot 'start-menu-shortcut.lnk') `
        -PathType Leaf
    $desktop = New-HardwarePathState `
        -Root $desktopShortcut `
        -Backup (Join-Path $TransactionRoot 'desktop-shortcut.lnk') `
        -PathType Leaf
    $uninstallRegistry = New-HardwareRegistryState `
        -RegistryPath $uninstallRegistryPath `
        -BackupFile (Join-Path $TransactionRoot 'uninstall-registry.reg')
    $installRegistry = New-HardwareRegistryState `
        -RegistryPath $installRegistryPath `
        -BackupFile (Join-Path $TransactionRoot 'install-registry.reg')

    [pscustomobject]@{
        App = $app
        User = $user
        StartMenu = $startMenu
        Desktop = $desktop
        UninstallRegistry = $uninstallRegistry
        InstallRegistry = $installRegistry
        PathStates = @($app, $user, $startMenu, $desktop)
        RegistryStates = @($uninstallRegistry, $installRegistry)
    }
}

function Restore-KiminolaHardwareStateSet {
    param([Parameter(Mandatory = $true)]$StateSet)

    $restoreErrors = @()
    foreach ($process in @(Get-Process -Name 'kiminola' -ErrorAction SilentlyContinue)) {
        try {
            Stop-Process -Id $process.Id -Force -ErrorAction Stop
            if (-not $process.WaitForExit(10000)) {
                throw "Process $($process.Id) did not stop."
            }
        } catch {
            $restoreErrors += "stop test app $($process.Id): $($_.Exception.Message)"
        } finally {
            $process.Dispose()
        }
    }
    foreach ($state in @($StateSet.User, $StateSet.App, $StateSet.StartMenu, $StateSet.Desktop)) {
        try {
            Restore-HardwarePath -State $state
        } catch {
            $restoreErrors += "restore '$($state.Root)' from '$($state.Backup)': $($_.Exception.Message)"
        }
    }
    foreach ($state in $StateSet.RegistryStates) {
        try {
            Restore-HardwareRegistryKey -State $state
        } catch {
            $restoreErrors += "restore registry '$($state.RegistryPath)' from '$($state.BackupFile)': $($_.Exception.Message)"
        }
    }
    if ($restoreErrors.Count -gt 0) {
        throw "Hardware runner restoration failed; recovery data was preserved:`n$($restoreErrors -join "`n")"
    }
}

function Invoke-KiminolaInterruptedHardwareRecovery {
    $recoveryRoot = Join-Path $env:LOCALAPPDATA 'KiminolaHardwareRecovery'
    $recoveryJournal = Join-Path $recoveryRoot 'active-transaction.json'
    if (-not (Test-Path -LiteralPath $recoveryJournal -PathType Leaf)) {
        $orphanedTransactions = @(
            Get-ChildItem -LiteralPath $recoveryRoot -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^[0-9a-f]{32}$' }
        )
        if ($orphanedTransactions.Count -gt 0) {
            throw "Hardware recovery found an orphaned transaction directory without an active journal: $($orphanedTransactions.FullName -join ', ')."
        }
        return
    }

    $journal = Read-HardwareRecoveryJournal -JournalPath $recoveryJournal
    $transactionRoot = Join-Path $recoveryRoot $journal.transaction_id
    $otherTransactions = @(
        Get-ChildItem -LiteralPath $recoveryRoot -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^[0-9a-f]{32}$' -and $_.FullName -ne $transactionRoot }
    )
    if ($otherTransactions.Count -gt 0) {
        throw "Hardware recovery found an orphaned transaction directory outside the active journal: $($otherTransactions.FullName -join ', ')."
    }
    $restoreCompleteProperty = $journal.PSObject.Properties['restore_complete']
    if ($null -ne $restoreCompleteProperty -and [bool]$restoreCompleteProperty.Value) {
        if (Test-Path -LiteralPath $transactionRoot) {
            Remove-Item -LiteralPath $transactionRoot -Recurse -Force -ErrorAction Stop
        }
        if (Test-Path -LiteralPath $transactionRoot) {
            throw "Completed hardware recovery transaction '$transactionRoot' could not be removed; retaining its journal."
        }
        Remove-Item -LiteralPath $recoveryJournal -Force -ErrorAction Stop
        Remove-Item -LiteralPath "$recoveryJournal.previous" -Force -ErrorAction SilentlyContinue
        Write-Host "PASS: completed cleanup for restored Kimi Nola hardware-runner transaction '$($journal.transaction_id)'."
        return
    }
    $stateSet = New-KiminolaHardwareStateSet -TransactionRoot $transactionRoot
    Apply-HardwareRecoveryJournal `
        -Journal $journal `
        -PathStates $stateSet.PathStates `
        -RegistryStates $stateSet.RegistryStates
    Restore-KiminolaHardwareStateSet -StateSet $stateSet
    Write-HardwareRecoveryJournal `
        -JournalPath $recoveryJournal `
        -TransactionId $journal.transaction_id `
        -PathStates $stateSet.PathStates `
        -RegistryStates $stateSet.RegistryStates `
        -RestoreComplete $true
    Remove-Item -LiteralPath $transactionRoot -Recurse -Force -ErrorAction Stop
    Remove-Item -LiteralPath $recoveryJournal -Force -ErrorAction Stop
    Remove-Item -LiteralPath "$recoveryJournal.previous" -Force -ErrorAction SilentlyContinue
    Write-Host "PASS: recovered interrupted Kimi Nola hardware-runner transaction '$($journal.transaction_id)'."
}

Invoke-KiminolaInterruptedHardwareRecovery
