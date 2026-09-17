# Desktop runtime / UX (agent 1)

No TODO/FIXME/unimplemented markers in this area — clean, already hardened. Gaps are specific.

1. **[HIGH] Prove/download failures never reach the user.** `core/src/server/prove.rs:25-35`
   StatusGuard::drop → Idle on any exit; `ServerStatus` (server.rs:69-91) has only
   Idle/Downloading/Proving, no error variant. Tray goes Proving→Idle on hard failure, and it also
   overwrites the startup "bb not found" tooltip (main.rs:679-683). ~1 day.
2. **[HIGH] Failed auto-update fails silently.** commands.rs:762-767 closes the prompt window
   regardless of outcome; every updater.rs failure path (:338, :348, :379, :436) logs and returns,
   no UI. User believes they're current forever. 2-4 hrs minimal / ~1 day proper.
3. **[MED] Settings shows DESIRED https state, not LIVE.** settings.js:65-68 reads config.https_enabled;
   main.rs LaunchHttpsGate::UntrustedSkip (:115-118) leaves https_enabled:true while never binding.
   No cue. Few hours.
4. **[MED] No "is it working" surface in release builds.** Show Logs + Versions submenu gated on
   dev_mode (tray.rs:91-113, main.rs:29-32). Settings never shows bb version or health. Promote Show
   Logs ~30min; status line ~2-4hrs.
5. **[MED] No single-instance guard on macOS/Linux.** exit-0-if-healthy is windows-gated
   (main.rs:295-304). Second launch → permanent broken second tray "Error: port in use". <1 hr (drop
   the OS gate, re-verify).
6. **[LOW] Onboarding HTTPS failure copy is raw.** onboarding.js:58 renders `Failed: ${res.https.Err}`
   verbatim (raw OS stderr). 1-2 hrs.

Solid: per-window IPC ACLs, HTTPS lifecycle mutex, auth-popup arbiter, crash recovery.
