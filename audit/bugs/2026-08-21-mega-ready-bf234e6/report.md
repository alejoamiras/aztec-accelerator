# Cross-OS bug hunt — mega-ready-audit

**Repo:** `alejoamiras/aztec-accelerator` @ `bf234e6` · **Date:** 2026-08-21 · **Model:** ox-alpha solo
**Method:** manual line-by-line reads of every OS-specific surface (autostart backends ×3,
crash-recovery arms ×3, trust backends ×3, updater Windows critical section, NSIS/schtasks XML,
NSS discovery, startup sequencer), driven by the real-user scenario matrix below. Subagent fleet
unavailable this session (4/5 empty returns) — all findings are from direct source reading.

## Scenario matrix → result

| Scenario | Where handled | Result |
|---|---|---|
| Dual-launch at logon (Run key + Task Scheduler both fire) | `main.rs spawn_http_server` structural AddrInUse + kernel owner probe | Sound (Windows bows out only on healthy+not-foreign) |
| Quit must not resurrect | Tray quit disarms (win); mac `KeepAlive{SuccessfulExit:false}` / linux `Restart=on-failure` key on exit code | Sound — clean quit AND `app.restart()` (exit 0/exec) never resurrect |
| Auto-update × crash recovery | Windows: disarm→marker→handoff transaction; macOS/Linux: exit-code semantics make re-arm unnecessary | Sound |
| Relocate app (drag to /Applications, moved install dir) | resolve-based heal (heal iff stored target does not resolve) | Sound (+ real-OS CI suite) |
| Upgrade 1.0.7→2.x | config migration (`safari_support` fold), autostart heal repairs old-exe pointer, legacy-exe prune #455 | Sound (+ packaged-E2E migration leg) |
| Uninstall over a second copy | three-state ownership oracle + task-local recovery check + autostart lock span | Sound; never-armed-copy residual documented |
| Port 59833 occupied by foreign process | bind_with_retry → error surfaced in tray, stays resident | Sound |
| Cert expiry mid-session / ignored renewal window | launch gate resets `https_enabled`; SDK falls back HTTP; renewal re-prompts each launch until expiry | Acceptable degradation, coherent |
| Non-ASCII usernames/install paths | Run-key quoting rules, UTF-16LE+BOM schtasks XML, systemd serializer charset gates | Sound |
| Snap/Flatpak Firefox (Ubuntu default) | `firefox_roots()` covers native + snap + flatpak layouts | Covered |
| Offline bb download | DownloadFailed 500 → SDK WASM fallback | Sound |
| Clock skew | `pending_at` future-tolerance in updater state | Sound |

## Findings

**B-1 (Low, hardening — not fixed, rationale below).** `main.rs:766` propagates a tray-build
failure with `?` out of `.setup()`, which tears down the whole app INCLUDING the already-spawnable
server. A hard tray failure (missing libayatana on exotic Linux setups) currently means "no
accelerator at all" instead of "accelerator without a tray". Not fixed in this engagement: the
fix threads `Option<TrayIcon>` through five call sites in the startup sequencer, and the realistic
trigger set is narrow (shipped Linux artifact is an AppImage that bundles the lib; stock-GNOME
*invisible-tray* builds still succeed). Recorded for a future polish arc; behavior change deserves
its own review cycle, not an audit-drive edit.

**B-2 (Informational).** Windows trust installs only into the CurrentUser Root store. Chrome/Edge
read it natively; Firefox reads it via `security.enterprise_roots.enabled`, default TRUE since
Firefox 68 on Windows. Users who disabled enterprise roots get TLS warnings on :59834 and the SDK
silently falls back to HTTP. Optional future work: detect Firefox-with-enterprise-roots-disabled
and surface a hint in Settings. No action taken.

**B-3 (Informational).** Flatpak Chromium/Brave NSS DBs (`~/.var/app/<app>/.pki/nssdb`) are not
discovered — only flatpak *Firefox* is. Native Chromium browsers use `~/.pki/nssdb` (covered).
Marginal population; note for the verified-stores doc.

## Verdict

No code changes required. Every high-frequency real-user scenario traces to deliberate,
tested handling. The two actionable items (B-1 graceful degradation, B-2 hint) are polish-tier
and belong in the Phase-4 roadmap rather than this engagement's diff.
