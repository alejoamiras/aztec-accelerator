# Observability / diagnostics / support (agent 8)

Setup: tracing → stdout AND daily-rotating file (7 kept) at log_dir() 0700. Default info. Privacy
CLEAN: bb.rs never logs witness bytes; browser errors generic, detail server-side only. NO crash
reporter/telemetry/panic hook anywhere.

1. **[HIGH] "Show Logs" tray item is dev-only; README claims otherwise.** tray.rs:91-113 builds
   show_logs only if dev_mode; production menu omits it. README:320 tells users to use it — false for
   every shipped build. Zero in-app path to own logs. [CORROBORATES agent 1 #4.] Cost: S.
2. **[HIGH] No panic hook; crash recovery restarts but records nothing.** No panic::set_hook. A tray
   app launched without console discards panic stderr — never reaches the tracing file logger.
   crash_recovery relaunches but captures no cause. Field panic / crash-loop completely invisible.
   Cost: M — panic hook that tracing::error!s payload+location before init.
3. **[MED] No self-diagnosis surface in GUI.** settings shows no version/bb-availability/https-live/
   CA-trust. /health (server.rs:319-350) carries it and is reachable via browser but undocumented +
   unlinked. Cost: S/M — Diagnostics panel with Copy.
4. **[MED] No support/bug-report path.** No ISSUE_TEMPLATE/SUPPORT.md; tray "GitHub" opens bare repo.
   Cost: S.
5. **[LOW] Origins persist in logs by design** (auth.rs info-level, 7-day retention) — durable local
   record of which dApps used it, undocumented, no purge control. bb stderr logged at warn (500-char
   trunc) is the one semi-external output on disk. Cost: S doc.
6. **[LOW] Windows log path undocumented** in README troubleshooting. Trivial.

Net: logging solid + privacy-conscious; gap is entirely in getting logs off the machine to a human
(#1 + #2).
