# Full Windows uninstall proof: install the real NSIS setup, let the PRODUCT arm the state under test, run
# the real uninstaller silently, then assert clean removal AND documented retention.
#
# Extracted to a script (repo convention, cf. packaged-e2e-*.sh / updater-smoke-windows.ps1) so the SAME code
# runs in two places: the release draft-gate leg in `_e2e-packaged.yml`, and a branch-runnable probe against
# an already-published installer. The release pipeline refuses to run from a non-main ref (by design), so
# without that second entry point this logic could only ever be exercised after it was already merged and
# release-blocking.
#
# Arming is by the product, not by CI: `intent_enabled_now()` (autostart.rs) reads the Run-key ARTIFACT as the
# source of truth, so seeding that value is the moral equivalent of the user ticking "Start on Login". The
# launched app then derives the rest itself — it heals the value to its own exactly-quoted path and arms the
# crash-recovery task. Everything asserted GONE below is asserted PRESENT first: without that, every removal
# assertion would pass vacuously on a runner where the app was never installed.

param(
  # Directory containing the Windows *-setup.exe under test.
  [Parameter(Mandatory = $true)][string]$AppArtifactDir
)

$ErrorActionPreference = "Stop"
# PowerShell 7.4+ defaults `$PSNativeCommandUseErrorActionPreference` to $true, which turns ANY non-zero exit
# from a native command into a terminating error under `Stop`. This script deliberately shells out to tools
# whose non-zero exits are EXPECTED and meaningful rather than fatal — `schtasks /Delete` on a task that does
# not exist yet (the pre-arm cleanup) exits 1, and `schtasks /Query` exits non-zero as its "absent" answer,
# which is precisely what the postconditions read. So decide on exit codes explicitly ($LASTEXITCODE), the
# way the rest of this repo's pwsh does, instead of letting a native exit code throw.
$PSNativeCommandUseErrorActionPreference = $false
$runKey = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
$runValueName = "Aztec Accelerator"
$taskName = "Aztec Accelerator Crash Recovery"
$aztecDir = Join-Path $env:USERPROFILE ".aztec-accelerator"

$setup = Get-ChildItem -Path $AppArtifactDir -Recurse -Filter "*-setup.exe" -EA SilentlyContinue | Select-Object -First 1
if (-not $setup) {
  Write-Host "::error::no Windows *-setup.exe under $AppArtifactDir"
  Get-ChildItem -Path $AppArtifactDir -Recurse -EA SilentlyContinue | Select-Object FullName | Out-String | Write-Host
  exit 1
}

# The installer is unsigned (B1 Authenticode is still deferred); a Defender quarantine mid-run would
# masquerade as a product failure. Scoped to the per-user install target on an ephemeral runner.
Add-MpPreference -ExclusionPath "$env:LOCALAPPDATA" -ErrorAction SilentlyContinue

# NSIS /S is ASYNC: -PassThru + WaitForExit so a (never-expected) interactive prompt fails fast here instead
# of hanging the job to its timeout.
Write-Host "installing $($setup.FullName) /S"
$inst = Start-Process -FilePath $setup.FullName -ArgumentList "/S" -PassThru
if (-not $inst.WaitForExit(180000)) {
  $inst.Kill()
  Write-Host "::error::installer did not finish within 180s — an interactive NSIS prompt would hang the runner"
  exit 1
}

# Address the app BY NAME under the per-user install root (installMode currentUser). Never a bare recursive
# first-match: the install dir also carries the bundled `bb` sidecar.
$exe = Get-ChildItem -Path "$env:LOCALAPPDATA" -Recurse -Filter "AztecAccelerator.exe" -EA SilentlyContinue | Select-Object -First 1
if (-not $exe) { Write-Host "::error::installed AztecAccelerator.exe not found under %LOCALAPPDATA%"; exit 1 }
$installDir = $exe.Directory.FullName
$uninstaller = Join-Path $installDir "uninstall.exe"
if (-not (Test-Path $uninstaller)) { Write-Host "::error::no uninstall.exe in $installDir"; exit 1 }
Write-Host "installed to $installDir"

# Cert MATERIAL on disk (headless-safe: generates CA/leaf/key, never touches the OS trust store — the same
# call the linux/macos legs use). This is what `--prepare-uninstall` must delete.
# POLL, don't test once: this is a GUI-subsystem binary, so the call returns as soon as it detaches and the
# certs land via a staged write + atomic rename (measured: a single Test-Path 190ms later still lost).
$certsDir = Join-Path $aztecDir "certs"
$caFile = Join-Path $certsDir "ca.pem"
& $exe.FullName --generate-certs-only
$deadline = (Get-Date).AddSeconds(30)
while (-not (Test-Path $caFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
if (-not (Test-Path $caFile)) {
  Get-ChildItem -Path $aztecDir -Recurse -EA SilentlyContinue | Select-Object FullName | Out-String | Write-Host
  Write-Host "::error::precondition: --generate-certs-only produced no ca.pem within 30s"
  exit 1
}

# A config file that must SURVIVE the uninstall (documented retention: config + bb cache are never touched).
$configFile = Join-Path $aztecDir "config.json"
Set-Content -Path $configFile -NoNewline -Value '{"config_version":2,"approved_origins":["http://uninstall.example"]}'

# Seed INTENT (= the user's "Start on Login" tick) as a STALE value, so the healed result proves the product
# wrote it rather than CI having pre-written the expected string.
Set-ItemProperty -Path $runKey -Name $runValueName -Value "$env:LOCALAPPDATA\Aztec Stale\Aztec Accelerator.exe "
schtasks /Delete /TN $taskName /F 2>$null | Out-Null

$env:AZTEC_ACCEL_NO_UPDATE = "1"
$proc = Start-Process -FilePath $exe.FullName -PassThru
$healthy = $false
for ($i = 0; $i -lt 30; $i++) {
  try {
    if ((Invoke-RestMethod -Uri "http://127.0.0.1:59833/health" -TimeoutSec 3).status -eq "ok") { $healthy = $true; break }
  } catch { }
  Start-Sleep -Seconds 2
}
if (-not $healthy) { Write-Host "::error::installed app did not serve /health within 60s"; exit 1 }

# ── PRECONDITIONS ──
$expectedRun = '"' + $exe.FullName + '"'
$armedRun = $null
$deadline = (Get-Date).AddSeconds(20)
do {
  $armedRun = (Get-ItemProperty -Path $runKey -Name $runValueName -EA SilentlyContinue).$runValueName
  if ($armedRun -ceq $expectedRun) { break }
  Start-Sleep -Milliseconds 500
} while ((Get-Date) -lt $deadline)
if ($armedRun -cne $expectedRun) {
  Write-Host "::error::precondition: the product did not heal the Run value to its own quoted path (got '$armedRun')"
  exit 1
}
schtasks /Query /TN $taskName 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) {
  Write-Host "::error::precondition: the product did not arm the crash-recovery task (intent was seeded on)"
  exit 1
}
Write-Host "armed by the product: healed Run value + crash-recovery task + certs on disk"

# ── The real uninstaller, silently, with the app STILL RUNNING (what a user does from Add/Remove Programs).
#    PREUNINSTALL runs `--prepare-uninstall` (ownership-checked teardown); POSTUNINSTALL runs the belt.
#    `$EXEDIR != $INSTDIR` holds because Windows copies uninstall.exe to a temp dir for a real uninstall. ──
$un = Start-Process -FilePath $uninstaller -ArgumentList "/S" -PassThru
if (-not $un.WaitForExit(180000)) { $un.Kill(); Write-Host "::error::uninstaller did not finish within 180s"; exit 1 }

# NSIS returns before its temp-dir copy finishes removing $INSTDIR; give it a bounded settle.
$gone = $false
$deadline = (Get-Date).AddSeconds(60)
do {
  if (-not (Test-Path $installDir)) { $gone = $true; break }
  Start-Sleep -Seconds 2
} while ((Get-Date) -lt $deadline)

# ── POSTCONDITIONS ──
$failures = @()
if (-not $gone) { $failures += "install dir still present: $installDir" }
$runAfter = (Get-ItemProperty -Path $runKey -Name $runValueName -EA SilentlyContinue).$runValueName
if ($null -ne $runAfter) { $failures += "autostart Run value survived: '$runAfter'" }
schtasks /Query /TN $taskName 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { $failures += "crash-recovery scheduled task survived" }
if (Test-Path $certsDir) { $failures += "certs dir survived: $certsDir" }
# Retention is as load-bearing as removal: an uninstall that eats the user's config is a data-loss bug.
if (-not (Test-Path $configFile)) { $failures += "config.json was deleted (documented retention says it is never touched)" }

# The PT1M crash-recovery trigger is exactly why "no orphan" needs a wait, not an instant check: a surviving
# task would relaunch the app within ~1 minute of it going away.
Start-Sleep -Seconds 75
$alive = Get-Process -Name "AztecAccelerator" -EA SilentlyContinue
if ($alive) { $failures += "an AztecAccelerator process is running 75s after uninstall (resurrected by a surviving task?)" }
if (Test-Path $installDir) { $failures += "install dir reappeared after the crash-recovery window" }

if ($failures.Count -gt 0) {
  foreach ($f in $failures) { Write-Host "::error::$f" }
  Write-Host "::error::uninstall left $($failures.Count) artifact(s) behind"
  exit 1
}
Write-Host "OK: real NSIS uninstall removed install dir + autostart + crash-recovery task + certs, kept config, and nothing came back"
exit 0
