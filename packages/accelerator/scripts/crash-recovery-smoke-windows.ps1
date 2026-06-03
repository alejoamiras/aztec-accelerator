#!/usr/bin/env pwsh
# ── Task Scheduler crash-recovery CRUX spike ───────────────────────────────────
# The shipped Windows crash-recovery relies on <RestartOnFailure> (Interval PT1M,
# Count 3) relaunching the app when it dies. Codex (session 019e8e9e) flagged that
# Microsoft's docs DON'T define "failure" in action-exit-code terms — a graceful
# non-zero exit may NOT trigger a restart, which would make the feature broken.
#
# This isolates the RestartOnFailure mechanism and answers, empirically, on a real
# runner:
#   exit0 → must STAY DOWN (clean quit / updater handoff = no relaunch)
#   exit1 → does a graceful non-zero exit trigger a restart?
#   kill  → does an ABNORMAL termination (a real crash) trigger a restart?
#
# Uses a SYSTEM principal (S-1-5-18) so the task runs headless on a CI runner,
# deliberately sidestepping the separate InteractiveToken/desktop question (that's
# about whether the REAL app's task launches at logon on a user's machine — true by
# design there; only the CI runner lacks a guaranteed interactive desktop). The
# RestartOnFailure SETTING behaves independently of the principal, so the crux
# answer transfers.
$ErrorActionPreference = 'Stop'
$Work = Join-Path $env:RUNNER_TEMP ("crsmoke-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $Work | Out-Null

function New-Stub($path, $counter, $mode) {
  $lines = @('@echo off', ">>`"$counter`" echo run")
  switch ($mode) {
    'exit0' { $lines += 'exit /b 0' }
    'exit1' { $lines += 'exit /b 1' }
    'hang'  { $lines += ':loop', 'ping -n 3 127.0.0.1 >nul', 'goto loop' }
  }
  Set-Content -Path $path -Value ($lines -join "`r`n") -Encoding Ascii
}

function Invoke-Case($name, $mode, [bool]$kill) {
  $counter = Join-Path $Work "$name.count"
  New-Item -ItemType File -Force -Path $counter | Out-Null
  $stub = Join-Path $Work "$name.cmd"
  New-Stub $stub $counter $mode
  $task = "CrSmoke-$name-$PID"
  $xmlPath = Join-Path $Work "$name.xml"
  # Mirrors the real task_xml restart contract (RestartOnFailure PT1M / Count 3).
  $xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Principals><Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Actions Context="Author"><Exec><Command>cmd.exe</Command><Arguments>/c "$stub"</Arguments></Exec></Actions>
</Task>
"@
  [System.IO.File]::WriteAllText($xmlPath, $xml, [System.Text.Encoding]::Unicode)
  & schtasks /Create /F /TN $task /XML $xmlPath | Out-Null
  & schtasks /Run /TN $task | Out-Null

  if ($kill) {
    Start-Sleep -Seconds 6   # let the action start looping, then crash it
    $procs = Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" |
             Where-Object { $_.CommandLine -like "*$stub*" }
    foreach ($p in $procs) { & taskkill /F /T /PID $p.ProcessId 2>$null | Out-Null }
    Write-Host "[$name] force-killed $($procs.Count) action process(es)"
  }

  # Restart (if any) fires after the PT1M interval, so poll past 60s with a deadline.
  $deadline = (Get-Date).AddSeconds(115)
  $runs = 0
  while ((Get-Date) -lt $deadline) {
    $runs = (Get-Content $counter -ErrorAction SilentlyContinue | Measure-Object -Line).Lines
    if ($runs -ge 2) { break }
    Start-Sleep -Seconds 5
  }
  & schtasks /End /F /TN $task 2>$null | Out-Null
  & schtasks /Delete /F /TN $task 2>$null | Out-Null
  return [int]$runs
}

Write-Host "== running crux cases (each waits past the PT1M restart floor) =="
$r0 = Invoke-Case 'exit0' 'exit0' $false
$r1 = Invoke-Case 'exit1' 'exit1' $false
$rk = Invoke-Case 'kill'  'hang'  $true

Write-Host ""
Write-Host "===== RESULTS ====="
Write-Host "exit0 runs=$r0   (expect 1 = stays down on clean quit/updater handoff)"
Write-Host "exit1 runs=$r1   (>=2 => RestartOnFailure fires on a graceful non-zero exit)"
Write-Host "kill  runs=$rk   (>=2 => RestartOnFailure fires on an abnormal kill = real crash)"
$crash = if ($rk -ge 2) { 'WORKS' } else { 'BROKEN' }
$quit  = if ($r0 -lt 2) { 'OK' } else { 'UNEXPECTED' }
Write-Host "VERDICT crash->relaunch:  $crash"
Write-Host "VERDICT quit->stays-down: $quit"
# Diagnostic only for now — never fail the job; we're learning the mechanism.
Write-Host "::notice::crash=$crash quit=$quit (exit0=$r0 exit1=$r1 kill=$rk)"
