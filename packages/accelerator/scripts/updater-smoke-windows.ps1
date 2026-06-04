<#
  Release-time updater smoke test (Windows / NSIS) — ADVISORY first.

  Windows sibling of updater-smoke-linux.sh. Proves a user on N-1 auto-updates to
  the just-built+signed build (N) via a local feed impersonating aztec-accelerator.dev,
  and relaunches reporting version N. Answers the open `v1Compatible` question on
  Windows: does the updater download the .nsis.zip, minisign-verify it, run the NSIS
  installer SILENTLY (currentUser, no UAC), and relaunch?

  Trust model (no signing key needed): we serve an ALREADY-signed N .nsis.zip; N-1
  (unmodified) verifies the .sig against its embedded pubkey. reqwest on Windows uses
  schannel, which reads the Windows cert store — so a local CA trusted in LocalMachine\Root
  is honored.

  Windows vs Linux swaps:
    CA trust : Import-Certificate -> LocalMachine\Root (vs update-ca-certificates). MUST be the
               MACHINE store, not per-user: adding a CA to the per-user Trusted Root pops a GUI
               confirmation that hangs a headless runner; the machine store is admin-authorized
               (runner is admin) so it adds silently. schannel validates against both.
    hosts    : %SystemRoot%\System32\drivers\etc\hosts
    install  : <setup>.exe /S  (silent NSIS, %LOCALAPPDATA%)   (vs cp + chmod AppImage)
    AV       : scoped Add-MpPreference exclusion (unsigned exe) (no Linux analogue)
    feed     : same bun server, no sudo (Windows binds :443 as user)

  Usage:
    updater-smoke-windows.ps1 -NVersion 9.9.9 -NArtifactsDir <dir> -N1Installer <setup.exe> -RepoRoot <root>
    -NArtifactsDir : dir with N's *-setup.nsis.zip + *-setup.nsis.zip.sig
    -N1Installer   : path to N-1's *-setup.exe
  UPDATER_SMOKE_MODE = positive (default) | negative (tamper the served zip, expect rejection)
#>
param(
  [Parameter(Mandatory)][string]$NVersion,
  [string]$PlatformKey = "windows-x86_64",
  [Parameter(Mandatory)][string]$NArtifactsDir,
  [Parameter(Mandatory)][string]$N1Installer,
  [Parameter(Mandatory)][string]$RepoRoot
)

$ErrorActionPreference = "Stop"
$Mode = if ($env:UPDATER_SMOKE_MODE) { $env:UPDATER_SMOKE_MODE } else { "positive" }
$FeedHost = "aztec-accelerator.dev"
$HealthUrl = "http://127.0.0.1:59833/health"
$HostsFile = "$env:SystemRoot\System32\drivers\etc\hosts"
$ConfigDir = Join-Path $env:USERPROFILE ".aztec-accelerator"
$InstallRoot = "$env:LOCALAPPDATA\Aztec Accelerator"
# Run-unique CA name so cleanup removes only THIS run's anchor (self-hosted safety).
$CaCn = "updater-smoke-CA-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)"
$Work = Join-Path $env:RUNNER_TEMP ("updater-smoke-" + [guid]::NewGuid().ToString('N'))
$ServeDir = Join-Path $Work "serve"
New-Item -ItemType Directory -Force -Path $ServeDir | Out-Null
$AppProc = $null
$FeedProc = $null
$CaThumb = $null
$PollJob = $null
$TaskName = "Aztec Accelerator Crash Recovery"

function Log($m) { Write-Host "── $m ──" }

function Cleanup {
  if ($AppProc) { Stop-Process -Id $AppProc.Id -Force -ErrorAction SilentlyContinue }
  Get-Process -Name "aztec-accelerator" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  if ($FeedProc) { Stop-Process -Id $FeedProc.Id -Force -ErrorAction SilentlyContinue }
  # Drop the scoped Defender exclusions we added (self-hosted-runner hygiene; no-op on
  # ephemeral GH-hosted runners, which are torn down — but never leave an AV hole behind).
  Remove-MpPreference -ExclusionPath $InstallRoot, $ServeDir -ErrorAction SilentlyContinue
  # Drop the test CA from LocalMachine\Root (ephemeral runner is torn down anyway).
  if ($CaThumb) { Get-ChildItem "Cert:\LocalMachine\Root\$CaThumb" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue }
  # Drop the hosts line we added (anchored, exact).
  if (Test-Path $HostsFile) {
    (Get-Content $HostsFile) | Where-Object { $_ -notmatch "^127\.0\.0\.1\s+$([regex]::Escape($FeedHost))$" } | Set-Content $HostsFile -ErrorAction SilentlyContinue
  }
  # (#96) Disarm: drop the autostart Run key + the crash-recovery task we may have armed, and
  # verify the task is gone — an armed task must not leak even on an ephemeral runner.
  Remove-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -ErrorAction SilentlyContinue
  & schtasks /Delete /F /TN $TaskName *> $null
  # (#97) Stop the background task-state poller if it's still running (don't leak a job).
  if ($PollJob) { Stop-Job $PollJob -ErrorAction SilentlyContinue; Remove-Job $PollJob -Force -ErrorAction SilentlyContinue }
}

function Dump-Logs {
  Write-Host "── feed log ──"; Get-Content (Join-Path $Work "feed.log") -ErrorAction SilentlyContinue
  Write-Host "── feed err ──"; Get-Content (Join-Path $Work "feed.err") -ErrorAction SilentlyContinue
  Write-Host "── app log (what the updater actually did) ──"
  Get-ChildItem "$env:LOCALAPPDATA\aztec-accelerator\logs" -ErrorAction SilentlyContinue |
    ForEach-Object { Write-Host "-- $($_.Name) --"; Get-Content $_.FullName -Tail 80 -ErrorAction SilentlyContinue }
  Write-Host "── last /health ──"; try { Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3 | ConvertTo-Json -Compress } catch { "unreachable" }
}

try {
  # ── Locate N's signed updater artifact ──
  $NZip = Get-ChildItem -Path $NArtifactsDir -Recurse -Filter "*-setup.nsis.zip" | Select-Object -First 1
  $NSig = Get-ChildItem -Path $NArtifactsDir -Recurse -Filter "*-setup.nsis.zip.sig" | Select-Object -First 1
  if (-not $NZip) { Write-Error "no *-setup.nsis.zip in $NArtifactsDir"; exit 1 }
  if (-not $NSig) { Write-Error "no *-setup.nsis.zip.sig in $NArtifactsDir"; exit 1 }
  $NName = $NZip.Name
  $Served = Join-Path $ServeDir $NName
  Copy-Item $NZip.FullName $Served
  $NSigText = (Get-Content $NSig.FullName -Raw).Trim()
  Log "N artifact: $NName"

  # Negative control: tamper the served zip (append a byte) but keep the GENUINE sig,
  # so the minisign check over the tampered bytes MUST fail against the embedded pubkey.
  if ($Mode -eq "negative") {
    Add-Content -Path $Served -Value ([byte]0x78) -AsByteStream
    Log "NEGATIVE mode: serving a TAMPERED .nsis.zip with the genuine signature — expecting REJECTION"
  }

  # ── Local CA + leaf (SAN = the prod host) via openssl (Git ships it on the runner) ──
  Log "generating local CA + leaf (SAN=$FeedHost)"
  & openssl req -x509 -newkey rsa:2048 -nodes -keyout "$Work\ca.key" -out "$Work\ca.pem" -days 2 -subj "/CN=$CaCn" 2>$null
  & openssl req -newkey rsa:2048 -nodes -keyout "$Work\leaf.key" -out "$Work\leaf.csr" -subj "/CN=$FeedHost" 2>$null
  "subjectAltName=DNS:$FeedHost`nextendedKeyUsage=serverAuth" | Set-Content "$Work\leaf.ext"
  & openssl x509 -req -in "$Work\leaf.csr" -CA "$Work\ca.pem" -CAkey "$Work\ca.key" -CAcreateserial -out "$Work\leaf.pem" -days 2 -extfile "$Work\leaf.ext" 2>$null

  # ── Trust the CA (LocalMachine\Root — schannel validates against it for reqwest) + host ──
  # MUST be LocalMachine\Root, NOT CurrentUser\Root: adding a CA to the PER-USER Trusted Root
  # store pops a GUI security-confirmation dialog ("install this certificate?") that hangs a
  # headless runner — regardless of certutil vs Import-Certificate vs X509Store.Add. The MACHINE
  # store is admin-authorized (the GH runner is admin) so it adds with NO prompt; schannel
  # validates against both stores. (Observed freeze on iterations 1-3.)
  Log "trusting CA (Import-Certificate -> LocalMachine\Root) + hosts entry"
  $CaThumb = (Import-Certificate -FilePath "$Work\ca.pem" -CertStoreLocation Cert:\LocalMachine\Root).Thumbprint
  Write-Host "CA trusted (thumbprint $CaThumb)"
  Add-Content -Path $HostsFile -Value "127.0.0.1 $FeedHost"
  Write-Host "hosts entry added"

  # ── Scoped Defender exclusion (the UNSIGNED installer/exe) ──
  Add-MpPreference -ExclusionPath $InstallRoot, $ServeDir -ErrorAction SilentlyContinue

  # ── Synthesize latest.json for N ──
  $latest = [ordered]@{
    version  = $NVersion
    notes    = "updater smoke $NVersion"
    pub_date = "2026-01-01T00:00:00Z"
    platforms = [ordered]@{ $PlatformKey = [ordered]@{ signature = $NSigText; url = "https://$FeedHost/releases/download/$NName" } }
  } | ConvertTo-Json -Depth 6
  Set-Content -Path (Join-Path $Work "latest.json") -Value $latest
  Log "latest.json:"; Write-Host $latest

  # ── Start the local HTTPS feed on :443 (no sudo on Windows) ──
  Log "starting feed server on :443"
  $FeedProc = Start-Process -FilePath "bun" -PassThru `
    -ArgumentList "$RepoRoot\packages\accelerator\scripts\updater-feed-server.ts","--cert","$Work\leaf.pem","--key","$Work\leaf.key","--latest-json","$Work\latest.json","--serve-dir","$ServeDir" `
    -RedirectStandardOutput (Join-Path $Work "feed.log") -RedirectStandardError (Join-Path $Work "feed.err")
  $feedUp = $false
  for ($i = 0; $i -lt 20; $i++) {
    try { Invoke-RestMethod -Uri "https://$FeedHost/releases/latest.json" -TimeoutSec 3 | Out-Null; $feedUp = $true; break } catch { }
    Start-Sleep -Milliseconds 500
  }
  if (-not $feedUp) { Write-Error "feed server not reachable"; Dump-Logs; exit 1 }

  # ── Install N-1 silently (currentUser → %LOCALAPPDATA%, no UAC) ──
  # Timed (not -Wait): a non-silent NSIS prompt would hang the runner forever, so fail fast.
  Log "installing N-1 silently: $N1Installer /S"
  $inst = Start-Process -FilePath $N1Installer -ArgumentList "/S" -PassThru
  if (-not $inst.WaitForExit(120000)) {
    try { $inst.Kill() } catch { }
    Write-Error "N-1 silent install did NOT finish in 120s — a non-silent NSIS prompt? (runner can't click)"
    exit 1
  }
  Write-Host "N-1 installed (exit $($inst.ExitCode))"
  $Exe = Get-ChildItem -Path $InstallRoot -Recurse -Filter "aztec-accelerator.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $Exe) { Write-Error "installed exe not found under $InstallRoot"; exit 1 }

  # ── Pre-seed auto-update so N-1 updates without UI ──
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  '{"config_version":1,"safari_support":false,"approved_origins":[],"speed":"full","auto_update":true}' | Set-Content (Join-Path $ConfigDir "config.json")

  # ── (#96) Arm crash-recovery the way the app does, so the update runs THROUGH the
  #    disarm-before-install guard (updater.rs). The app registers the Task Scheduler task on
  #    startup iff autostart is_enabled(); is_enabled() needs only the Run key under the
  #    productName "Aztec Accelerator" — a missing StartupApproved entry counts as enabled
  #    (auto-launch `unwrap_or(true)`). Positive leg only (the negative leg rejects before install).
  if ($Mode -eq "positive") {
    # (#97) Clear any stale crash-recovery task FIRST, so the poller's "seen present" below provably
    #   reflects THIS run's registration by N-1 (Cleanup's delete only runs in `finally`, after).
    & schtasks /Delete /F /TN $TaskName *> $null
    Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -Value "`"$($Exe.FullName)`""
    Log "armed crash-recovery (stale task cleared; autostart Run key set for 'Aztec Accelerator')"
  }

  # ── Launch N-1; it should auto-update to N and relaunch ──
  Log "launching N-1 (expecting auto-update → $NVersion)"
  $AppProc = Start-Process -FilePath $Exe.FullName -PassThru

  if ($Mode -eq "negative") {
    Log "NEGATIVE: asserting /health never reports $NVersion (tampered artifact rejected), 120s"
    for ($i = 0; $i -lt 60; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
      if ($got -eq $NVersion) { Write-Error "NEGATIVE FAILED — a TAMPERED artifact was ACCEPTED (updated to $NVersion). The updater is not verifying signatures."; Dump-Logs; exit 1 }
      Start-Sleep -Seconds 2
    }
    if (-not (Select-String -Path (Join-Path $Work "feed.log") -Pattern "/releases/download/" -Quiet)) {
      Write-Error "NEGATIVE inconclusive — the updater never downloaded the artifact, so signature rejection wasn't exercised."; Dump-Logs; exit 1
    }
    Log "SUCCESS (negative) — updater downloaded the tampered artifact and refused to update"
    Dump-Logs; exit 0
  }

  # ── Positive: prove the full armed → disarm(sustained-absent during install) → re-arm cycle, via a
  #    DECOUPLED background poller. (codex post-impl: don't couple /Query sampling to /health latency,
  #    and require SUSTAINED absence so a one-off schtasks error can't read as a disarm.)
  $PollState = Join-Path $Work "task-state.txt"
  Set-Content -Path $PollState -Value "0 0"
  $PollJob = Start-Job -ScriptBlock {
    param($tn, $outFile)
    # Sample the task ~every 200ms, independent of /health. Track whether it was ever PRESENT (armed)
    # and the longest run of consecutive ABSENT samples AFTER that (the disarm). A seconds-long
    # install-window absence → a long streak; a single transient schtasks blip → streak 1 (filtered).
    $sawPresent = 0; $streak = 0; $maxStreak = 0
    while ($true) {
      & schtasks /Query /TN $tn *> $null
      if ($LASTEXITCODE -eq 0) { $sawPresent = 1; $streak = 0 }
      elseif ($sawPresent -eq 1) { $streak++; if ($streak -gt $maxStreak) { $maxStreak = $streak } }
      Set-Content -Path $outFile -Value "$sawPresent $maxStreak"
      Start-Sleep -Milliseconds 200
    }
  } -ArgumentList $TaskName, $PollState

  # ── Wait for the update (/health == N). The poller runs independently, so a slow /health never
  #    starves task sampling. 150 * 2s = 300s budget.
  Log "polling $HealthUrl for version == $NVersion (up to 300s); background poller watching '$TaskName'"
  $updated = $false
  for ($i = 0; $i -lt 150; $i++) {
    try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
    if ($got -eq $NVersion) { $updated = $true; break }
    Start-Sleep -Seconds 2
  }
  if (-not $updated) {
    Write-Error "updater smoke failed — /health never reported $NVersion (does Tauri's Windows updater apply the .nsis.zip? see feed/app logs)"; Dump-Logs; exit 1
  }
  if (-not (Select-String -Path (Join-Path $Work "feed.log") -Pattern "/releases/download/" -Quiet)) {
    Write-Error "/health reports $NVersion but the feed log has no download hit — the update didn't flow through our feed."; Dump-Logs; exit 1
  }
  Log "SUCCESS — updated to $NVersion via the local feed (artifact downloaded + relaunched)"

  # ── Read what the poller observed, then stop it. ──
  Stop-Job $PollJob -ErrorAction SilentlyContinue
  $raw = (Get-Content $PollState -Raw -ErrorAction SilentlyContinue); if (-not $raw) { $raw = "0 0" }
  $parts = $raw.Trim() -split '\s+'
  $sawPresent = [int]$parts[0]; $maxAbsentStreak = [int]$parts[1]
  Remove-Job $PollJob -Force -ErrorAction SilentlyContinue; $PollJob = $null

  # (#97) ARMED: the poller must have seen the task PRESENT (N-1 registered it; the stale task was
  #   cleared pre-run, so this is genuinely this run's registration — not a leftover).
  if ($sawPresent -ne 1) {
    Write-Error "(#97) ARMING proof FAILED — '$TaskName' was never observed present; N-1 did not register the crash-recovery task (autostart Run key or startup enable_crash_recovery regressed). The update-while-armed scenario was never set up."; Dump-Logs; exit 1
  }
  # (#97) DISARM: after being present, the task must have been SUSTAINED-absent (>=3 consecutive ~200ms
  #   samples ≈ >=600ms) during the update. The real disarm-before-install window is the whole NSIS
  #   install (seconds); requiring a streak rejects a one-off schtasks error AND proves the guard ran.
  #   If it stayed armed throughout, disable_crash_recovery() never disarmed — the race is live (and the
  #   end-state re-arm check below would still pass, hiding it: the exact #96 gap).
  if ($maxAbsentStreak -lt 3) {
    Write-Error "(#97) DISARM proof FAILED — '$TaskName' was never observed sustained-absent during the update (longest consecutive-absent run: $maxAbsentStreak samples). disable_crash_recovery() did not disarm before install; the half-written-binary race is live."; Dump-Logs; exit 1
  }
  Log "(#97) armed + disarmed — task seen present, then sustained-absent ($maxAbsentStreak samples) across the install"

  # (#96) RE-ARM (durable end-state): the task must be present again after the update.
  $rearmed = $false
  for ($r = 0; $r -lt 10; $r++) {
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) { $rearmed = $true; break }
    Start-Sleep -Seconds 2
  }
  if (-not $rearmed) {
    Write-Error "update succeeded but '$TaskName' is ABSENT — the guard did not re-arm (updater.rs rearm path or N-startup enable_crash_recovery failed)."; Dump-Logs; exit 1
  }
  Log "(#97) re-armed — full armed→disarm→re-arm cycle proven"
  exit 0
}
finally {
  Cleanup
}
