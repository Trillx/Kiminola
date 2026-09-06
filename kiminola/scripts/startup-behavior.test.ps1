[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Executable,

    [ValidateRange(5, 120)]
    [int]$TimeoutSeconds = 20,

    [switch]$RequireForeground
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:ExecutablePath = (Resolve-Path -LiteralPath $Executable).Path
$script:ProcessName = [IO.Path]::GetFileNameWithoutExtension($script:ExecutablePath)
$script:OwnedProcessIds = [Collections.Generic.List[int]]::new()
$script:ReadyFile = Join-Path ([IO.Path]::GetTempPath()) ("kiminola-startup-ready-{0}-{1}.txt" -f $PID, [Guid]::NewGuid().ToString('N'))
$env:KIMINOLA_STARTUP_TEST_READY_FILE = $script:ReadyFile

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace KiminolaStartupHarness
{
    public sealed class WindowInfo
    {
        public IntPtr Handle { get; set; }
        public bool Visible { get; set; }
        public string Title { get; set; }
    }

    public static class NativeWindows
    {
        private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

        [DllImport("user32.dll")]
        private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

        [DllImport("user32.dll")]
        private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

        [DllImport("user32.dll")]
        private static extern bool IsWindowVisible(IntPtr window);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetWindowTextLengthW(IntPtr window);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int capacity);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool PostMessageW(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

        [DllImport("user32.dll")]
        private static extern IntPtr GetForegroundWindow();

        public static WindowInfo[] ForProcess(uint expectedProcessId)
        {
            var windows = new List<WindowInfo>();
            EnumWindows(delegate(IntPtr window, IntPtr parameter)
            {
                uint processId;
                GetWindowThreadProcessId(window, out processId);
                if (processId == expectedProcessId)
                {
                    int length = GetWindowTextLengthW(window);
                    var title = new StringBuilder(length + 1);
                    GetWindowTextW(window, title, title.Capacity);
                    windows.Add(new WindowInfo
                    {
                        Handle = window,
                        Visible = IsWindowVisible(window),
                        Title = title.ToString()
                    });
                }
                return true;
            }, IntPtr.Zero);
            return windows.ToArray();
        }

        public static void RequestClose(IntPtr window)
        {
            const uint CloseMessage = 0x0010;
            if (!PostMessageW(window, CloseMessage, UIntPtr.Zero, IntPtr.Zero))
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not post WM_CLOSE to the Kimi Nola main window.");
            }
        }

        public static bool IsVisible(IntPtr window)
        {
            return IsWindowVisible(window);
        }

        public static IntPtr Foreground()
        {
            return GetForegroundWindow();
        }
    }
}
'@

function Wait-Until {
    param(
        [Parameter(Mandatory = $true)][string]$Description,
        [Parameter(Mandatory = $true)][scriptblock]$Condition
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $value = & $Condition
        if ($null -ne $value -and $value -ne $false) {
            return $value
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)

    throw "Timed out after $TimeoutSeconds seconds waiting for $Description."
}

function Get-MainWindow {
    param([Parameter(Mandatory = $true)][int]$ProcessId)

    return @([KiminolaStartupHarness.NativeWindows]::ForProcess([uint32]$ProcessId)) |
        Where-Object { $_.Title -eq 'Kimi Nola' } |
        Select-Object -First 1
}

function Get-KiminolaProcesses {
    return @(Get-Process -Name $script:ProcessName -ErrorAction SilentlyContinue)
}

function Start-OwnedApp {
    param([string[]]$Arguments = @())

    $parameters = @{
        FilePath = $script:ExecutablePath
        PassThru = $true
    }
    if (@($Arguments).Count -gt 0) {
        $parameters.ArgumentList = $Arguments
    }
    $process = Start-Process @parameters
    $script:OwnedProcessIds.Add($process.Id)
    return $process
}

function Wait-SecondaryExit {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)

    if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
        throw "Secondary process $($Process.Id) did not exit within $TimeoutSeconds seconds."
    }
    if ($Process.ExitCode -ne 0) {
        throw "Secondary process $($Process.Id) exited with code $($Process.ExitCode)."
    }
}

function Wait-PrimaryReady {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)

    Wait-Until -Description "primary process $($Process.Id) to complete Tauri setup" -Condition {
        if ($Process.HasExited) {
            throw "Primary process $($Process.Id) exited before completing Tauri setup."
        }
        if (-not (Test-Path -LiteralPath $script:ReadyFile -PathType Leaf)) {
            return $false
        }
        try {
            return [IO.File]::ReadAllText($script:ReadyFile).Trim() -eq [string]$Process.Id
        }
        catch [IO.IOException] {
            return $false
        }
    } | Out-Null
}

function Assert-SinglePrimary {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Primary)

    if ($Primary.HasExited) {
        throw "Primary process $($Primary.Id) exited unexpectedly."
    }
    $processes = @(Get-KiminolaProcesses)
    if ($processes.Count -ne 1 -or $processes[0].Id -ne $Primary.Id) {
        $ids = ($processes | ForEach-Object Id) -join ', '
        throw "Expected only primary PID $($Primary.Id), found Kimi Nola PIDs: $ids"
    }
}

function Stop-OwnedPrimary {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Primary)

    if (-not $Primary.HasExited) {
        Stop-Process -Id $Primary.Id -Force
        if (-not $Primary.WaitForExit($TimeoutSeconds * 1000)) {
            throw "Primary process $($Primary.Id) did not stop during scenario cleanup."
        }
    }
    Wait-Until -Description 'all Kimi Nola processes to stop' -Condition {
        @(Get-KiminolaProcesses).Count -eq 0
    } | Out-Null
}

$existing = @(Get-KiminolaProcesses)
if ($existing.Count -gt 0) {
    $ids = ($existing | ForEach-Object Id) -join ', '
    throw "Refusing to run while existing '$($script:ProcessName)' processes are active: $ids"
}

try {
    Write-Host 'Scenario 1: ordinary primary -> close-to-background -> ordinary relaunch'
    Remove-Item -LiteralPath $script:ReadyFile -Force -ErrorAction SilentlyContinue
    $ordinaryPrimary = Start-OwnedApp
    Wait-PrimaryReady -Process $ordinaryPrimary
    $ordinaryWindow = Wait-Until -Description 'ordinary primary main window to become visible' -Condition {
        $candidate = Get-MainWindow -ProcessId $ordinaryPrimary.Id
        if ($null -ne $candidate -and $candidate.Visible) { $candidate }
    }

    [KiminolaStartupHarness.NativeWindows]::RequestClose($ordinaryWindow.Handle)
    $ordinaryWindow = Wait-Until -Description 'close request to hide, but not destroy, the primary window' -Condition {
        $candidate = Get-MainWindow -ProcessId $ordinaryPrimary.Id
        if ($null -ne $candidate -and -not $candidate.Visible) { $candidate }
    }

    $ordinarySecondary = Start-OwnedApp
    Wait-SecondaryExit -Process $ordinarySecondary
    $reactivatedWindow = Wait-Until -Description 'ordinary relaunch to reactivate the primary window' -Condition {
        $candidate = Get-MainWindow -ProcessId $ordinaryPrimary.Id
        if ($null -ne $candidate -and $candidate.Visible) { $candidate }
    }
    Assert-SinglePrimary -Primary $ordinaryPrimary

    if ($RequireForeground) {
        Wait-Until -Description 'reactivated primary window to become foreground' -Condition {
            [KiminolaStartupHarness.NativeWindows]::Foreground() -eq $reactivatedWindow.Handle
        } | Out-Null
    }
    Write-Host 'PASS: ordinary relaunch reused one process and reactivated its hidden window.'
    Stop-OwnedPrimary -Primary $ordinaryPrimary

    Write-Host 'Scenario 2: background primary -> background relaunch -> ordinary relaunch'
    Remove-Item -LiteralPath $script:ReadyFile -Force -ErrorAction SilentlyContinue
    $backgroundPrimary = Start-OwnedApp -Arguments @('--background')
    Wait-PrimaryReady -Process $backgroundPrimary
    $backgroundWindow = Wait-Until -Description 'background primary main HWND to exist and remain hidden' -Condition {
        $candidate = Get-MainWindow -ProcessId $backgroundPrimary.Id
        if ($null -ne $candidate -and -not $candidate.Visible) { $candidate }
    }

    $backgroundSecondary = Start-OwnedApp -Arguments @('--background')
    Wait-SecondaryExit -Process $backgroundSecondary
    Start-Sleep -Milliseconds 300
    $backgroundWindow = Get-MainWindow -ProcessId $backgroundPrimary.Id
    if ($null -eq $backgroundWindow -or $backgroundWindow.Visible) {
        throw 'A background relaunch surfaced the primary window.'
    }
    Assert-SinglePrimary -Primary $backgroundPrimary

    $laterOrdinarySecondary = Start-OwnedApp
    Wait-SecondaryExit -Process $laterOrdinarySecondary
    $shownAfterOrdinary = Wait-Until -Description 'later ordinary relaunch to show the background primary' -Condition {
        $candidate = Get-MainWindow -ProcessId $backgroundPrimary.Id
        if ($null -ne $candidate -and $candidate.Visible) { $candidate }
    }
    Assert-SinglePrimary -Primary $backgroundPrimary

    if ($RequireForeground) {
        Wait-Until -Description 'background primary activated by ordinary relaunch to become foreground' -Condition {
            [KiminolaStartupHarness.NativeWindows]::Foreground() -eq $shownAfterOrdinary.Handle
        } | Out-Null
    }
    Write-Host 'PASS: background relaunch stayed hidden; later ordinary relaunch showed the same process.'
    Stop-OwnedPrimary -Primary $backgroundPrimary

    Write-Host 'PASS: Kimi Nola startup PID/HWND behavior verified.'
}
finally {
    foreach ($ownedProcessId in $script:OwnedProcessIds) {
        $owned = Get-Process -Id $ownedProcessId -ErrorAction SilentlyContinue
        if ($null -ne $owned) {
            Stop-Process -Id $ownedProcessId -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-Item -LiteralPath $script:ReadyFile -Force -ErrorAction SilentlyContinue
    Remove-Item Env:KIMINOLA_STARTUP_TEST_READY_FILE -ErrorAction SilentlyContinue
}
