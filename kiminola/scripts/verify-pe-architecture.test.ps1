$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$verifier = Join-Path $scriptDir 'verify-pe-architecture.ps1'
$fixtureRoot = Join-Path $env:TEMP ("kiminola-pe-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureRoot -Force | Out-Null

function New-PeFixture([string]$Path, [uint16]$Machine) {
    $bytes = New-Object byte[] 256
    $bytes[0] = [byte][char]'M'
    $bytes[1] = [byte][char]'Z'
    [BitConverter]::GetBytes([int32]128).CopyTo($bytes, 0x3c)
    $bytes[128] = [byte][char]'P'
    $bytes[129] = [byte][char]'E'
    [BitConverter]::GetBytes($Machine).CopyTo($bytes, 132)
    [IO.File]::WriteAllBytes($Path, $bytes)
}

try {
    $x64 = Join-Path $fixtureRoot 'x64.exe'
    $arm64 = Join-Path $fixtureRoot 'arm64.dll'
    $invalid = Join-Path $fixtureRoot 'invalid.dll'
    New-PeFixture -Path $x64 -Machine 0x8664
    New-PeFixture -Path $arm64 -Machine 0xaa64
    [IO.File]::WriteAllText($invalid, 'not a PE file')

    & $verifier -Target 'x86_64-pc-windows-msvc' -Paths @($x64)

    & $verifier -Target 'aarch64-pc-windows-msvc' -Paths @($arm64)

    $mismatchFailed = $false
    try { & $verifier -Target 'aarch64-pc-windows-msvc' -Paths @($x64) } catch { $mismatchFailed = $true }
    if (-not $mismatchFailed) { throw 'Architecture mismatch should fail.' }

    $invalidFailed = $false
    try { & $verifier -Target 'x86_64-pc-windows-msvc' -Paths @($invalid) } catch { $invalidFailed = $true }
    if (-not $invalidFailed) { throw 'Invalid PE input should fail.' }

    Write-Host 'All PE architecture verifier checks passed.'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}
