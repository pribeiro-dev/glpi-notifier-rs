param(
  [switch]$Tail,
  [int]$TailLines = 80
)

$ErrorActionPreference = "Stop"

$TaskName = "GlpiNotifier"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\GlpiNotifier"
$LogPath   = Join-Path $InstallDir "glpi-notifier.log"
$HBPath    = Join-Path $env:LOCALAPPDATA "GlpiNotifier\heartbeat.json"

# Scheduled Task status
$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
$info = if ($task) { Get-ScheduledTaskInfo -TaskName $TaskName } else { $null }
$proc = Get-Process glpi-notifier-rs -ErrorAction SilentlyContinue

[pscustomobject]@{
  TaskExists     = [bool]$task
  TaskState      = if($task){ $task.State } else { $null }
  LastRunTime    = if($info){ $info.LastRunTime } else { $null }
  LastTaskResult = if($info){ $info.LastTaskResult } else { $null }
  NextRunTime    = if($info){ $info.NextRunTime } else { $null }
  ProcessRunning = [bool]$proc
} | Format-List

# Log output
Write-Host "`n--- Log ---`n"
if (Test-Path $LogPath) {
  if ($Tail) {
    Get-Content $LogPath -Encoding UTF8 -Wait
  } else {
    Get-Content $LogPath -Encoding UTF8 -Tail $TailLines
  }
} else {
  Write-Host "No log found at $LogPath"
}

# Heartbeat info
Write-Host "`n--- Heartbeat ---`n"
if (Test-Path $HBPath) {
  try {
    $hb = Get-Content $HBPath -Raw | ConvertFrom-Json
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $age = $now - [int64]$hb.ts
    [pscustomobject]@{
      Path       = $HBPath
      Timestamp  = [DateTimeOffset]::FromUnixTimeSeconds([int64]$hb.ts).UtcDateTime
      AgeSeconds = $age
      OK         = [bool]$hb.ok
      LastNew    = [int]$hb.new
      Alive      = ($age -lt 120)
    } | Format-List
  } catch {
    Write-Warning "Failed to parse heartbeat.json: $_"
    Get-Content $HBPath
  }
} else {
  Write-Host "No heartbeat found at $HBPath"
}