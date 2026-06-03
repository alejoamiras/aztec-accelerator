#!/usr/bin/env pwsh
# ── Task Scheduler crash-recovery mechanism spike ──────────────────────────────
# Round 1 (RestartOnFailure) was proven BROKEN — it does not relaunch a dead/crashed
# action (exit1=1, kill=1). Codex (019e8e9e) predicted it; a real runner confirmed it.
#
# Round 2 (this): the chosen fix is a REPEATING TimeTrigger (PT1M) + IgnoreNew —
# every minute Task Scheduler tries to start the action; IgnoreNew makes that a no-op
# if it's already running, and a RELAUNCH if it died. This spike confirms the two OS
# behaviors the fix depends on:
#   relaunch → a dead action is restarted by the next tick (runs grows past 1)
#   nodup    → a live action is NOT duplicated by later ticks (runs stays 1)
#
# (quit→stays-down is handled in app code — the quit path calls disable_crash_recovery()
# to delete the task before exit — NOT by the trigger, so it's not spiked here.)
#
# SYSTEM principal so the task runs headless on CI (isolates the trigger/IgnoreNew
# mechanism from the InteractiveToken/desktop question).
$ErrorActionPreference = 'Stop'
$Work = Join-Path $env:RUNNER_TEMP ("crsmoke2-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $Work | Out-Null

function New-Stub($path, $counter, $mode) {
  $lines = @('@echo off', ">>`"$counter`" echo run")
  switch ($mode) {
    'exit'  { $lines += 'exit /b 0' }                                   # dies between ticks
    'hang'  { $lines += ':loop', 'ping -n 3 127.0.0.1 >nul', 'goto loop' }  # stays alive
  }
  Set-Content -Path $path -Value ($lines -join "`r`n") -Encoding Ascii
}

function Register-RepeatingTask($task, $stub) {
  $xmlPath = Join-Path $Work "$task.xml"
  # Repeating TimeTrigger (past StartBoundary + PT1M, no Duration = forever) + IgnoreNew.
  $xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <TimeTrigger>
      <StartBoundary>2024-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <Repetition><Interval>PT1M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>
    </TimeTrigger>
  </Triggers>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Principals><Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Actions Context="Author"><Exec><Command>cmd.exe</Command><Arguments>/c "$stub"</Arguments></Exec></Actions>
</Task>
"@
  [System.IO.File]::WriteAllText($xmlPath, $xml, [System.Text.Encoding]::Unicode)
  & schtasks /Create /F /TN $task /XML $xmlPath | Out-Null
}

function Runs($counter) { (Get-Content $counter -ErrorAction SilentlyContinue | Measure-Object -Line).Lines }

# ── relaunch: stub dies each tick → repetition must restart it (runs grows) ──
$cR = Join-Path $Work 'relaunch.count'; New-Item -ItemType File -Force -Path $cR | Out-Null
$sR = Join-Path $Work 'relaunch.cmd'; New-Stub $sR $cR 'exit'
Register-RepeatingTask 'CrSmoke2-relaunch' $sR
& schtasks /Run /TN 'CrSmoke2-relaunch' | Out-Null     # seed run 1; ticks drive the rest
$deadline = (Get-Date).AddSeconds(185); $rRelaunch = 0
while ((Get-Date) -lt $deadline) { $rRelaunch = Runs $cR; if ($rRelaunch -ge 2) { break }; Start-Sleep 5 }
& schtasks /Delete /F /TN 'CrSmoke2-relaunch' 2>$null | Out-Null

# ── nodup: stub stays alive → IgnoreNew must skip later ticks (runs stays 1) ──
$cN = Join-Path $Work 'nodup.count'; New-Item -ItemType File -Force -Path $cN | Out-Null
$sN = Join-Path $Work 'nodup.cmd'; New-Stub $sN $cN 'hang'
Register-RepeatingTask 'CrSmoke2-nodup' $sN
& schtasks /Run /TN 'CrSmoke2-nodup' | Out-Null
Start-Sleep -Seconds 150                                # span >=2 ticks while it's alive
$rNodup = Runs $cN
& schtasks /End /F /TN 'CrSmoke2-nodup' 2>$null | Out-Null
& schtasks /Delete /F /TN 'CrSmoke2-nodup' 2>$null | Out-Null
Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" |
  Where-Object { $_.CommandLine -like "*$sN*" } |
  ForEach-Object { & taskkill /F /T /PID $_.ProcessId 2>$null | Out-Null }

Write-Host ""
Write-Host "===== RESULTS (repeating-trigger mechanism) ====="
Write-Host "relaunch runs=$rRelaunch   (>=2 => dead action IS relaunched by the repetition)"
Write-Host "nodup    runs=$rNodup   (==1 => live action is NOT duplicated; IgnoreNew works)"
$relOk = if ($rRelaunch -ge 2) { 'WORKS' } else { 'BROKEN' }
$dupOk = if ($rNodup -eq 1) { 'OK' } else { 'DUPLICATED' }
Write-Host "VERDICT relaunch-on-death: $relOk"
Write-Host "VERDICT no-dup-when-alive: $dupOk"
Write-Host "::notice::relaunch=$relOk nodup=$dupOk (relaunch_runs=$rRelaunch nodup_runs=$rNodup)"
