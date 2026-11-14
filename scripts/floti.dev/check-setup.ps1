# The release workflow's check of what `floti setup` left on a Windows runner
# after floti was installed in this user's own folder, by install.ps1 or by
# the desktop installer, run there and nowhere else. It reads the computer
# and changes nothing. The same checks after either way in are what "runs the
# same setup" means: the same floti, on the same PATH, and nothing that
# starts the host at sign-in, since on Windows a client starts it when it
# needs one.
#
#   powershell -File check-setup.ps1 -Version <version> -By <what installed it>
#
# floti must be at %LOCALAPPDATA%\Programs\Floti\bin\floti.exe and say
# <Version>; that folder must be on the user's Path; there must be no logon
# task "Floti host", which an earlier floti registered and which opened a
# console window at every sign-in; and no floti host may be running from
# that folder, since setup starts none. Every check runs, and each one that
# fails is one line naming what is wrong.
param(
  [Parameter(Mandatory)] [string]$Version,
  [Parameter(Mandatory)] [string]$By
)
$ErrorActionPreference = 'Stop'

$failures = New-Object System.Collections.Generic.List[string]
function Wrong([string]$Why) { $failures.Add($Why) }

function Get-UserPath {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  try { [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') } finally { $key.Close() }
}

$folder = Join-Path $env:LOCALAPPDATA 'Programs\Floti\bin'
$floti = Join-Path $folder 'floti.exe'

$said = ''
if (-not (Test-Path -LiteralPath $floti -PathType Leaf)) {
  Wrong "$floti is not there."
} else {
  $said = (& $floti version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { Wrong "$floti version did not answer (exit $LASTEXITCODE)." }
  elseif ($said -notlike "floti $Version *") { Wrong "$floti version said `"$said`", not floti $Version." }
}

$onPath = [Environment]::ExpandEnvironmentVariables((Get-UserPath)) -split ';' | ForEach-Object { $_.Trim().TrimEnd('\') }
if ($onPath -notcontains $folder) { Wrong "$folder is not on the user's Path." }

if (Get-ScheduledTask -TaskName 'Floti host' -ErrorAction SilentlyContinue) {
  Wrong 'the logon task "Floti host" is registered; on Windows nothing starts the host at sign-in.'
}

# The host renamed aside by an install keeps running as floti.old-*.exe in
# the same folder, so it is looked for by folder and command line, not by
# name.
$hosts = @(Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and (Split-Path -Parent $_.ExecutablePath) -eq $folder -and $_.CommandLine -match '\shost(\s|$)'
  })
foreach ($running in $hosts) {
  Wrong "$($running.ExecutablePath) runs as a host ($($running.CommandLine)); setup starts none on Windows."
}

if ($failures.Count -gt 0) {
  foreach ($why in $failures) { Write-Output "::error::${By}: $why" }
  exit 1
}

Write-Output "${By}: $said answers from $floti, its folder is on the user's Path, and nothing starts the host at sign-in or runs it yet."
Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY "- ${By}: ``$said`` answers from ``$floti``, its folder is on the user's Path, no logon task ``Floti host`` is registered, and no host runs until a client starts one"
