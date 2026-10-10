Set-StrictMode -Version Latest

if (-not ('Kiminola.NativeFile' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace Kiminola {
    public static class NativeFile {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool MoveFileEx(string existingFileName, string newFileName, int flags);
    }
}
'@
}

function Get-HardwareNormalizedPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    return [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

function New-HardwarePathState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Backup,
        [Parameter(Mandatory = $true)][ValidateSet('Container', 'Leaf')][string]$PathType
    )

    $exists = Test-Path -LiteralPath $Root
    if ($exists -and -not (Test-Path -LiteralPath $Root -PathType $PathType)) {
        throw "Hardware-state path '$Root' exists but is not the required $PathType type."
    }

    [pscustomobject]@{
        Root = $Root
        Backup = $Backup
        PathType = $PathType
        InitiallyExisted = $exists
        SnapshotComplete = $false
    }
}

function Test-HardwareStatePlan {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$PathStates,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$RegistryStates
    )

    $reservedPaths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($state in $PathStates) {
        $root = Get-HardwareNormalizedPath $state.Root
        $backup = Get-HardwareNormalizedPath $state.Backup
        if (-not $reservedPaths.Add($root) -or -not $reservedPaths.Add($backup)) {
            throw "Hardware-state plan reuses a path: '$root' or '$backup'."
        }
        if (Test-Path -LiteralPath $backup) {
            throw "Refusing to overwrite existing hardware-state backup '$backup'."
        }
        $rootVolume = [IO.Path]::GetPathRoot($root)
        $backupVolume = [IO.Path]::GetPathRoot($backup)
        if (-not [string]::Equals($rootVolume, $backupVolume, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Hardware-state backup '$backup' must be on the same volume as '$root'."
        }
    }
    foreach ($state in $RegistryStates) {
        $backup = Get-HardwareNormalizedPath $state.BackupFile
        if (-not $reservedPaths.Add($backup)) {
            throw "Hardware-state plan reuses registry backup '$backup'."
        }
        if (Test-Path -LiteralPath $backup) {
            throw "Refusing to overwrite existing registry backup '$backup'."
        }
    }
}

function Backup-HardwarePath {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$State)

    if (-not $State.InitiallyExisted) { return }
    if (Test-Path -LiteralPath $State.Backup) {
        throw "Refusing to overwrite existing hardware-state backup '$($State.Backup)'."
    }
    $rootVolume = [IO.Path]::GetPathRoot((Get-HardwareNormalizedPath $State.Root))
    $backupVolume = [IO.Path]::GetPathRoot((Get-HardwareNormalizedPath $State.Backup))
    if (-not [string]::Equals($rootVolume, $backupVolume, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Hardware-state backup '$($State.Backup)' must be on the same volume as '$($State.Root)'."
    }

    try {
        Move-Item -LiteralPath $State.Root -Destination $State.Backup -ErrorAction Stop
    } catch {
        if (-not (Test-Path -LiteralPath $State.Root) -and
            (Test-Path -LiteralPath $State.Backup -PathType $State.PathType)) {
            $State.SnapshotComplete = $true
        }
        throw
    }

    if ((Test-Path -LiteralPath $State.Root) -or
        -not (Test-Path -LiteralPath $State.Backup -PathType $State.PathType)) {
        throw "Hardware-state snapshot did not complete for '$($State.Root)'; both paths are preserved."
    }
    $State.SnapshotComplete = $true
}

function Restore-HardwarePath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$State,
        [scriptblock]$RemoveAction = {
            param($Path)
            Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
        }
    )

    $rootExists = Test-Path -LiteralPath $State.Root
    $rootHasExpectedType = Test-Path -LiteralPath $State.Root -PathType $State.PathType
    $backupExists = Test-Path -LiteralPath $State.Backup
    $backupHasExpectedType = Test-Path -LiteralPath $State.Backup -PathType $State.PathType

    if ($State.InitiallyExisted -and -not $State.SnapshotComplete) {
        if ($rootHasExpectedType -and -not $backupExists) {
            return
        }
        throw "No complete snapshot exists for '$($State.Root)'; preserving the original and backup paths for recovery."
    }
    if ($State.SnapshotComplete -and -not $backupExists) {
        if ($rootHasExpectedType) {
            return
        }
        throw "Completed snapshot is missing from '$($State.Backup)'."
    }
    if ($State.SnapshotComplete -and -not $backupHasExpectedType) {
        throw "Completed snapshot has the wrong type at '$($State.Backup)'."
    }
    if (-not $State.InitiallyExisted -and $backupExists) {
        throw "Unexpected backup exists for initially absent path '$($State.Root)'; preserving both paths for recovery."
    }

    if ($rootExists) {
        & $RemoveAction $State.Root
    }
    if (Test-Path -LiteralPath $State.Root) {
        throw "Could not clear candidate state '$($State.Root)'; backup remains at '$($State.Backup)'."
    }

    if ($State.SnapshotComplete) {
        Move-Item -LiteralPath $State.Backup -Destination $State.Root -ErrorAction Stop
        if (-not (Test-Path -LiteralPath $State.Root -PathType $State.PathType) -or
            (Test-Path -LiteralPath $State.Backup)) {
            throw "Hardware-state restore did not complete for '$($State.Root)'."
        }
    }
}

function ConvertTo-NativeRegistryPath {
    param([Parameter(Mandatory = $true)][string]$RegistryPath)

    if ($RegistryPath.StartsWith('Registry::HKEY_CURRENT_USER\', [StringComparison]::OrdinalIgnoreCase)) {
        return 'HKCU\' + $RegistryPath.Substring('Registry::HKEY_CURRENT_USER\'.Length)
    }
    if ($RegistryPath.StartsWith('Registry::HKEY_LOCAL_MACHINE\', [StringComparison]::OrdinalIgnoreCase)) {
        return 'HKLM\' + $RegistryPath.Substring('Registry::HKEY_LOCAL_MACHINE\'.Length)
    }
    throw "Unsupported registry path '$RegistryPath'."
}

function New-HardwareRegistryState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$BackupFile
    )

    [pscustomobject]@{
        RegistryPath = $RegistryPath
        NativeRegistryPath = ConvertTo-NativeRegistryPath $RegistryPath
        BackupFile = $BackupFile
        InitiallyExisted = Test-Path -LiteralPath $RegistryPath
        ExportComplete = $false
        RemovalStarted = $false
        LiveKeyRemoved = $false
    }
}

function Export-HardwareRegistryKey {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$State)

    if (-not $State.InitiallyExisted) { return }
    if (Test-Path -LiteralPath $State.BackupFile) {
        throw "Refusing to overwrite existing registry backup '$($State.BackupFile)'."
    }

    & reg.exe export $State.NativeRegistryPath $State.BackupFile /y | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $State.BackupFile -PathType Leaf) -or
        (Get-Item -LiteralPath $State.BackupFile).Length -eq 0) {
        throw "Could not export registry key '$($State.NativeRegistryPath)'."
    }
    $exportStream = [IO.FileStream]::new(
        $State.BackupFile,
        [IO.FileMode]::Open,
        [IO.FileAccess]::ReadWrite,
        [IO.FileShare]::Read
    )
    try {
        $exportStream.Flush($true)
    } finally {
        $exportStream.Dispose()
    }
    $State.ExportComplete = $true
}

function Start-HardwareRegistryRemoval {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$State)

    if ($State.InitiallyExisted -and -not $State.ExportComplete) {
        throw "Refusing to start registry removal for '$($State.RegistryPath)' before its export is complete."
    }
    $State.RemovalStarted = $true
}

function Remove-ExportedHardwareRegistryKey {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$State)

    if ($State.InitiallyExisted -and -not $State.ExportComplete) {
        throw "Refusing to remove registry key '$($State.RegistryPath)' before its export is complete."
    }
    if (-not $State.RemovalStarted) {
        throw "Refusing to remove registry key '$($State.RegistryPath)' before the removal phase is durably recorded."
    }
    if (-not (Test-Path -LiteralPath $State.RegistryPath)) {
        $State.LiveKeyRemoved = $true
        return
    }
    Remove-Item -LiteralPath $State.RegistryPath -Recurse -Force -ErrorAction Stop
    if (Test-Path -LiteralPath $State.RegistryPath) {
        throw "Could not clear snapshotted registry key '$($State.RegistryPath)'."
    }
    $State.LiveKeyRemoved = $true
}

function Restore-HardwareRegistryKey {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$State)

    $registryExists = Test-Path -LiteralPath $State.RegistryPath
    $backupExists = Test-Path -LiteralPath $State.BackupFile -PathType Leaf
    if ($State.InitiallyExisted -and -not $State.ExportComplete) {
        if ($registryExists) {
            return
        }
        throw "No complete registry export exists for '$($State.RegistryPath)'; preserving current state and any backup."
    }
    if ($State.ExportComplete -and -not $backupExists) {
        throw "Completed registry export is missing from '$($State.BackupFile)'; preserving current registry state."
    }
    if (-not $State.RemovalStarted) {
        if ($State.InitiallyExisted -and -not $registryExists) {
            throw "Registry key '$($State.RegistryPath)' disappeared before removal was durably started; preserving its export."
        }
        return
    }

    if (Test-Path -LiteralPath $State.RegistryPath) {
        Remove-Item -LiteralPath $State.RegistryPath -Recurse -Force -ErrorAction Stop
    }
    if (Test-Path -LiteralPath $State.RegistryPath) {
        throw "Could not clear candidate registry key '$($State.RegistryPath)'."
    }

    if ($State.ExportComplete) {
        & reg.exe import $State.BackupFile | Out-Null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $State.RegistryPath)) {
            throw "Could not restore registry key '$($State.NativeRegistryPath)' from '$($State.BackupFile)'."
        }
        $verificationFile = "$($State.BackupFile).restored"
        try {
            & reg.exe export $State.NativeRegistryPath $verificationFile /y | Out-Null
            if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $verificationFile -PathType Leaf)) {
                throw "Could not verify restored registry key '$($State.NativeRegistryPath)'."
            }
            $expectedHash = (Get-FileHash -LiteralPath $State.BackupFile -Algorithm SHA256).Hash
            $actualHash = (Get-FileHash -LiteralPath $verificationFile -Algorithm SHA256).Hash
            if ($actualHash -ne $expectedHash) {
                throw "Restored registry key '$($State.NativeRegistryPath)' differs from its snapshot."
            }
        } finally {
            Remove-Item -LiteralPath $verificationFile -Force -ErrorAction SilentlyContinue
        }
    }
}

function Write-DurableHardwareFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][byte[]]$Bytes
    )

    $stream = [IO.FileStream]::new(
        $Path,
        [IO.FileMode]::CreateNew,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None,
        4096,
        [IO.FileOptions]::WriteThrough
    )
    try {
        $stream.Write($Bytes, 0, $Bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Write-HardwareRecoveryJournal {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$JournalPath,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{32}$')][string]$TransactionId,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$PathStates,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$RegistryStates,
        [bool]$RestoreComplete = $false
    )

    $journalDirectory = Split-Path -Parent $JournalPath
    New-Item -ItemType Directory -Path $journalDirectory -Force | Out-Null
    $document = [ordered]@{
        schema_version = 3
        transaction_id = $TransactionId
        restore_complete = $RestoreComplete
        path_states = @($PathStates | ForEach-Object {
            [ordered]@{
                root = Get-HardwareNormalizedPath $_.Root
                backup = Get-HardwareNormalizedPath $_.Backup
                path_type = $_.PathType
                initially_existed = [bool]$_.InitiallyExisted
                snapshot_complete = [bool]$_.SnapshotComplete
            }
        })
        registry_states = @($RegistryStates | ForEach-Object {
            [ordered]@{
                registry_path = $_.RegistryPath
                backup_file = Get-HardwareNormalizedPath $_.BackupFile
                initially_existed = [bool]$_.InitiallyExisted
                export_complete = [bool]$_.ExportComplete
                removal_started = [bool]$_.RemovalStarted
                live_key_removed = [bool]$_.LiveKeyRemoved
            }
        })
    }
    $temporaryPath = "$JournalPath.$([guid]::NewGuid().ToString('N')).tmp"
    $replacementBackup = "$JournalPath.previous"
    try {
        $journalBytes = [Text.UTF8Encoding]::new($false).GetBytes(($document | ConvertTo-Json -Depth 6))
        Write-DurableHardwareFile -Path $temporaryPath -Bytes $journalBytes
        if (Test-Path -LiteralPath $JournalPath -PathType Leaf) {
            Remove-Item -LiteralPath $replacementBackup -Force -ErrorAction SilentlyContinue
            Write-DurableHardwareFile -Path $replacementBackup -Bytes ([IO.File]::ReadAllBytes($JournalPath))
        }
        $moveReplaceExisting = 0x1
        $moveWriteThrough = 0x8
        if (-not [Kiminola.NativeFile]::MoveFileEx($temporaryPath, $JournalPath, ($moveReplaceExisting -bor $moveWriteThrough))) {
            throw "Could not atomically publish hardware recovery journal '$JournalPath' (Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error()))."
        }
        $verified = Get-Content -Raw -LiteralPath $JournalPath | ConvertFrom-Json
        if ($verified.transaction_id -ne $TransactionId -or $verified.schema_version -ne 3) {
            throw "Published hardware recovery journal '$JournalPath' failed readback verification."
        }
        if (Test-Path -LiteralPath $replacementBackup -PathType Leaf) {
            Remove-Item -LiteralPath $replacementBackup -Force -ErrorAction Stop
        }
    } finally {
        Remove-Item -LiteralPath $temporaryPath -Force -ErrorAction SilentlyContinue
    }
}

function Read-HardwareRecoveryJournal {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$JournalPath)

    if (-not (Test-Path -LiteralPath $JournalPath -PathType Leaf)) {
        throw "Hardware recovery journal does not exist: '$JournalPath'."
    }
    try {
        $journal = Get-Content -Raw -LiteralPath $JournalPath | ConvertFrom-Json
    } catch {
        $previousPath = "$JournalPath.previous"
        if (-not (Test-Path -LiteralPath $previousPath -PathType Leaf)) { throw }
        $journal = Get-Content -Raw -LiteralPath $previousPath | ConvertFrom-Json
    }
    if ($journal.schema_version -notin @(2, 3) -or $journal.transaction_id -notmatch '^[0-9a-f]{32}$') {
        throw "Hardware recovery journal '$JournalPath' is invalid or unsupported."
    }
    return $journal
}

function Apply-HardwareRecoveryJournal {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Journal,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$PathStates,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$RegistryStates
    )

    $pathRecords = @($Journal.path_states | Where-Object { $null -ne $_ })
    $registryRecords = @($Journal.registry_states | Where-Object { $null -ne $_ })
    if ($pathRecords.Count -ne $PathStates.Count -or $registryRecords.Count -ne $RegistryStates.Count) {
        throw 'Hardware recovery journal state count does not match the expected runner state.'
    }

    foreach ($state in $PathStates) {
        $root = Get-HardwareNormalizedPath $state.Root
        $backup = Get-HardwareNormalizedPath $state.Backup
        $matches = @($pathRecords | Where-Object {
            [string]::Equals($_.root, $root, [StringComparison]::OrdinalIgnoreCase) -and
            [string]::Equals($_.backup, $backup, [StringComparison]::OrdinalIgnoreCase) -and
            $_.path_type -eq $state.PathType
        })
        if ($matches.Count -ne 1) {
            throw "Hardware recovery journal does not uniquely match path '$($state.Root)'."
        }
        $state.InitiallyExisted = [bool]$matches[0].initially_existed
        $state.SnapshotComplete = [bool]$matches[0].snapshot_complete
        if ($state.InitiallyExisted -and -not $state.SnapshotComplete -and
            -not (Test-Path -LiteralPath $state.Root) -and
            (Test-Path -LiteralPath $state.Backup -PathType $state.PathType)) {
            $state.SnapshotComplete = $true
        }
    }

    foreach ($state in $RegistryStates) {
        $backup = Get-HardwareNormalizedPath $state.BackupFile
        $matches = @($registryRecords | Where-Object {
            [string]::Equals($_.registry_path, $state.RegistryPath, [StringComparison]::OrdinalIgnoreCase) -and
            [string]::Equals($_.backup_file, $backup, [StringComparison]::OrdinalIgnoreCase)
        })
        if ($matches.Count -ne 1) {
            throw "Hardware recovery journal does not uniquely match registry '$($state.RegistryPath)'."
        }
        $state.InitiallyExisted = [bool]$matches[0].initially_existed
        $state.ExportComplete = [bool]$matches[0].export_complete
        $removalStartedProperty = $matches[0].PSObject.Properties['removal_started']
        $state.RemovalStarted = if ($null -ne $removalStartedProperty) {
            [bool]$removalStartedProperty.Value
        } else {
            [bool]$matches[0].live_key_removed
        }
        $state.LiveKeyRemoved = [bool]$matches[0].live_key_removed
    }
}
