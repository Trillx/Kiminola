param([string]$WavPath)
# Writes its own PID so the spike can target this process tree for
# process-loopback capture, then plays the WAV in a loop for 60 s.
$PID | Out-File -FilePath "$PSScriptRoot\ps-pid.txt" -Encoding ascii
$player = New-Object System.Media.SoundPlayer $WavPath
$player.PlayLooping()
Start-Sleep -Seconds 60
