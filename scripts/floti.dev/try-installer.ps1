# The release dry run's test of the desktop's Windows installer, run for real
# in a CI runner and nowhere else: it runs the run's Helm-<version>-setup.exe
# silently and per user, as `floti add desktop` and a person's double click
# do, and checks that it put floti where install.ps1 puts it and ran the same
# setup.
#
#   powershell -File try-installer.ps1 -Installer <setup.exe> -Served <folder>
#     -Version <version> -Target <target> -Order <after-script|alone>
#
# <Served> holds the run's floti files. `-Order after-script` runs where
# install.ps1 has installed floti already; `alone` on a clean runner, where
# nothing of Floti's may be there before. After the install:
#
# - floti.exe in %LOCALAPPDATA%\Programs\Floti\bin is, byte for byte, the
#   release's floti-<version>-<target>.exe, the floti for this CPU;
# - no process runs from the app's own folder, %LOCALAPPDATA%\Programs\Helm,
#   so the floti the app carries is not the running host;
# - check-setup.ps1 passes: that floti says <Version>, its folder is on the
#   user's Path, and the logon task "Floti host" runs it and is running.
#
# Every check runs, and each one that fails is one line naming what is wrong.
param(
  [Parameter(Mandatory)] [string]$Installer,
  [Parameter(Mandatory)] [string]$Served,
  [Parameter(Mandatory)] [string]$Version,
  [Parameter(Mandatory)] [string]$Target,
  [Parameter(Mandatory)] [ValidateSet('after-script', 'alone')] [string]$Order
)
$ErrorActionPreference = 'Stop'
$by = "the installer on $Target ($Order)"

function Fail([string]$Why) {
  Write-Output "::error::${by}: $Why"
  exit 1
}

$folder = Join-Path $env:LOCALAPPDATA 'Programs\Floti\bin'
$floti = Join-Path $folder 'floti.exe'
$app = Join-Path $env:LOCALAPPDATA 'Programs\Helm'
$expected = Join-Path $Served "floti-$Version-$Target.exe"
if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) { Fail "there is no installer at $Installer." }
if (-not (Test-Path -LiteralPath $expected -PathType Leaf)) { Fail "the run has no $expected to compare with." }

if ($Order -eq 'alone') {
  if (Test-Path -LiteralPath $folder) { Fail "the runner is not clean: $folder is there before the install." }
  if (Get-ScheduledTask -TaskName 'Floti host' -ErrorAction SilentlyContinue) { Fail 'the runner is not clean: the logon task "Floti host" is there before the install.' }
} elseif (-not (Test-Path -LiteralPath $floti -PathType Leaf)) {
  Fail "install.ps1 left no $floti for the installer to find."
}

# Start-Process -Wait waits for the installer and every process it started
# that is still its descendant; the host is started by Task Scheduler, not by
# the installer, so it is not one.
$run = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($run.ExitCode -ne 0) { Fail "$Installer /S exited $($run.ExitCode)." }

$failures = New-Object System.Collections.Generic.List[string]
if (-not (Test-Path -LiteralPath (Join-Path $app 'Helm.exe') -PathType Leaf)) { $failures.Add("the app is not at $app\Helm.exe.") }

if (Test-Path -LiteralPath $floti -PathType Leaf) {
  $have = (Get-FileHash -Algorithm SHA256 -LiteralPath $floti).Hash
  if ($have -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $expected).Hash) {
    $match = Get-ChildItem -LiteralPath $Served -Filter "floti-$Version-*.exe" |
      Where-Object { (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash -eq $have } |
      Select-Object -First 1
    $is = if ($match) { "the release's $($match.Name)" } else { 'no file of the release' }
    $failures.Add("$floti is not the release's floti-$Version-$Target.exe: it is $is.")
  }
}

$fromApp = @(Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith("$app\", [StringComparison]::OrdinalIgnoreCase)
  })
foreach ($process in $fromApp) {
  $failures.Add("$($process.ExecutablePath) runs from the app's own folder ($($process.CommandLine)).")
}

foreach ($why in $failures) { Write-Output "::error::${by}: $why" }

& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'check-setup.ps1') -Version $Version -By $by
$setup = $LASTEXITCODE
if ($failures.Count -gt 0 -or $setup -ne 0) { exit 1 }

Write-Output "${by}: the app is at $app, floti.exe in $folder is the release's floti-$Version-$Target.exe, and nothing runs from the app's folder."
Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY "- ${by}: ``Helm-$Version-setup.exe /S`` put the app at ``$app`` and, in ``$folder``, the release's ``floti-$Version-$Target.exe`` byte for byte; nothing runs from the app's folder"
