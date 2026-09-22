[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'hardware-runner-state.ps1')

$fixtureRoot = Join-Path $env:TEMP ("kiminola-hardware-state-test-" + [guid]::NewGuid().ToString('N'))
$registryPath = "Registry::HKEY_CURRENT_USER\Software\KiminolaHardwareStateTest-$([guid]::NewGuid().ToString('N'))"
$interruptedRegistryPath = "$registryPath-Interrupted"

try {
    New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

    $originalRoot = Join-Path $fixtureRoot 'existing-root'
    $originalBackup = Join-Path $fixtureRoot 'existing-backup'
    New-Item -ItemType Directory -Path $originalRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $originalRoot 'state.txt'), 'original')
    $existingState = New-HardwarePathState -Root $originalRoot -Backup $originalBackup -PathType Container
    Backup-HardwarePath -State $existingState
    New-Item -ItemType Directory -Path $originalRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $originalRoot 'state.txt'), 'candidate')
    Restore-HardwarePath -State $existingState
    if ((Get-Content -Raw -LiteralPath (Join-Path $originalRoot 'state.txt')) -ne 'original') {
        throw 'A complete directory snapshot did not replace candidate state with the original state.'
    }

    $absentRoot = Join-Path $fixtureRoot 'initially-absent-root'
    $absentState = New-HardwarePathState -Root $absentRoot -Backup (Join-Path $fixtureRoot 'absent-backup') -PathType Container
    Backup-HardwarePath -State $absentState
    New-Item -ItemType Directory -Path $absentRoot -Force | Out-Null
    Restore-HardwarePath -State $absentState
    if (Test-Path -LiteralPath $absentRoot) {
        throw 'State created under an initially absent directory was not removed.'
    }

    $failedRoot = Join-Path $fixtureRoot 'failed-root'
    $failedBackup = Join-Path $fixtureRoot 'failed-backup'
    New-Item -ItemType Directory -Path $failedRoot, $failedBackup -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $failedRoot 'state.txt'), 'must-survive')
    [IO.File]::WriteAllText((Join-Path $failedBackup 'sentinel.txt'), 'unrelated-backup')
    $failedState = New-HardwarePathState -Root $failedRoot -Backup $failedBackup -PathType Container
    $backupRejected = $false
    try { Backup-HardwarePath -State $failedState } catch { $backupRejected = $true }
    if (-not $backupRejected) { throw 'A conflicting backup path was not rejected.' }
    $restoreRejected = $false
    try { Restore-HardwarePath -State $failedState } catch { $restoreRejected = $true }
    if (-not $restoreRejected -or
        (Get-Content -Raw -LiteralPath (Join-Path $failedRoot 'state.txt')) -ne 'must-survive' -or
        (Get-Content -Raw -LiteralPath (Join-Path $failedBackup 'sentinel.txt')) -ne 'unrelated-backup') {
        throw 'An incomplete snapshot did not preserve both paths for manual recovery.'
    }

    $lockedRoot = Join-Path $fixtureRoot 'cleanup-failure-root'
    $lockedBackup = Join-Path $fixtureRoot 'cleanup-failure-backup'
    New-Item -ItemType Directory -Path $lockedRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $lockedRoot 'state.txt'), 'original')
    $lockedState = New-HardwarePathState -Root $lockedRoot -Backup $lockedBackup -PathType Container
    Backup-HardwarePath -State $lockedState
    New-Item -ItemType Directory -Path $lockedRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $lockedRoot 'state.txt'), 'candidate')
    $cleanupRejected = $false
    try {
        Restore-HardwarePath -State $lockedState -RemoveAction { param($Path) throw "simulated lock: $Path" }
    } catch {
        $cleanupRejected = $true
    }
    if (-not $cleanupRejected -or
        (Get-Content -Raw -LiteralPath (Join-Path $lockedRoot 'state.txt')) -ne 'candidate' -or
        (Get-Content -Raw -LiteralPath (Join-Path $lockedBackup 'state.txt')) -ne 'original') {
        throw 'Cleanup failure did not leave the candidate and backup unnested for recovery.'
    }

    $shortcut = Join-Path $fixtureRoot 'Kimi Nola.lnk'
    $shortcutBackup = Join-Path $fixtureRoot 'shortcut.backup'
    [IO.File]::WriteAllText($shortcut, 'original-shortcut')
    $shortcutState = New-HardwarePathState -Root $shortcut -Backup $shortcutBackup -PathType Leaf
    Backup-HardwarePath -State $shortcutState
    [IO.File]::WriteAllText($shortcut, 'candidate-shortcut')
    Restore-HardwarePath -State $shortcutState
    if ((Get-Content -Raw -LiteralPath $shortcut) -ne 'original-shortcut') {
        throw 'Shortcut file state was not restored.'
    }

    $planBackup = Join-Path $fixtureRoot 'plan-conflict'
    New-Item -ItemType Directory -Path $planBackup -Force | Out-Null
    $planState = New-HardwarePathState `
        -Root $originalRoot `
        -Backup $planBackup `
        -PathType Container
    $planRejected = $false
    try { Test-HardwareStatePlan -PathStates @($planState) -RegistryStates @() } catch { $planRejected = $true }
    if (-not $planRejected) {
        throw 'The complete hardware-state plan did not reject a conflicting later backup before mutation.'
    }

    $journalRoot = Join-Path $fixtureRoot 'durable-journal'
    $journalPath = Join-Path $journalRoot 'active-transaction.json'
    $journalTransaction = '0123456789abcdef0123456789abcdef'
    $journalOriginal = Join-Path $fixtureRoot 'journal-original'
    $journalBackup = Join-Path $journalRoot "$journalTransaction\original"
    New-Item -ItemType Directory -Path $journalOriginal, (Split-Path -Parent $journalBackup) -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $journalOriginal 'state.txt'), 'journal-original')
    $journalState = New-HardwarePathState -Root $journalOriginal -Backup $journalBackup -PathType Container
    Write-HardwareRecoveryJournal `
        -JournalPath $journalPath `
        -TransactionId $journalTransaction `
        -PathStates @($journalState) `
        -RegistryStates @()
    Write-HardwareRecoveryJournal `
        -JournalPath $journalPath `
        -TransactionId $journalTransaction `
        -PathStates @($journalState) `
        -RegistryStates @()
    Move-Item -LiteralPath $journalOriginal -Destination $journalBackup

    $resumedState = New-HardwarePathState -Root $journalOriginal -Backup $journalBackup -PathType Container
    $journal = Read-HardwareRecoveryJournal -JournalPath $journalPath
    Apply-HardwareRecoveryJournal -Journal $journal -PathStates @($resumedState) -RegistryStates @()
    if (-not $resumedState.SnapshotComplete) {
        throw 'Recovery did not infer a completed atomic move interrupted before the journal rewrite.'
    }
    Restore-HardwarePath -State $resumedState
    Restore-HardwarePath -State $resumedState
    if ((Get-Content -Raw -LiteralPath (Join-Path $journalOriginal 'state.txt')) -ne 'journal-original') {
        throw 'Durable journal recovery did not restore and preserve the original path.'
    }
    if (@(Get-ChildItem -LiteralPath $journalRoot -Filter '*.tmp' -File).Count -ne 0) {
        throw 'Atomic journal writes left a temporary file behind.'
    }
    if (Test-Path -LiteralPath "$journalPath.previous") {
        throw 'A verified atomic journal rewrite left its previous generation behind.'
    }

    $wrongDirectory = Join-Path $fixtureRoot 'wrong-directory-type'
    [IO.File]::WriteAllText($wrongDirectory, 'must-survive')
    $wrongDirectoryRejected = $false
    try {
        New-HardwarePathState `
            -Root $wrongDirectory `
            -Backup (Join-Path $fixtureRoot 'wrong-directory-backup') `
            -PathType Container | Out-Null
    } catch {
        $wrongDirectoryRejected = $true
    }
    if (-not $wrongDirectoryRejected -or (Get-Content -Raw -LiteralPath $wrongDirectory) -ne 'must-survive') {
        throw 'A file at a required directory path was not rejected and preserved.'
    }

    $wrongShortcut = Join-Path $fixtureRoot 'wrong-shortcut-type.lnk'
    New-Item -ItemType Directory -Path $wrongShortcut -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $wrongShortcut 'state.txt'), 'must-survive')
    $wrongShortcutRejected = $false
    try {
        New-HardwarePathState `
            -Root $wrongShortcut `
            -Backup (Join-Path $fixtureRoot 'wrong-shortcut-backup') `
            -PathType Leaf | Out-Null
    } catch {
        $wrongShortcutRejected = $true
    }
    if (-not $wrongShortcutRejected -or
        (Get-Content -Raw -LiteralPath (Join-Path $wrongShortcut 'state.txt')) -ne 'must-survive') {
        throw 'A directory at a required shortcut path was not rejected and preserved.'
    }

    New-Item -Path $registryPath -Force | Out-Null
    New-ItemProperty -Path $registryPath -Name DisplayVersion -Value 'original' -PropertyType String -Force | Out-Null
    $registryState = New-HardwareRegistryState -RegistryPath $registryPath -BackupFile (Join-Path $fixtureRoot 'registry.reg')
    $registryJournalPath = Join-Path $fixtureRoot 'registry-active-transaction.json'
    Write-HardwareRecoveryJournal `
        -JournalPath $registryJournalPath `
        -TransactionId $journalTransaction `
        -PathStates @() `
        -RegistryStates @($registryState)
    Export-HardwareRegistryKey -State $registryState
    Write-HardwareRecoveryJournal `
        -JournalPath $registryJournalPath `
        -TransactionId $journalTransaction `
        -PathStates @() `
        -RegistryStates @($registryState)
    Start-HardwareRegistryRemoval -State $registryState
    Write-HardwareRecoveryJournal `
        -JournalPath $registryJournalPath `
        -TransactionId $journalTransaction `
        -PathStates @() `
        -RegistryStates @($registryState)
    Remove-ExportedHardwareRegistryKey -State $registryState
    if (Test-Path -LiteralPath $registryPath) {
        throw 'The original installer registry key remained active after its verified snapshot.'
    }
    New-Item -Path $registryPath -Force | Out-Null
    Set-ItemProperty -Path $registryPath -Name DisplayVersion -Value 'candidate'
    Restore-HardwareRegistryKey -State $registryState
    if ((Get-ItemPropertyValue -Path $registryPath -Name DisplayVersion) -ne 'original') {
        throw 'Installer registry state was not restored.'
    }

    New-Item -Path $interruptedRegistryPath -Force | Out-Null
    New-ItemProperty -Path $interruptedRegistryPath -Name DisplayVersion -Value 'original' -PropertyType String -Force | Out-Null
    New-Item -Path "$interruptedRegistryPath\Child" -Force | Out-Null
    New-ItemProperty -Path "$interruptedRegistryPath\Child" -Name Marker -Value 'must-survive' -PropertyType String -Force | Out-Null
    $interruptedBackup = Join-Path $fixtureRoot 'interrupted-registry.reg'
    $interruptedState = New-HardwareRegistryState `
        -RegistryPath $interruptedRegistryPath `
        -BackupFile $interruptedBackup
    Export-HardwareRegistryKey -State $interruptedState
    Write-HardwareRecoveryJournal `
        -JournalPath $registryJournalPath `
        -TransactionId $journalTransaction `
        -PathStates @() `
        -RegistryStates @($interruptedState)
    Start-HardwareRegistryRemoval -State $interruptedState
    Write-HardwareRecoveryJournal `
        -JournalPath $registryJournalPath `
        -TransactionId $journalTransaction `
        -PathStates @() `
        -RegistryStates @($interruptedState)
    Remove-Item -LiteralPath "$interruptedRegistryPath\Child" -Recurse -Force

    $resumedRegistryState = New-HardwareRegistryState `
        -RegistryPath $interruptedRegistryPath `
        -BackupFile $interruptedBackup
    $registryJournal = Read-HardwareRecoveryJournal -JournalPath $registryJournalPath
    Apply-HardwareRecoveryJournal `
        -Journal $registryJournal `
        -PathStates @() `
        -RegistryStates @($resumedRegistryState)
    Restore-HardwareRegistryKey -State $resumedRegistryState
    if (-not $resumedRegistryState.ExportComplete -or -not $resumedRegistryState.RemovalStarted -or
        $resumedRegistryState.LiveKeyRemoved -or
        (Get-ItemPropertyValue -Path "$interruptedRegistryPath\Child" -Name Marker) -ne 'must-survive') {
        throw 'Recovery did not replace a partially deleted live key from the durably recorded export.'
    }

    $absentRegistryPath = "$registryPath-Absent"
    $absentRegistryState = New-HardwareRegistryState `
        -RegistryPath $absentRegistryPath `
        -BackupFile (Join-Path $fixtureRoot 'absent-registry.reg')
    Export-HardwareRegistryKey -State $absentRegistryState
    Start-HardwareRegistryRemoval -State $absentRegistryState
    Remove-ExportedHardwareRegistryKey -State $absentRegistryState
    New-Item -Path $absentRegistryPath -Force | Out-Null
    Restore-HardwareRegistryKey -State $absentRegistryState
    if (Test-Path -LiteralPath $absentRegistryPath) {
        throw 'Candidate registry state created under an initially absent key was not removed.'
    }

    Write-Host 'PASS: hardware runner state restores complete snapshots and preserves recoverable state on injected failures.'
} finally {
    Remove-Item -LiteralPath $registryPath -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $interruptedRegistryPath -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
