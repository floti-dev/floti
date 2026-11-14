# The release workflow's test of `floti uninstall` on a Windows runner, after
# an install and check-setup.ps1, run there and nowhere else: it removes what
# the install put there.
#
#   powershell -File try-uninstall.ps1 -By <what installed it>
#
# Three runs, each of which must exit 0:
#
# 1. `floti uninstall` from %LOCALAPPDATA%\Programs\Floti\bin\floti.exe, on
#    a computer an earlier floti set up first: its logon task "Floti host",
#    imported from the XML it kept in the data folder's `startup` folder, is
#    registered here and run, so the host it starts runs from floti's folder.
#    Uninstall must say it stopped the host and removed that task, and no
#    process may run from floti's folder after it; floti.exe, the task and
#    its XML, the desktop app where it was installed (Floti.exe and the Start
#    menu's Floti.lnk) and floti's folder on the user's Path must be gone,
#    and the data folder, %LOCALAPPDATA%\Floti, must stay. The floti.exe uninstall
#    runs from is renamed aside and deleted by a detached Windows PowerShell
#    once it exits, then Programs\Floti goes; that is waited for, up to a
#    minute, and a copy uninstall says is left for a later sweep fails it.
#    The desktop app's settings are kept, and are not checked.
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

# It uninstalls Floti from this computer, so it runs only on a CI runner.
if ($env:GITHUB_ACTIONS -ne 'true') {
  Write-Output 'try-uninstall.ps1 uninstalls Floti from this computer, so it runs only in GitHub Actions (GITHUB_ACTIONS=true); nothing was changed.'
  exit 1
}

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
$app = Join-Path $env:LOCALAPPDATA 'Programs\Floti\desktop\Floti.exe'
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Floti.lnk'

if (-not (Test-Path -LiteralPath $floti -PathType Leaf)) { Fail "there is no $floti to uninstall." }
if (-not (Test-Path -LiteralPath $data -PathType Container)) { Fail "setup made no data folder at $data." }
$hadApp = Test-Path -LiteralPath $app -PathType Leaf

# What an earlier floti's setup left: the logon task "Floti host", as its
# setup wrote and imported it, and the host the task started. This floti's
# setup registers and starts neither.
$startup = Join-Path $data 'startup'
$taskXml = Join-Path $startup 'Floti host.xml'
New-Item -ItemType Directory -Force -Path $startup | Out-Null
$user = if ($env:USERDOMAIN) { "$env:USERDOMAIN\$env:USERNAME" } else { $env:USERNAME }
$escape = { param([string]$Text) [Security.SecurityElement]::Escape($Text) }
$task = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts the Floti host when you sign in.</Description>
    <URI>\Floti host</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>$(& $escape $user)</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>$(& $escape $user)</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"$(& $escape $floti)"</Command>
      <Arguments>host</Arguments>
    </Exec>
  </Actions>
</Task>
"@
[IO.File]::WriteAllText($taskXml, $task, [Text.Encoding]::Unicode)
& schtasks.exe /Create /TN 'Floti host' /XML $taskXml /F | Out-Null
if ($LASTEXITCODE -ne 0) { Fail "could not register the earlier floti's logon task to uninstall (schtasks exited $LASTEXITCODE)." }
& schtasks.exe /Run /TN 'Floti host' | Out-Null
if ($LASTEXITCODE -ne 0) { Fail "could not run the earlier floti's logon task (schtasks exited $LASTEXITCODE)." }
# The host answers once `floti update-stop --ask` reaches it: 0 with
# sessions to end, 7 with none; 3 while nothing answers yet.
$answered = $false
$previous = $ErrorActionPreference
# What it says on standard error while nothing answers must not stop the script.
$ErrorActionPreference = 'Continue'
try {
  for ($try = 0; $try -lt 600 -and -not $answered; $try++) {
    & $floti update-stop --ask *> $null
    if ($LASTEXITCODE -eq 0 -or $LASTEXITCODE -eq 7) { $answered = $true } else { Start-Sleep -Milliseconds 100 }
  }
} finally {
  $ErrorActionPreference = $previous
}
if (-not $answered) { Fail 'the host the logon task started did not answer within a minute.' }

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
Assert-Said $said '^sign-in entry: removed \(logon task "Floti host"\)$' ''
Assert-Said $said '^PATH entry: removed' ''
Assert-Said $said '^data: kept' ''
Assert-Said $said '^floti: removed' ''
if (Test-Path -LiteralPath $floti) { Fail "floti uninstall left $floti." }
if (Get-ScheduledTask -TaskName 'Floti host' -ErrorAction SilentlyContinue) { Fail 'floti uninstall left the logon task "Floti host".' }
if (Test-Path -LiteralPath $startup) { Fail "floti uninstall left the logon task's XML folder $startup." }
$onPath = [Environment]::ExpandEnvironmentVariables((Get-UserPath)) -split ';' | ForEach-Object { $_.Trim().TrimEnd('\') }
if ($onPath -contains $folder) { Fail "floti uninstall left $folder on the user's Path." }
if ($hadApp) {
  Assert-Said $said '^desktop app: removed' ''
  if (Test-Path -LiteralPath $app) { Fail "floti uninstall left the desktop app at $app." }
  if (Test-Path -LiteralPath $menu) { Fail "floti uninstall left the Start menu's entry at $menu." }
}
if (-not (Test-Path -LiteralPath $data -PathType Container)) { Fail "floti uninstall removed the data folder $data, which only --purge removes." }

# The host and its session keeper ran from floti's folder; once uninstall
# has stopped them, nothing may, given up to ten seconds to exit.
$running = @()
for ($try = 0; $try -lt 100; $try++) {
  $running = @(Get-CimInstance Win32_Process | Where-Object {
      $_.ExecutablePath -and (Split-Path -Parent $_.ExecutablePath) -eq $folder
    })
  if ($running.Count -eq 0) { break }
  Start-Sleep -Milliseconds 100
}
if ($running.Count -gt 0) {
  Fail "after floti uninstall, $($running[0].ExecutablePath) still runs ($($running[0].CommandLine))."
}

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
  Fail "floti uninstall could not have the copy it ran from deleted once it exited: $flotiLine"
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
