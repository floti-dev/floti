# The release dry run's test of floti.dev's install.ps1, run for real in a CI
# runner and nowhere else: it installs floti in this user's own folder, puts
# it on their PATH and runs `floti setup`, as a person's `irm | iex` does.
#
#   pwsh try-install.ps1 -Served <folder> -Version <version> -Target <target>
#
# <Served> holds the run's floti files and an install.ps1 made with
# `floti-dev.mjs scripts --release-url http://127.0.0.1:8765`, which a local
# server serves there. Every script runs in Windows PowerShell, which is what
# `irm | iex` gets on a Windows that has nothing else. First the script is cut
# off at ten places, each run with Invoke-Expression and a LOCALAPPDATA of its
# own, which must stay empty, with the user's Path unchanged and no floti file
# asked for. Then `irm <url>/install.ps1 | iex` runs, and the session must
# go on after it: the script never exits a person's window. floti must be at
# %LOCALAPPDATA%\Programs\Floti\bin\floti.exe, `floti version` must name
# <Version> and <Target>, its folder must be on the user's Path, and the
# script must have reached setup, whatever setup made of the runner.
param(
  [Parameter(Mandatory)] [string]$Served,
  [Parameter(Mandatory)] [string]$Version,
  [Parameter(Mandatory)] [string]$Target
)
$ErrorActionPreference = 'Stop'

# It installs Floti on this computer and puts it on the user's Path, so it
# runs only on a CI runner.
if ($env:GITHUB_ACTIONS -ne 'true') {
  Write-Output 'try-install.ps1 installs Floti on this computer, so it runs only in GitHub Actions (GITHUB_ACTIONS=true); nothing was changed.'
  exit 1
}

$port = 8765
$url = "http://127.0.0.1:$port"

function Fail([string]$Why) {
  Write-Output "::error::install.ps1 on ${Target}: $Why"
  exit 1
}

# Runs `command` in Windows PowerShell, with LOCALAPPDATA set to
# `localAppData` when one is given, and answers what it wrote.
function Invoke-WindowsPowerShell([string]$Command, [string]$LocalAppData) {
  $out = New-TemporaryFile
  $err = New-TemporaryFile
  $before = $env:LOCALAPPDATA
  if ($LocalAppData) { $env:LOCALAPPDATA = $LocalAppData }
  try {
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Command))
    $child = Start-Process powershell.exe -ArgumentList '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded `
      -NoNewWindow -Wait -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
  } finally {
    $env:LOCALAPPDATA = $before
  }
  $said = (Get-Content -Raw $out) + (Get-Content -Raw $err)
  Remove-Item $out, $err
  [pscustomobject]@{ Code = $child.ExitCode; Said = "$said" }
}

function Get-UserPath {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  try { [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') } finally { $key.Close() }
}

$work = Join-Path $env:RUNNER_TEMP "try-install-$PID"
New-Item -ItemType Directory -Path $work | Out-Null
$log = Join-Path $work 'server.log'
$server = Start-Process python -ArgumentList '-m', 'http.server', $port, '--bind', '127.0.0.1', '--directory', $Served `
  -NoNewWindow -PassThru -RedirectStandardError $log -RedirectStandardOutput (Join-Path $work 'server.out')
try {
  $answered = $false
  for ($try = 0; $try -lt 200 -and -not $answered; $try++) {
    try { Invoke-WebRequest -UseBasicParsing "$url/install.ps1" | Out-Null; $answered = $true } catch { Start-Sleep -Milliseconds 100 }
  }
  if (-not $answered) { Fail 'the local server never answered.' }

  $script = [IO.File]::ReadAllText((Join-Path $Served 'install.ps1'))
  $lastLine = ($script.TrimEnd() -split "`n")[-1]
  $size = $script.Length
  $pathBefore = Get-UserPath
  $cuts = @(1..9 | ForEach-Object { [int]($size * $_ / 10) }) + @($script.TrimEnd().Length - $lastLine.Length)
  foreach ($cut in $cuts) {
    $folder = Join-Path $work "cut-$cut"
    New-Item -ItemType Directory -Path $folder | Out-Null
    $file = Join-Path $work "cut-$cut.ps1"
    [IO.File]::WriteAllText($file, $script.Substring(0, $cut))
    Invoke-WindowsPowerShell "Invoke-Expression ([IO.File]::ReadAllText('$file'))" $folder | Out-Null
    $left = @(Get-ChildItem -Force -LiteralPath $folder)
    if ($left.Count -gt 0) { Fail "cut off after $cut of $size characters, it ran: it left $($left.Name -join ', ') in LOCALAPPDATA." }
    if ((Get-UserPath) -ne $pathBefore) { Fail "cut off after $cut of $size characters, it changed the user's Path." }
  }
  if (Select-String -Quiet -Pattern 'GET /floti-' -LiteralPath $log) { Fail 'a script cut off asked the server for a floti file.' }
  Write-Output 'install.ps1 cut off at ten places, the last just before its last line: each ran nothing.'

  # The marker is printed by the session iex ran in, after it: a script that
  # exits takes that session with it, as it would close a person's window.
  $marker = 'iex returned to the session that ran it'
  $run = Invoke-WindowsPowerShell "irm $url/install.ps1 | iex; [Console]::Out.WriteLine('$marker')"
  Write-Output $run.Said
  $said = $run.Said -split "`r?`n"
  if ($said -notcontains $marker) { Fail "iex never returned: the script ended the PowerShell session it ran in (exit $($run.Code))." }
  $floti = Join-Path $env:LOCALAPPDATA 'Programs\Floti\bin\floti.exe'
  $failed = $said | Where-Object { $_ -like 'floti install: *' } | Select-Object -First 1
  if ($failed) { Fail "it did not install floti: $failed" }
  if ($said -notcontains "floti $Version is at $floti.") { Fail "it did not say floti is at $floti." }
  if (-not (Test-Path -LiteralPath $floti)) { Fail "$floti is not there." }
  $line = (& $floti version | Out-String).Trim()
  $os, $cpu = $Target -split '-', 2
  if ($line -notlike "floti $Version protocol * $os $cpu") { Fail "$floti version said `"$line`", not floti $Version for $os $cpu." }
  $folder = Split-Path -Parent $floti
  $onPath = [Environment]::ExpandEnvironmentVariables((Get-UserPath)) -split ';' | ForEach-Object { $_.Trim().TrimEnd('\') }
  if ($onPath -notcontains $folder) { Fail "$folder is not on the user's Path." }
  $setup = $said | Where-Object { $_ -match '^(The Floti host |floti setup: )' } | Select-Object -Last 1
  if (-not $setup) { Fail "it never reached floti setup (exit $($run.Code))." }

  Write-Output "install.ps1 on $Target installed $line at $floti; setup said: $setup"
  Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY "- ``install.ps1`` on ``$Target``: cut off at ten places it ran nothing; whole, through Windows PowerShell's ``irm | iex``, it installed ``$line`` at ``$floti``, put its folder on the user's Path, and setup said `"$setup`""
} finally {
  Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
}
