# The release workflow's test of `floti uninstall` on a Windows runner, after
# an install and check-setup.ps1, run there and nowhere else: it removes what
# the install put there.
#
#   powershell -File try-uninstall.ps1 -By <what installed it>
#
# Three runs, each of which must exit 0:
#
# 1. `floti uninstall` from %LOCALAPPDATA%\Programs\Floti\bin\floti.exe. It
#    must say it stopped the host; floti.exe, the logon task "Floti host",
#    the desktop app where it was installed (Helm.exe and the Start menu's
#    Helm.lnk) and floti's folder on the user's Path must be gone, and the
#    data folder, %LOCALAPPDATA%\Floti, must stay. The floti.exe uninstall
#    runs from is renamed aside and deleted by a detached Windows PowerShell
#    once it exits, then Programs\Floti goes; that is waited for, up to a
#    minute. Where the runner would not let that PowerShell start, uninstall
#    says the copy is left for the next install's sweep, and this warns
#    rather than fails, as docs/PACKAGING.md allows. The desktop app's
#    settings are kept, and are not checked.
# 2. `floti uninstall --purge`, and the data folder must be gone.
# 3. `floti uninstall` again, which changes nothing: no line says anything
#    was removed, stopped or closed, and the sign-in entry, the desktop app
#    and the data each say "not found".
#
# floti removes itself in the first run, so the other two run from a copy of
# it made beforehand outside its install place; a floti from elsewhere
# leaves itself ("floti: left as it is"), which the third run's last line
# says. Each missing thing fails with one line naming it.
param(
  [Parameter(Mandatory)] [string]$By
)
$ErrorActionPreference = 'Stop'

function Fail([string]$Why) {
  # To the host: called inside a function whose output is its answer.
  Write-Host "::error::${By}: $Why"
  exit 1
}

function Get-UserPath {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  try { [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') } finally { $key.Close() }
}

$programs = Join-Path $env:LOCALAPPDATA 'Programs\Floti'
$folder = Join-Path $programs 'bin'
$floti = Join-Path $folder 'floti.exe'
$data = Join-Path $env:LOCALAPPDATA 'Floti'
$app = Join-Path $env:LOCALAPPDATA 'Programs\Helm\Helm.exe'
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Helm.lnk'

if (-not (Test-Path -LiteralPath $floti -PathType Leaf)) { Fail "there is no $floti to uninstall." }
if (-not (Test-Path -LiteralPath $data -PathType Container)) { Fail "setup made no data folder at $data." }
$hadApp = Test-Path -LiteralPath $app -PathType Leaf
$work = Join-Path $env:RUNNER_TEMP "try-uninstall-$PID"
New-Item -ItemType Directory -Path $work | Out-Null
$copy = Join-Path $work 'floti.exe'
Copy-Item -LiteralPath $floti -Destination $copy

# Runs `<program> uninstall <arguments>`, shows what it said, fails unless it
# exited 0, and answers its lines. Run with the call operator, not
# Start-Process -Wait, which waits on a job of its own that the PowerShell
# deleting floti once it exits would have to break away from.
function Invoke-Uninstall([string]$Program, [string[]]$Arguments) {
  $previous = $ErrorActionPreference
  # A native program's standard error, merged, must not stop the script.
  $ErrorActionPreference = 'Continue'
  try {
    $lines = @(& $Program uninstall @Arguments 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previous
  }
  $lines | ForEach-Object { Write-Host "  $_" }
  if ($code -ne 0) { Fail "floti uninstall $($Arguments -join ' ') exited ${code}: $($lines | Select-Object -Last 1)" }
  , $lines
}

function Assert-Said([string[]]$Lines, [string]$Pattern, [string]$Run) {
  if (-not ($Lines | Where-Object { $_ -match $Pattern })) { Fail "floti uninstall$Run did not say a line matching '$Pattern'." }
}

Write-Output 'floti uninstall:'
$said = Invoke-Uninstall $floti @()
Assert-Said $said '^host: stopped' ''
Assert-Said $said '^sign-in entry: removed' ''
Assert-Said $said '^PATH entry: removed' ''
Assert-Said $said '^data: kept' ''
Assert-Said $said '^floti: removed' ''
if (Test-Path -LiteralPath $floti) { Fail "floti uninstall left $floti." }
if (Get-ScheduledTask -TaskName 'Floti host' -ErrorAction SilentlyContinue) { Fail 'floti uninstall left the logon task "Floti host".' }
$onPath = [Environment]::ExpandEnvironmentVariables((Get-UserPath)) -split ';' | ForEach-Object { $_.Trim().TrimEnd('\') }
if ($onPath -contains $folder) { Fail "floti uninstall left $folder on the user's Path." }
if ($hadApp) {
  Assert-Said $said '^desktop app: removed' ''
  if (Test-Path -LiteralPath $app) { Fail "floti uninstall left the desktop app at $app." }
  if (Test-Path -LiteralPath $menu) { Fail "floti uninstall left the Start menu's entry at $menu." }
}
if (-not (Test-Path -LiteralPath $data -PathType Container)) { Fail "floti uninstall removed the data folder $data, which only --purge removes." }

$flotiLine = $said | Where-Object { $_ -like 'floti: removed*' } | Select-Object -First 1
$aside = 'the copy it ran from was deleted once it exited'
if ($flotiLine -like '*goes once it exits*') {
  $gone = $false
  for ($try = 0; $try -lt 600 -and -not $gone; $try++) {
    if (Test-Path -LiteralPath $programs) { Start-Sleep -Milliseconds 100 } else { $gone = $true }
  }
  if (-not $gone) {
    $left = @(Get-ChildItem -Force -Recurse -LiteralPath $programs | ForEach-Object { $_.FullName }) -join ', '
    Fail "a minute after floti uninstall exited, $programs is still there, holding: $left."
  }
} elseif ($flotiLine -like '*is left for the next*') {
  Write-Output "::warning::${By}: $flotiLine"
  $aside = 'the copy it ran from is left for the next install, since the runner would not let the deleting PowerShell start'
} elseif (Test-Path -LiteralPath $programs) {
  Fail "floti uninstall said `"$flotiLine`" and left $programs."
}

Write-Output 'floti uninstall --purge, from a copy:'
$said = Invoke-Uninstall $copy @('--purge')
Assert-Said $said '^data: removed' ' --purge'
if (Test-Path -LiteralPath $data) { Fail "floti uninstall --purge left the data folder $data." }

Write-Output 'floti uninstall again, from a copy:'
$said = Invoke-Uninstall $copy @()
$changed = $said | Where-Object { $_ -match '^[^:]+: (removed|stopped|closed)' } | Select-Object -First 1
if ($changed) { Fail "a second floti uninstall changed something: $changed" }
foreach ($line in 'sign-in entry: not found', 'desktop app: not found', 'data: not found') {
  Assert-Said $said "^$line$" ' run again'
}
Assert-Said $said '^floti: left as it is' ' run again'

Remove-Item -Recurse -Force -LiteralPath $work
$removed = if ($hadApp) { 'floti, the logon task, the desktop app and the Path entry' } else { 'floti, the logon task and the Path entry' }
Write-Output "${By}: floti uninstall removed $removed and kept the data ($aside); --purge removed the data; run again it found nothing."
Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY "- ${By}: ``floti uninstall`` removed $removed and kept the data ($aside); ``--purge`` removed the data; a second run exited 0 and found nothing"
