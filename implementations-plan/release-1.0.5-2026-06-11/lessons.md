# Release 1.0.5 — incident log + runbook deltas (2026-06-11)

**Shipped:** the full security hardening (SEC-01..08, deny-by-default + prompt-once) + the q7e3
quality pass, to all users via auto-update. rc.3 dry-run first, then stable. Users were never
exposed during the incident below (the live feed served 1.0.4 throughout).

## Incident: stable cut failed 3× at `Create GitHub Release`

`Missing DARWIN_AARCH64_SIZE — cannot generate latest.json`, deterministic (a rerun + a fresh
dispatch reproduced it).

**Root cause:** the `Flatten and rename artifacts` step `mv`'s every binary out of `artifacts/`
into `release-files/` *before* `Generate latest.json` runs — `artifacts/` holds only the desktop
`.sig` files at that point. The SEC-03 size reads (#341) targeted `artifacts/`, a tree that never
contains binaries when the step executes. The sig reads (older code) work precisely because sigs
stay behind. Fixed in #356: sizes read from `release-files/` by exact release name — the same files
the feed URLs point at.

## Lessons

1. **Stable-only workflow steps are untested by rc dry-runs.** `Generate latest.json` is gated
   `is_prerelease == 'false'`, so #341's size lines shipped without ever executing — the rc.3
   "dress rehearsal" could not catch them. When touching a stable-only step, either dry-run it in
   isolation (extract to a script testable in CI) or accept that the next stable cut is its first
   test and stage the rollback first (we did; it's why users were safe).
2. **Instrument before theorizing.** Pre-fix forensics burned cycles on dead ends (extraction
   races, action SHAs, runner images, byte-level inspection) — all eliminated honestly, none the
   cause. The one-line `find artifacts -type f` dump (#355) answered it in a single run. The local
   "repro" was misleading because it replayed the *pre-flatten* artifact tree; the job's tree at
   that step is post-flatten.
3. **The re-cut footgun has a safe exception:** if the failure happens *after* `Create Git Tag`
   but *before* the GitHub release exists, N-1 still resolves correctly — delete the orphan tag
   and fresh-dispatch. (Reruns reuse the original workflow snapshot, so a workflow fix *requires*
   a fresh dispatch.) The forbidden case remains: never fresh-dispatch once the release exists.
4. **The full-verification watcher earns its keep:** run conclusion → CDN feed poll → per-platform
   completeness (signature+size+url) → HEAD every asset. It caught nothing this time only because
   the failure was upstream; post-S3 it is the only proof the CDN serves a complete feed.
5. **Release notes are template-only** (no changelog section). For behavior-changing releases,
   prepend a "What's new" via `gh release edit` post-cut (done for 1.0.5 — the deny-by-default
   popup needed explaining).
