[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string[]]$Paths
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$expectedMachine = switch ($Target) {
    'x86_64-pc-windows-msvc' { [uint16]0x8664 }
    'aarch64-pc-windows-msvc' { [uint16]0xaa64 }
}
$expectedName = if ($expectedMachine -eq 0x8664) { 'x64' } else { 'ARM64' }

foreach ($path in $Paths) {
    $resolved = (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
    $bytes = [IO.File]::ReadAllBytes($resolved)
    if ($bytes.Length -lt 64 -or $bytes[0] -ne [byte][char]'M' -or $bytes[1] -ne [byte][char]'Z') {
        throw "'$resolved' is not a valid PE file (missing MZ header)."
    }

    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($peOffset -lt 0 -or $peOffset + 6 -gt $bytes.Length) {
        throw "'$resolved' has an invalid PE header offset."
    }
    if ($bytes[$peOffset] -ne [byte][char]'P' -or
        $bytes[$peOffset + 1] -ne [byte][char]'E' -or
        $bytes[$peOffset + 2] -ne 0 -or
        $bytes[$peOffset + 3] -ne 0) {
        throw "'$resolved' is not a valid PE file (missing PE signature)."
    }

    $actualMachine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
    if ($actualMachine -ne $expectedMachine) {
        throw ("PE architecture mismatch for '{0}': expected {1} (0x{2:X4}), got 0x{3:X4}." -f `
            $resolved, $expectedName, $expectedMachine, $actualMachine)
    }

    Write-Host ("PASS: {0} is {1} (0x{2:X4})." -f $resolved, $expectedName, $actualMachine)
}
