$ErrorActionPreference = "Stop"

# Per-user install dir
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\GlpiNotifier"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# Source = repo root (this script is under scripts\)
$Src  = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Src

# Copy binaries/assets if present in root
$exeSrc   = Join-Path $Root "target\release\glpi-notifier-rs.exe"
$snoreSrc = Join-Path $Root "snoretoast.exe"
$logoSrc  = Join-Path $Root "assets\logo.png"
$healthSrc = Join-Path $Src "health.ps1"   # <-- copy if present

if (-not (Test-Path $exeSrc)) {
  Write-Host "NOTE: Build first (cargo build --release). Copying scripts/assets only."
} else {
  Copy-Item -Path $exeSrc -Destination $InstallDir -Force
}

if (Test-Path $snoreSrc)  { Copy-Item -Path $snoreSrc -Destination $InstallDir -Force }
if (Test-Path $logoSrc)   { New-Item -ItemType Directory -Force -Path (Join-Path $InstallDir "assets") | Out-Null
                             Copy-Item -Path $logoSrc -Destination (Join-Path $InstallDir "assets") -Force }

# .env – create from template if missing
$EnvDest = Join-Path $InstallDir ".env"
$EnvTemplate = Join-Path $Root ".env.template"
if (-not (Test-Path $EnvDest)) {
  if (Test-Path $EnvTemplate) {
    Copy-Item $EnvTemplate $EnvDest -Force
  } else {
@"
GLPI_BASE_URL=https://your-domain/apirest.php
GLPI_APP_TOKEN=
GLPI_USER_TOKEN=
POLL_SECONDS=60
VERIFY_SSL=true
FIRST_RUN_NOTIFY=true
DEBUG_LIST=true
GLPI_TICKET_URL_TEMPLATE=https://your-glpi/front/ticket.form.php?id={id}
# GLPI_LOGO_PATH=C:\Users\...\logo.png
"@ | Out-File -FilePath $EnvDest -Encoding UTF8 -Force
  }
}

# Launcher CMD with file logging
$Launcher = Join-Path $InstallDir "Run-GlpiNotifier.cmd"
@"
@echo off
cd /d "%~dp0"
set "RUST_LOG=info"
set "LOG=%LOCALAPPDATA%\Programs\GlpiNotifier\glpi-notifier.log"
"%~dp0glpi-notifier-rs.exe" >> "%LOG%" 2>&1
"@ | Out-File -FilePath $Launcher -Encoding ASCII -Force

# Health script: prefer copying from repo; if missing, generate fallback
$Health = Join-Path $InstallDir "health.ps1"
if (Test-Path $healthSrc) {
  Copy-Item -Path $healthSrc -Destination $Health -Force
} else {
@"
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
"@ | Out-File -FilePath $Health -Encoding UTF8 -Force
}

# Register/refresh Scheduled Task (At logon)
$TaskName = "GlpiNotifier"
try {
  Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
} catch {}

$Action  = New-ScheduledTaskAction -Execute $Launcher
$Trigger = New-ScheduledTaskTrigger -AtLogOn
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
  -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Settings $Settings `
  -Description "GLPI notifier with Windows toasts (user-mode, Scheduled Task)" -User $env:USERNAME | Out-Null

# Start now and fire a test toast
Start-ScheduledTask -TaskName $TaskName
Start-Process -FilePath (Join-Path $InstallDir "glpi-notifier-rs.exe") -ArgumentList "--test-toast"

Write-Host "Installed to $InstallDir. Scheduled Task 'GlpiNotifier' registered."