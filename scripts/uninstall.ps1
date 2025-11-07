$ErrorActionPreference = "Stop"
$TaskName = "GlpiNotifier"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\GlpiNotifier"

try { Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue | Out-Null } catch {}
try { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue | Out-Null } catch {}

Get-Process glpi-notifier-rs -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "GlpiNotifier uninstalled."
