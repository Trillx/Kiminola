[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [ValidateRange(10, 900)]
    [int]$DurationSeconds = 180
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$sampleRate = 16000
$sampleCount = $sampleRate
$dataBytes = $sampleCount * 2
$stream = [IO.File]::Open($OutputPath, [IO.FileMode]::Create, [IO.FileAccess]::Write)
$writer = [IO.BinaryWriter]::new($stream)
try {
    $writer.Write([Text.Encoding]::ASCII.GetBytes('RIFF'))
    $writer.Write([int32](36 + $dataBytes))
    $writer.Write([Text.Encoding]::ASCII.GetBytes('WAVE'))
    $writer.Write([Text.Encoding]::ASCII.GetBytes('fmt '))
    $writer.Write([int32]16)
    $writer.Write([int16]1)
    $writer.Write([int16]1)
    $writer.Write([int32]$sampleRate)
    $writer.Write([int32]($sampleRate * 2))
    $writer.Write([int16]2)
    $writer.Write([int16]16)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('data'))
    $writer.Write([int32]$dataBytes)
    for ($sample = 0; $sample -lt $sampleCount; $sample++) {
        $value = [int16](12000 * [Math]::Sin(2 * [Math]::PI * 440 * $sample / $sampleRate))
        $writer.Write($value)
    }
} finally {
    $writer.Dispose()
    $stream.Dispose()
}

$player = [Media.SoundPlayer]::new($OutputPath)
$player.Load()
$player.PlayLooping()
try {
    Start-Sleep -Seconds $DurationSeconds
} finally {
    $player.Stop()
    $player.Dispose()
}
