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
# masquerade as a product failure. Scoped to the EXACT install directory (not all of %LOCALAPPDATA%) — the
# path need not exist yet for an exclusion to be registered.
$installRoot = Join-Path $env:LOCALAPPDATA "Aztec Accelerator"
Add-MpPreference -ExclusionPath $installRoot -ErrorAction SilentlyContinue

# NSIS /S is ASYNC: -PassThru + WaitForExit so a (never-expected) interactive prompt fails fast here instead
# of hanging the job to its timeout.
Write-Host "installing $($setup.FullName) /S"
$inst = Start-Process -FilePath $setup.FullName -ArgumentList "/S" -PassThru
if (-not $inst.WaitForExit(180000)) {
  $inst.Kill()
  Write-Host "::error::installer did not finish within 180s — an interactive NSIS prompt would hang the runner"
  exit 1
}
if ($inst.ExitCode -ne 0) { Write-Host "::error::installer exited $($inst.ExitCode)"; exit 1 }

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
# The Run KEY itself does not exist on a fresh runner profile (measured: Set-ItemProperty failed with
# "Cannot find path ... because it does not exist"), so create it first. `-Force` is a no-op when it already
# exists, which is the normal case on a real user's machine.
New-Item -Path $runKey -Force -ErrorAction SilentlyContinue | Out-Null
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
$taskXml = schtasks /Query /TN $taskName /XML 2>$null
if ($LASTEXITCODE -ne 0) {
  Write-Host "::error::precondition: the product did not arm the crash-recovery task (intent was seeded on)"
  exit 1
}
# A task merely EXISTING under that name proves little (a foreign task could hold the name). Bind the
# precondition to OUR installed exe. Parse the XML rather than substring-matching the whole document: the
# path could otherwise appear in an argument or description and still satisfy a naive `-like`, which would
# make "bound to our exe" a claim the check does not actually support. Fail closed if the XML will not parse
# — falling back to a weaker match is exactly the fail-open pattern this pass removed elsewhere.
$xmlText = ($taskXml -join "`n").TrimStart([char]0xFEFF, ' ', "`t", "`r", "`n")
try {
  [xml]$taskDoc = $xmlText
} catch {
  Write-Host "::error::precondition: could not parse the crash-recovery task XML"
  Write-Host $xmlText
  exit 1
}
# Array-wrap and demand EXACTLY ONE action: with multiple <Exec> nodes `.Command` is a collection, and `-ne`
# against a collection FILTERS it instead of returning a boolean — two identical commands would yield an
# empty (falsy) result and sail through. Requiring one command also means "the task runs our exe" cannot be
# satisfied by our exe merely being one action among several.
$taskCommands = @($taskDoc.Task.Actions.Exec.Command)
if ($taskCommands.Count -ne 1 -or
    [string]::IsNullOrWhiteSpace($taskCommands[0]) -or
    $taskCommands[0].Trim('"') -ne $exe.FullName) {
  Write-Host "::error::precondition: expected exactly one crash-recovery action running the installed exe; got $($taskCommands.Count): $($taskCommands -join ' | ')"
  exit 1
}
# Byte-exact config snapshot: "still exists" would also pass if uninstall truncated or rewrote it.
$configHashBefore = (Get-FileHash -Path $configFile -Algorithm SHA256).Hash
Write-Host "armed by the product: healed Run value + crash-recovery task (bound to our exe) + certs on disk"

# ── The real uninstaller, silently, with the app STILL RUNNING (what a user does from Add/Remove Programs).
#    PREUNINSTALL runs `--prepare-uninstall` (ownership-checked teardown); POSTUNINSTALL runs the belt.
#    `$EXEDIR != $INSTDIR` holds because Windows copies uninstall.exe to a temp dir for a real uninstall. ──
# "Uninstalled while running" is the whole point of this leg, so prove the app is ALIVE right now — health
# was sampled earlier, and an app that crashed in between would let a broken running-app teardown pass.
# Check by NAME, not just our PID: the crash-recovery task is armed at this point and may legitimately have
# started a second instance that won the port race, letting our original process bow out benignly. What must
# hold is "an app is running", not "this exact pid is running".
$aliveBefore = @(Get-Process -Name "AztecAccelerator" -ErrorAction SilentlyContinue)
if ($aliveBefore.Count -eq 0) {
  Write-Host "::error::precondition: no AztecAccelerator process is running before the uninstall (nothing proves running-app teardown)"
  exit 1
}

$un = Start-Process -FilePath $uninstaller -ArgumentList "/S" -PassThru
if (-not $un.WaitForExit(180000)) { $un.Kill(); Write-Host "::error::uninstaller did not finish within 180s"; exit 1 }
if ($un.ExitCode -ne 0) { Write-Host "::error::uninstaller exited $($un.ExitCode)"; exit 1 }

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

# Run-value absence must not be FAIL-OPEN: `-EA SilentlyContinue` maps an unreadable-but-SURVIVING value to
# $null, which would pass. Read the key and enumerate value NAMES instead, so a real read error throws.
if (Test-Path $runKey) {
  if ((Get-Item -Path $runKey).GetValueNames() -contains $runValueName) {
    $survived = (Get-ItemProperty -Path $runKey -Name $runValueName).$runValueName
    $failures += "autostart Run value survived: '$survived'"
  }
}

# Same fail-open trap on the task: treating ANY non-zero as "absent" would pass on access-denied or a
# scheduler fault with the task still armed. Exit code 1 is the "does not exist" answer; anything else
# non-zero is an ERROR, not an absence — the product's own trust/recovery code draws the same distinction.
schtasks /Query /TN $taskName 2>$null | Out-Null
$queryCode = $LASTEXITCODE
if ($queryCode -eq 0) { $failures += "crash-recovery scheduled task survived" }
elseif ($queryCode -ne 1) { $failures += "schtasks /Query failed with $queryCode - cannot conclude the task is gone" }

if (Test-Path $certsDir) { $failures += "certs dir survived: $certsDir" }

# Retention is as load-bearing as removal, and EXISTENCE is not retention: a truncated or rewritten config
# would sail through a Test-Path. Compare the bytes.
if (-not (Test-Path $configFile)) {
  $failures += "config.json was deleted (documented retention says it is never touched)"
} else {
  $configHashAfter = (Get-FileHash -Path $configFile -Algorithm SHA256).Hash
  if ($configHashAfter -ne $configHashBefore) { $failures += "config.json was MODIFIED by the uninstall" }
}

# An app was provably running when the uninstaller started, so NO app may be running now. This must be a
# by-name check, not just our pid: deleting a scheduled task does not terminate a process that task already
# started (Microsoft documents this for `schtasks /delete`), so a task-spawned instance could outlive both
# the task and our original process and slip past a pid-only assertion.
# A "did anything come back?" sleep is deliberately absent: a missed scheduled start can be delayed by
# minutes, so no bounded wait proves the absence of a FUTURE relaunch — it would look like evidence without
# being any. Future triggers are proven dead by asserting the task itself is gone, above; this check covers
# the already-started case that task-absence does not subsume.
$aliveAfter = @(Get-Process -Name "AztecAccelerator" -ErrorAction SilentlyContinue)
if ($aliveAfter.Count -gt 0) {
  $failures += "$($aliveAfter.Count) AztecAccelerator process(es) survived the uninstall (pids: $($aliveAfter.Id -join ', '))"
}

if ($failures.Count -gt 0) {
  foreach ($f in $failures) { Write-Host "::error::$f" }
  Write-Host "::error::uninstall left $($failures.Count) artifact(s) behind"
  exit 1
}
Write-Host "OK: real NSIS uninstall removed install dir + autostart + crash-recovery task + certs, kept config byte-identical, and stopped the running app"
exit 0
