# The release workflow's check of what `floti setup` left on a Windows runner
# after floti was installed in this user's own folder, by install.ps1 or by
# the desktop installer, run there and nowhere else. It reads the computer
# and changes nothing. The same checks after either way in are what "runs the
# same setup" means: the same floti, on the same PATH, started by the same
# logon task.
#
#   powershell -File check-setup.ps1 -Version <version> -By <what installed it>
#
# floti must be at %LOCALAPPDATA%\Programs\Floti\bin\floti.exe and say
# <Version>; that folder must be on the user's Path; the logon task
# "Floti host" must run that floti.exe with `host` at this user's sign-in and
# be running; and a floti host must be running from that folder. Every check
# runs, and each one that fails is one line naming what is missing.
param(
  [Parameter(Mandatory)] [string]$Version,
  [Parameter(Mandatory)] [string]$By
)
$ErrorActionPreference = 'Stop'

$failures = New-Object System.Collections.Generic.List[string]
function Missing([string]$Why) { $failures.Add($Why) }

function Get-UserPath {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  try { [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') } finally { $key.Close() }
}

$folder = Join-Path $env:LOCALAPPDATA 'Programs\Floti\bin'
$floti = Join-Path $folder 'floti.exe'

$said = ''
if (-not (Test-Path -LiteralPath $floti -PathType Leaf)) {
  Missing "$floti is not there."
} else {
  $said = (& $floti version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { Missing "$floti version did not answer (exit $LASTEXITCODE)." }
  elseif ($said -notlike "floti $Version *") { Missing "$floti version said `"$said`", not floti $Version." }
}

$onPath = [Environment]::ExpandEnvironmentVariables((Get-UserPath)) -split ';' | ForEach-Object { $_.Trim().TrimEnd('\') }
if ($onPath -notcontains $folder) { Missing "$folder is not on the user's Path." }

$task = Get-ScheduledTask -TaskName 'Floti host' -ErrorAction SilentlyContinue
if (-not $task) {
  Missing 'the logon task "Floti host" is not registered.'
} else {
  $action = @($task.Actions)[0]
  $command = if ($action) { ([string]$action.Execute).Trim('"') } else { '' }
  if ($command -ne $floti) { Missing "the logon task `"Floti host`" runs `"$command`", not $floti." }
  elseif ([string]$action.Arguments -ne 'host') { Missing "the logon task `"Floti host`" runs floti with `"$($action.Arguments)`", not host." }
  $logon = @($task.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
  if ($logon.Count -eq 0) { Missing 'the logon task "Floti host" has no logon trigger.' }
  if ([string]$task.State -ne 'Running') { Missing "the logon task `"Floti host`" is $($task.State), not Running." }
}

# The host renamed aside by an install keeps running as floti.old-*.exe in
# the same folder, so it is found by folder and command line, not by name.
$hosts = @(Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and (Split-Path -Parent $_.ExecutablePath) -eq $folder -and $_.CommandLine -match '\shost\s*$'
  })
if ($hosts.Count -eq 0) { Missing "no floti host is running from $folder." }

if ($failures.Count -gt 0) {
  foreach ($why in $failures) { Write-Output "::error::${By}: $why" }
  exit 1
}

$running = ($hosts | ForEach-Object { Split-Path -Leaf $_.ExecutablePath }) -join ', '
Write-Output "${By}: $said answers from $floti, its folder is on the user's Path, and the logon task `"Floti host`" runs it and is running ($running)."
Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY "- ${By}: ``$said`` answers from ``$floti``, its folder is on the user's Path, and the logon task ``Floti host`` runs it at sign-in and is running (``$running``)"
