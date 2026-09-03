<#
  Release-time updater smoke test (Windows / NSIS) — release-blocking.

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
    -NArtifactsDir : dir with N's *-setup.nsis.zip, .sig, and pre-signed smoke-latest.json
    -N1Installer   : path to N-1's *-setup.exe
  UPDATER_SMOKE_MODE = positive (default) | negative (tamper the served zip, expect rejection)
                     | barrier  (piece-3 L8: hold the update open mid-NSIS via the sentinel baked
                       into N's NSIS_HOOK_PREINSTALL — a PRE-MUTATION barrier at the top of
                       Section Install (2.8.1 silent updates never run the old uninstaller);
                       assert the txn mid-flight (marker+handoff live, no token), the old exe
                       intact, and a second instance Q fully suppressed — no heal, no rearm, no
                       second download — then release and finish positive. Requires
                       UPDATER_SMOKE_BARRIER_PREFIX (run-unique prefix, baked at N build time).
                     | copy-initiator (arc-hunt r2 F2 regression: a COPY of the install drives the
                       update while the installed instance is closed and the copy REMAINS on disk.
                       The marker must record the INSTALLER's destination, not the copy's path —
                       pre-fix, N saw a path mismatch it could not prove absent and suppressed
                       reconciliation, stranding marker+handoff+token and skipping heal/rearm.
                       Also proves the F1 ownership gate: autostart is OFF here, so NO
                       crash-recovery task may exist afterwards.)
#>
param(
  [Parameter(Mandatory)][string]$NVersion,
  [string]$PlatformKey = "windows-x86_64",
  [Parameter(Mandatory)][string]$NArtifactsDir,
  [Parameter(Mandatory)][string]$N1Installer,
  [Parameter(Mandatory)][string]$RepoRoot,
  # When set, /health must report THIS version after N-1 launches, BEFORE any update is expected —
  # proves the installed N-1 actually runs (a wrong fixture or a crashed N-1 otherwise passes the
  # negative leg and fails the positive one confusingly late).
  [string]$N1Version = ""
)
$NBinaryName = "AztecAccelerator.exe"

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
$BarrierPrefix = $env:UPDATER_SMOKE_BARRIER_PREFIX
if ($Mode -eq "barrier" -and -not $BarrierPrefix) {
  Write-Error "barrier mode requires UPDATER_SMOKE_BARRIER_PREFIX (baked into N's PREINSTALL sentinel)"
  exit 1
}
function BarrierFile($suffix) { Join-Path $ConfigDir "$BarrierPrefix-$suffix" }
$QDir = Join-Path $Work "q"
$QProc = $null
$AppProc = $null
$FeedProc = $null
$CaThumb = $null
$PollJob = $null
$TaskName = "Aztec Accelerator Crash Recovery"

function Log($m) { Write-Host "── $m ──" }
Log "mode: $Mode"

function Cleanup {
  if ($QProc) { Stop-Process -Id $QProc.Id -Force -ErrorAction SilentlyContinue }
  if ($AppProc) { Stop-Process -Id $AppProc.Id -Force -ErrorAction SilentlyContinue }
  Get-Process -Name "AztecAccelerator", "aztec-accelerator" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
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
  # Barrier files are run-unique but live under the app's real config dir — never leave them.
  if ($BarrierPrefix) {
    Get-ChildItem -Path $ConfigDir -Filter "$BarrierPrefix-*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
  }
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
  # Byte size of the GENUINE artifact (before any negative-mode tamper) — bound into the signed
  # manifest (F-004 Layer A + SEC-03).
  $NSize = $NZip.Length
  Log "N artifact: $NName ($NSize bytes)"

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

  # ── Stage the pre-signed latest.json for N (F-004 Layer A) ──
  # The isolated release signer created and verified this feed before any smoke job started.
  # This process never receives the production signing key.
  $SignedFeed = Get-ChildItem -Path $NArtifactsDir -Recurse -Filter "smoke-latest.json" | Select-Object -First 1
  if (-not $SignedFeed) { Write-Error "no pre-signed smoke-latest.json in $NArtifactsDir"; exit 1 }
  $latestObj = Get-Content $SignedFeed.FullName -Raw | ConvertFrom-Json
  $platformNames = @($latestObj.platforms.PSObject.Properties.Name)
  $platform = $latestObj.platforms.$PlatformKey
  $expectedUrl = "https://$FeedHost/releases/download/$NName"
  if ($latestObj.version -ne $NVersion -or $platformNames.Count -ne 1 -or
      $platformNames[0] -ne $PlatformKey -or -not $platform -or
      $platform.signature -ne $NSigText -or $platform.url -ne $expectedUrl -or
      [int64]$platform.size -ne [int64]$NSize -or
      -not $latestObj.manifest -or -not $latestObj.manifest_sig) {
    Write-Error "pre-signed smoke feed does not bind the exact N artifact"
    exit 1
  }
  Copy-Item $SignedFeed.FullName (Join-Path $Work "latest.json")
  Log "latest.json:"; Get-Content (Join-Path $Work "latest.json")

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
  if (-not $feedUp) { Dump-Logs; Write-Error "feed server not reachable"; exit 1 }

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
  $Exe = Get-ChildItem -Path $InstallRoot -Recurse -Filter $NBinaryName -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $Exe) { Write-Error "installed N-1 exe ($NBinaryName) not found under $InstallRoot"; exit 1 }

  # ── Pre-seed auto-update so N-1 updates without UI ──
  New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null
  '{"config_version":1,"https_enabled":false,"approved_origins":[],"speed":"full","auto_update":true}' | Set-Content (Join-Path $ConfigDir "config.json")

  # ── (#96) Arm crash-recovery the way the app does, so the update runs THROUGH the
  #    disarm-before-install guard (updater.rs). The app registers the Task Scheduler task on
  #    startup iff autostart is_enabled(); is_enabled() needs only the Run key under the
  #    productName "Aztec Accelerator" — a missing StartupApproved entry counts as enabled
  #    (auto-launch `unwrap_or(true)`). Positive leg only (the negative leg rejects before install).
  if ($Mode -eq "positive") {
    # (#97) Clear any stale crash-recovery task FIRST and VERIFY it's gone, so the poller's "seen
    #   present" below provably reflects THIS run's registration by N-1 (not a leftover). Cleanup's
    #   delete only runs in `finally`, after.
    & schtasks /Delete /F /TN $TaskName *> $null
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) {
      Dump-Logs; Write-Error "(#97) pre-run cleanup FAILED — a stale '$TaskName' task survived /Delete; cannot prove this run armed it."; exit 1
    }
    # The `Run` key is created lazily by Windows on the first entry — a clean runner may not have it yet, and
    # `Set-ItemProperty` errors "Cannot find path" if the parent key is absent. Create it first (idempotent).
    New-Item -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Force | Out-Null
    Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -Value "`"$($Exe.FullName)`""
    Log "armed crash-recovery (stale task cleared + verified gone; autostart Run key set)"

    # (#97) Start the task-state poller BEFORE launching N-1, so Start-Job's startup latency can't miss
    #   the first-present observation or the disarm window (first update check is 5s post-launch,
    #   main.rs:436). It emits "<sawPresent> <maxConsecutiveAbsentAfterPresent>" each ~200ms; the parent
    #   reads the last value via Receive-Job (no file/torn-read race).
    $PollJob = Start-Job -ScriptBlock {
      param($tn)
      $sawPresent = 0; $streak = 0; $maxStreak = 0
      while ($true) {
        & schtasks /Query /TN $tn *> $null
        if ($LASTEXITCODE -eq 0) { $sawPresent = 1; $streak = 0 }
        elseif ($sawPresent -eq 1) { $streak++; if ($streak -gt $maxStreak) { $maxStreak = $streak } }
        "$sawPresent $maxStreak"
        Start-Sleep -Milliseconds 200
      }
    } -ArgumentList $TaskName

    # Barrier: Start-Job is async — wait until the poller has actually emitted a sample (i.e. is
    #   sampling) before launching, so it can't miss the early task lifecycle. `-Keep` peeks without
    #   consuming (the final Receive-Job still sees every sample). Best-effort + bounded (~5s); the
    #   disarm is 5s out, so even a slow start has margin.
    for ($w = 0; $w -lt 50; $w++) {
      if (@(Receive-Job $PollJob -Keep -ErrorAction SilentlyContinue).Count -gt 0) { break }
      Start-Sleep -Milliseconds 100
    }
  }

  # ── BARRIER pre-launch: the sentinel (baked into N's PREINSTALL) only engages when the
  #    request file exists, and Q must be a copy of the PRE-update install (after the update starts,
  #    $InstallRoot is mid-transition).
  if ($Mode -eq "barrier") {
    Set-Content -Path (BarrierFile "request") -Value "armed"
    Copy-Item -Recurse $InstallRoot $QDir
    Log "BARRIER: request seeded (prefix $BarrierPrefix); Q staged from the pre-update install"
  }

  # ── Launch N-1; it should auto-update to N and relaunch ──
  Log "launching N-1 (expecting auto-update → $NVersion)"
  $AppProc = Start-Process -FilePath $Exe.FullName -PassThru

  # ── N-1 launch proof: the installed fixture must actually RUN and report ITS version before any
  #    update. Seeing N here without ever seeing N-1 means the fixture is wrong (or already N) and
  #    the "update happened" tail would pass vacuously.
  if ($N1Version) {
    $sawN1 = $false
    for ($i = 0; $i -lt 60; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 2).version } catch { $got = $null }
      if ($got -eq $N1Version) { $sawN1 = $true; break }
      if ($got -eq $NVersion) { Dump-Logs; Write-Error "N-1 launch proof FAILED — /health reported N ($NVersion) before ever reporting N-1 ($N1Version); wrong fixture or no real update is being exercised."; exit 1 }
      Start-Sleep -Milliseconds 500
    }
    if (-not $sawN1) { Dump-Logs; Write-Error "N-1 launch proof FAILED — /health never reported $N1Version within 30s; the installed N-1 did not run (crash on startup? wrong artifact?)"; exit 1 }
    Log "N-1 alive at $N1Version"
  }

  if ($Mode -eq "negative") {
    Log "NEGATIVE: asserting /health never reports $NVersion (tampered artifact rejected), 120s"
    for ($i = 0; $i -lt 60; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
      if ($got -eq $NVersion) { Dump-Logs; Write-Error "NEGATIVE FAILED — a TAMPERED artifact was ACCEPTED (updated to $NVersion). The updater is not verifying signatures."; exit 1 }
      Start-Sleep -Seconds 2
    }
    if (-not (Select-String -Path (Join-Path $Work "feed.log") -Pattern "/releases/download/" -Quiet)) {
      Dump-Logs; Write-Error "NEGATIVE inconclusive — the updater never downloaded the artifact, so signature rejection wasn't exercised."; exit 1
    }
    # Rejecting is only proven by a LIVE N-1 still reporting its own version — a crash after the
    # download would also "never report N" and pass vacuously.
    if ($N1Version) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
      if ($got -ne $N1Version) { Dump-Logs; Write-Error "NEGATIVE inconclusive — N-1 is not alive at $N1Version after the rejection window (got '$got'); it may have crashed rather than refused."; exit 1 }
    }
    Log "SUCCESS (negative) — updater downloaded the tampered artifact and refused to update"
    Dump-Logs; exit 0
  }

  if ($Mode -eq "copy-initiator") {
    # ── Arc-hunt r2 F2 regression. N-1 is installed but NEVER launched from $InstallRoot; a COPY
    #    drives the update instead, and the copy stays on disk throughout (so the marker's
    #    proven-absent rename-tolerance can't mask a wrong expected path). ──
    if ($AppProc -and -not $AppProc.HasExited) { Stop-Process -Id $AppProc.Id -Force; $AppProc.WaitForExit() }
    $AppProc = $null
    Get-Process -Name "AztecAccelerator" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    # Autostart must be OFF for this leg (no Run value armed) — that is exactly the case an
    # ownership gate on the Run value alone would leave open, and it must still not leave a task.
    Remove-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -ErrorAction SilentlyContinue
    & schtasks /Delete /F /TN $TaskName *> $null
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) { Dump-Logs; Write-Error "copy-initiator precondition FAILED — '$TaskName' survived pre-run delete; a later 'no task' assert would be meaningless."; exit 1 }

    Copy-Item -Recurse -Force $InstallRoot $QDir
    $CopyExe = Get-ChildItem -Path $QDir -Recurse -Filter $NBinaryName | Select-Object -First 1
    if (-not $CopyExe) { Write-Error "copy-initiator — copied exe ($NBinaryName) not found under $QDir"; exit 1 }
    # Baseline the marker-armed log count BEFORE the copy runs: the installed N-1 was launched
    # earlier for the launch proof and its own 5s update poll may already have armed a marker, and
    # daily log files persist — so mere PRESENCE of the line could come from P, not the copy
    # (r4 #4). Only an INCREASE attributable to the copy counts.
    $AppLogGlob = "$env:LOCALAPPDATA\aztec-accelerator\logs\*.log"
    $MarkerLogBefore = @(Select-String -Path $AppLogGlob -Pattern "update window marker armed" -ErrorAction SilentlyContinue).Count
    # Hash the copy BEFORE the update: on the dispatch path the copy has the SAME file name as N,
    # so "is there an AztecAccelerator.exe beside the copy" is trivially true and cannot detect an
    # install into the copy's dir. The copy's BYTES staying N-1's is what proves it (the first
    # version of this assert was name-based and failed on the copy's own existence).
    $CopyHashBefore = (Get-FileHash $CopyExe.FullName -Algorithm SHA256).Hash
    Log "copy-initiator: launching the COPY ($($CopyExe.FullName)); installed dir left closed"
    $QProc = Start-Process -FilePath $CopyExe.FullName -PassThru

    # The copy must run (it owns :59833 now) and then drive the update to completion.
    $sawCopy = $false
    for ($i = 0; $i -lt 60; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 2).version } catch { $got = $null }
      if ($got -eq $N1Version) { $sawCopy = $true; break }
      Start-Sleep -Milliseconds 500
    }
    if (-not $sawCopy) { Dump-Logs; Write-Error "copy-initiator FAILED — the copied instance never served /health == $N1Version."; exit 1 }
    $updated = $false
    for ($i = 0; $i -lt 150; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
      if ($got -eq $NVersion) { $updated = $true; break }
      Start-Sleep -Seconds 2
    }
    if (-not $updated) { Dump-Logs; Write-Error "copy-initiator FAILED — /health never reported $NVersion; the copy-driven update did not complete."; exit 1 }

    # The transaction must have EXISTED: "files absent afterwards" alone also passes if marker
    # creation was skipped entirely and the install proceeded (r3 #4). updater.rs logs
    # "update window marker armed" on successful create — require it.
    $MarkerLogAfter = @(Select-String -Path $AppLogGlob -Pattern "update window marker armed" -ErrorAction SilentlyContinue).Count
    if ($MarkerLogAfter -le $MarkerLogBefore) {
      Dump-Logs; Write-Error "copy-initiator INCONCLUSIVE — no NEW 'update window marker armed' line ($MarkerLogBefore -> $MarkerLogAfter): the copy never created a marker, so the reconciliation assert below would pass vacuously."; exit 1
    }
    # THE regression assert: N (relaunched from the INSTALL dir) reconciled the transaction away.
    # Pre-fix the marker held the copy's path, N's exe_canon differed, the copy still existed
    # (so absence was not provable) and reconciliation was suppressed — leaving these behind.
    foreach ($f in @("update-in-progress.json", "update-txn", "update-txn-done")) {
      if (Test-Path (Join-Path $ConfigDir $f)) { Dump-Logs; Write-Error "copy-initiator FAILED — $f survived; the marker's expected_install_path did not match the installer's destination (arc-hunt r2 F2 regressed)."; exit 1 }
    }
    # The copy must STILL exist (otherwise proven-absence could have rescued a wrong path and the
    # assert above would pass for the wrong reason).
    if (-not (Test-Path $CopyExe.FullName)) { Dump-Logs; Write-Error "copy-initiator INCONCLUSIVE — the copied exe vanished; proven-absence could have masked a wrong expected path."; exit 1 }
    # N must be installed (and running) at the INSTALL dir, not beside the copy.
    $NewExe = Get-ChildItem -Path $InstallRoot -Recurse -Filter $NBinaryName -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $NewExe) { Dump-Logs; Write-Error "copy-initiator FAILED — $NBinaryName absent from $InstallRoot; the installer did not target the install dir."; exit 1 }
    if ((Get-FileHash $CopyExe.FullName -Algorithm SHA256).Hash -ne $CopyHashBefore) {
      Dump-Logs; Write-Error "copy-initiator FAILED — the copy's bytes changed: the installer wrote N into the copy's directory ($QDir) instead of the install dir."; exit 1
    }
    # F1 ownership gate: autostart is OFF and no process owns an entry, so nothing may have armed
    # crash recovery — least of all the copy.
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) { Dump-Logs; Write-Error "copy-initiator FAILED — '$TaskName' exists although autostart was OFF; an implicit arm fired (arc-hunt r2 F1 regressed)."; exit 1 }
    Log "SUCCESS (copy-initiator) — copy-driven update installed at the install dir, transaction fully reconciled with the copy still present, and no crash-recovery task armed."
    exit 0
  }

  if ($Mode -eq "barrier") {
    # ── The window OPENS when N's installer reaches the injected NSIS_HOOK_PREINSTALL at the top
    #    of Section Install and parks on the release file. This is a PRE-MUTATION barrier: marker
    #    live, completion token not yet written, the OLD exe still on disk. (2.8.1 silent updates
    #    never run the old uninstaller — PageLeaveReinstall's update-mode guard, measured in run
    #    30472678896 — so N's own installer is the only mid-update actor to park.) ──
    Log "BARRIER: waiting for the sentinel (N's PREINSTALL) to signal ready (240s)"
    $ready = $false
    for ($i = 0; $i -lt 240; $i++) {
      if (Test-Path (BarrierFile "ready")) { $ready = $true; break }
      Start-Sleep -Seconds 1
    }
    if (-not $ready) {
      Dump-Logs; Write-Error "BARRIER FAILED — the sentinel never signalled ready: N's injected NSIS_HOOK_PREINSTALL did not run (injection regressed, the update never started, or the template moved the hook)."; exit 1
    }
    Log "BARRIER OPEN — measuring the mid-update world"
    $Marker = Join-Path $ConfigDir "update-in-progress.json"
    $FeedLog = Join-Path $Work "feed.log"

    # 1. Phase proof — this really is the pre-mutation point: the sentinel (inside N's installer,
    #    before any File copy) measured the OLD exe still present, and it is byte-identical to the
    #    pre-update copy we staged for Q (nothing has been rewritten yet).
    $pStatus = ""
    try { $pStatus = (Get-Content (BarrierFile "p-status") -Raw).Trim() } catch { }
    if ($pStatus -ne "present") {
      Dump-Logs; Write-Error "BARRIER FAILED — sentinel measured P as '$pStatus' (expected 'present' at PREINSTALL, before any File copy); the park point is not where we think it is."; exit 1
    }
    $QExe = Get-ChildItem -Path $QDir -Recurse -Filter $NBinaryName | Select-Object -First 1
    if (-not $QExe) { Write-Error "BARRIER — staged Q exe not found under $QDir"; exit 1 }
    if ((Get-FileHash $Exe.FullName -Algorithm SHA256).Hash -ne (Get-FileHash $QExe.FullName -Algorithm SHA256).Hash) {
      Dump-Logs; Write-Error "BARRIER FAILED — installed exe already differs from the pre-update copy inside the window; mutation began before the barrier (park point too late)."; exit 1
    }
    # 2. The real production transaction is mid-flight: marker live, handoff written, token NOT yet
    #    (POSTINSTALL runs after the copies — a token here would mean the barrier isn't holding).
    if (-not (Test-Path $Marker)) { Dump-Logs; Write-Error "BARRIER FAILED — update-in-progress.json absent inside the window; N-1 (current ref) did not write the marker before install()."; exit 1 }
    if (-not (Test-Path (Join-Path $ConfigDir "update-txn"))) { Dump-Logs; Write-Error "BARRIER FAILED — the update-txn handoff is absent inside the window."; exit 1 }
    if (Test-Path (Join-Path $ConfigDir "update-txn-done")) { Dump-Logs; Write-Error "BARRIER FAILED — the completion token already exists INSIDE the window; POSTINSTALL ran, so the barrier is not holding the install open."; exit 1 }
    # 3. :59833 must go dark (N-1 exits for the install) — precondition that makes Q's /health
    #    binding below attributable to Q and only Q. POLLED, not one-shot: install() spawns the
    #    installer BEFORE N-1's process::exit, so the sentinel can signal ready while N-1 is
    #    still dying.
    $portFree = $false
    for ($i = 0; $i -lt 30; $i++) {
      try { Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 2 | Out-Null } catch { $portFree = $true; break }
      Start-Sleep -Milliseconds 500
    }
    if (-not $portFree) { Dump-Logs; Write-Error "BARRIER FAILED — :59833 still serving 15s into the window; N-1 did not exit for the install."; exit 1 }
    # Baselines for the D22 no-second-download proof. $AppLogGlob covers N-1's and Q's shared log.
    $AppLogGlob = "$env:LOCALAPPDATA\aztec-accelerator\logs\*.log"
    $K1 = @(Select-String -Path $FeedLog -Pattern "/releases/latest.json" -ErrorAction SilentlyContinue).Count
    $D1 = @(Select-String -Path $FeedLog -Pattern "/releases/download/" -ErrorAction SilentlyContinue).Count
    $R1 = @(Select-String -Path $AppLogGlob -Pattern "an update window is still live" -ErrorAction SilentlyContinue).Count
    # Seed a STALE (non-resolving) Run value: if Q's heal were NOT suppressed it would rewrite this.
    $StaleVal = '"C:\nonexistent\updater-smoke-stale\AztecAccelerator.exe"'
    Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -Value $StaleVal
    # Precondition: no crash-recovery task exists (barrier mode never armed one).
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) { Dump-Logs; Write-Error "BARRIER precondition FAILED — '$TaskName' already exists before Q; the no-rearm assertion would be meaningless."; exit 1 }

    # ── Q: a second instance launched INSIDE the window (from a pre-update copy). Every suppression
    #    must hold: no heal, no rearm, no marker removal, no second download — and Q must be ALIVE
    #    (a Q that crashes at startup would pass all "nothing happened" asserts vacuously). ──
    Log "BARRIER: launching Q ($($QExe.FullName))"
    $QProc = Start-Process -FilePath $QExe.FullName -PassThru
    $qUp = $false
    for ($i = 0; $i -lt 120; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 2).version } catch { $got = $null }
      if ($got -eq $N1Version) { $qUp = $true; break }
      Start-Sleep -Milliseconds 500
    }
    if (-not $qUp) { Dump-Logs; Write-Error "BARRIER FAILED — Q never served /health == $N1Version (Q crashed, or startup reconcile misbehaved; the server spawns AFTER reconciliation, so this also proves reconcile completed)."; exit 1 }
    if ($QProc.HasExited) { Dump-Logs; Write-Error "BARRIER FAILED — Q exited after binding /health."; exit 1 }
    # Q's scheduled update check (5s post-launch) must HIT the feed (latest.json count rises) …
    $checked = $false
    for ($i = 0; $i -lt 60; $i++) {
      $K2 = @(Select-String -Path $FeedLog -Pattern "/releases/latest.json" -ErrorAction SilentlyContinue).Count
      if ($K2 -gt $K1) { $checked = $true; break }
      Start-Sleep -Seconds 1
    }
    if (-not $checked) { Dump-Logs; Write-Error "BARRIER FAILED — Q never fetched latest.json; the D22 no-second-update proof was not exercised."; exit 1 }
    # … and D22 must REJECT it: wait for the decision's own log line (updater.rs "an update window
    # is still live; not starting another update") to appear beyond the barrier-open baseline —
    # attributable to Q, since N-1 exited and N hasn't launched — THEN assert no download happened.
    # (An arbitrary sleep instead of this milestone would let a slow regressed download pass.)
    $rejected = $false
    for ($i = 0; $i -lt 60; $i++) {
      $R2 = @(Select-String -Path $AppLogGlob -Pattern "an update window is still live" -ErrorAction SilentlyContinue).Count
      if ($R2 -gt $R1) { $rejected = $true; break }
      Start-Sleep -Seconds 1
    }
    if (-not $rejected) { Dump-Logs; Write-Error "BARRIER FAILED — Q fetched latest.json but never logged the D22 live-window rejection; the no-second-update decision was not observed."; exit 1 }
    # The milestone proves the DECISION was logged; a regression that logs it but still schedules
    # the download asynchronously would pass an immediate sample. Give such a download a bounded
    # window to reach the feed before sampling (arc-hunt round 1).
    Start-Sleep -Seconds 5
    $D2 = @(Select-String -Path $FeedLog -Pattern "/releases/download/" -ErrorAction SilentlyContinue).Count
    if ($D2 -ne $D1) { Dump-Logs; Write-Error "BARRIER FAILED — a SECOND artifact download happened inside the window ($D1 -> $D2); D22 did not reject while the marker is live."; exit 1 }
    # Suppressions: marker untouched, Run value byte-identical (no heal), task still absent (no rearm).
    if (-not (Test-Path $Marker)) { Dump-Logs; Write-Error "BARRIER FAILED — Q REMOVED the live update marker (foreign-window removal; the four-part rule regressed)."; exit 1 }
    $runVal = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator")."Aztec Accelerator"
    if ($runVal -cne $StaleVal) { Dump-Logs; Write-Error "BARRIER FAILED — Q healed the Run value inside the window ('$runVal'); marker suppression of heal regressed."; exit 1 }
    & schtasks /Query /TN $TaskName *> $null
    if ($LASTEXITCODE -eq 0) { Dump-Logs; Write-Error "BARRIER FAILED — Q re-armed '$TaskName' inside the window; marker suppression of rearm regressed."; exit 1 }
    if ($QProc.HasExited) { Dump-Logs; Write-Error "BARRIER FAILED — Q exited during the suppression window (asserts above may be vacuous)."; exit 1 }
    Log "BARRIER: Q fully suppressed (alive, checked feed, no download, no heal, no rearm, marker intact)"
    # Free :59833 BEFORE releasing — the resumed installer relaunches N, which must bind it.
    Stop-Process -Id $QProc.Id -Force; $QProc.WaitForExit(); $QProc = $null

    # ── Release the window. The sentinel must still OWN it: a timed-out sentinel already fell
    #    through, meaning everything above was measured OUTSIDE the window — reject loudly. ──
    if (Test-Path (BarrierFile "timed-out")) { Dump-Logs; Write-Error "BARRIER FAILED — the sentinel timed out before release; the measured window was not held."; exit 1 }
    Set-Content -Path (BarrierFile "release") -Value "go"
    Log "BARRIER RELEASED — expecting the install to complete and N to reconcile the marker away"

    $updated = $false
    for ($i = 0; $i -lt 150; $i++) {
      try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
      if ($got -eq $NVersion) { $updated = $true; break }
      Start-Sleep -Seconds 2
    }
    if (-not $updated) { Dump-Logs; Write-Error "BARRIER tail FAILED — /health never reported $NVersion after release."; exit 1 }
    # Window-end truths: marker/handoff/token all reconciled away; the stale Run value HEALED to the
    # installed exe (window closed ⇒ heal unsuppressed); crash-recovery re-armed (Run key enabled).
    foreach ($f in @("update-in-progress.json", "update-txn", "update-txn-done")) {
      if (Test-Path (Join-Path $ConfigDir $f)) { Dump-Logs; Write-Error "BARRIER tail FAILED — $f still present after N started; startup reconciliation did not remove the transaction."; exit 1 }
    }
    $runVal = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator")."Aztec Accelerator"
    $expectedHeal = '"' + $Exe.FullName + '"'
    if (-not $runVal.Equals($expectedHeal, [System.StringComparison]::OrdinalIgnoreCase)) {
      Dump-Logs; Write-Error "BARRIER tail FAILED — Run value is '$runVal', expected the healed quoted exe $expectedHeal; window-end re-heal regressed."; exit 1
    }
    $rearmed = $false
    for ($r = 0; $r -lt 15; $r++) {
      & schtasks /Query /TN $TaskName *> $null
      if ($LASTEXITCODE -eq 0) { $rearmed = $true; break }
      Start-Sleep -Seconds 2
    }
    if (-not $rearmed) { Dump-Logs; Write-Error "BARRIER tail FAILED — '$TaskName' absent after the window closed; startup rearm did not run."; exit 1 }
    Log "SUCCESS (barrier) — L8 proven at the pre-mutation barrier: txn mid-flight (marker+handoff, no token), old exe intact, Q fully suppressed, D22 held, and the window-end reconcile removed the txn, healed the Run value, and re-armed."
    exit 0
  }

  # ── Positive: the task-state poller is already running (started BEFORE launch, above). Wait for the
  #    update, then read what the poller observed. ──
  Log "polling $HealthUrl for version == $NVersion (up to 300s); background poller watching '$TaskName'"
  $updated = $false
  for ($i = 0; $i -lt 150; $i++) {
    try { $got = (Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 3).version } catch { $got = $null }
    if ($got -eq $NVersion) { $updated = $true; break }
    Start-Sleep -Seconds 2
  }
  if (-not $updated) {
    Dump-Logs; Write-Error "updater smoke failed — /health never reported $NVersion (does Tauri's Windows updater apply the .nsis.zip? see feed/app logs)"; exit 1
  }
  if (-not (Select-String -Path (Join-Path $Work "feed.log") -Pattern "/releases/download/" -Quiet)) {
    Dump-Logs; Write-Error "/health reports $NVersion but the feed log has no download hit — the update didn't flow through our feed."; exit 1
  }
  Log "SUCCESS — updated to $NVersion via the local feed (artifact downloaded + relaunched)"

  # End-state: no update-transaction file survives N's startup. A same-key 3.x N-1's marker must
  # have been reconciled away by now; /health == N happens after startup reconciliation.
  foreach ($f in @("update-in-progress.json", "update-txn", "update-txn-done")) {
    if (Test-Path (Join-Path $ConfigDir $f)) { Dump-Logs; Write-Error "end-state FAILED — $f present after the update; the startup reconcile did not clear the transaction."; exit 1 }
  }

  # End-state: the current executable name is installed.
  $NewExe = Get-ChildItem -Path $InstallRoot -Recurse -Filter $NBinaryName -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $NewExe) { Dump-Logs; Write-Error "end-state FAILED — $NBinaryName not found under $InstallRoot after the update."; exit 1 }
  # End-state: the autostart Run value points at the installed executable, quoted.
  if ($Mode -eq "positive") {
    $endVal = (Get-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run" -Name "Aztec Accelerator" -ErrorAction SilentlyContinue)."Aztec Accelerator"
    $expected = '"' + $NewExe.FullName + '"'
    if (-not $endVal -or -not $endVal.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase)) {
      Dump-Logs; Write-Error "end-state FAILED — Run value is '$endVal', expected the quoted installed exe $expected (arming or startup heal regressed)."; exit 1
    }
  }

  # ── Read what the poller observed: Receive-Job returns every emitted "<sawPresent> <maxStreak>" line;
  #    the LAST is the final state (no file/torn-read race). Default closed to "0 0" → ARMING fails. ──
  Stop-Job $PollJob -ErrorAction SilentlyContinue
  $samples = @(Receive-Job $PollJob -ErrorAction SilentlyContinue)
  Remove-Job $PollJob -Force -ErrorAction SilentlyContinue; $PollJob = $null
  $last = if ($samples.Count -gt 0) { [string]$samples[-1] } else { "0 0" }
  $parts = $last.Trim() -split '\s+'
  $sawPresent = [int]$parts[0]; $maxAbsentStreak = [int]$parts[1]

  # (#97) ARMED: the poller must have seen the task PRESENT (N-1 registered it; the stale task was
  #   cleared pre-run, so this is genuinely this run's registration — not a leftover).
  if ($sawPresent -ne 1) {
    Dump-Logs; Write-Error "(#97) ARMING proof FAILED — '$TaskName' was never observed present; N-1 did not register the crash-recovery task (autostart Run key or startup enable_crash_recovery regressed). The update-while-armed scenario was never set up."; exit 1
  }
  # (#97) DISARM: after being present, the task must have been SUSTAINED-absent (>=3 consecutive ~200ms
  #   samples ≈ >=600ms) during the update. The real disarm-before-install window is the whole NSIS
  #   install (seconds); requiring a streak rejects a one-off schtasks error AND proves the guard ran.
  #   If it stayed armed throughout, disable_crash_recovery() never disarmed — the race is live (and the
  #   end-state re-arm check below would still pass, hiding it: the exact #96 gap).
  if ($maxAbsentStreak -lt 3) {
    Dump-Logs; Write-Error "(#97) DISARM proof FAILED — '$TaskName' was never observed sustained-absent during the update (longest consecutive-absent run: $maxAbsentStreak samples). disable_crash_recovery() did not disarm before install; the half-written-binary race is live."; exit 1
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
    Dump-Logs; Write-Error "update succeeded but '$TaskName' is ABSENT — the guard did not re-arm (updater.rs rearm path or N-startup enable_crash_recovery failed)."; exit 1
  }
  Log "(#97) re-armed — full armed→disarm→re-arm cycle proven"
  exit 0
}
finally {
  Cleanup
}
