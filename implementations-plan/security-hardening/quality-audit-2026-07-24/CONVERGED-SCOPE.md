# Converged remediation scope — codex + Claude (2026-07-24)

Bar: production app, SAFE FOR USERS. Threat model: primary untrusted actor = remote dApp on
127.0.0.1:59833 (approval ≠ trust); same-user local = out of scope (documented, Unix-0o700 parity);
build-machine/repo-write = supply chain (matters for shipped artifacts). Full audit: ./README.md.

## RELEASE-BLOCKING (must fix before security-hardening → main)
1. **C9 authorize Remember unguarded** — click-steal guard covers Allow/Deny but not Remember → stolen
   click pre-arms persistent grant. Guard+disable Remember while inactive. [remote clickjacking] (mine)
2. **/prove resource limits** — cap per-origin piggyback senders AND add per-origin concurrency +
   subprocess-runtime + output-size + memory bounds (codex: sender-cap alone leaves exhaustion paths).
   [remote DoS from approved origin]
3. **Version-downgrade via x-aztec-version** — remote header picks the bb version. Enforce a safe policy:
   floor at bundled + reject prereleases/syntactic-aliases + opt-in allowlist for vetted older versions
   (NOT a user prompt). Every selectable version must already be safe. [remote → old vulnerable bb + witness]
4. **CWD cache fail-open** — home-resolution failure → CWD fallback lets preseeded bb+marker bypass verify.
   Fail closed (no unowned-dir fallback). [executable-integrity invariant]
5. **Updater rollback-race + bounded streaming** — record_pending fail-open lets a running v1 install v2
   over v3; size cap checked after buffering; feed parsed before size bound. Serialize + bound-during-read.
6. **win_acl owner not verified** — verify (and/or set) object OWNER == current user, not just the DACL
   (a foreign owner keeps implicit WRITE_DAC). [cross-user; Windows builds] (mine, F-003)
7. **C8 rollback destroys recovery / swallows disarm / is_enabled-unknown** — rollback disarms pre-existing
   recovery on re-enable failure (destroys the user's recovery path); disable/quit ignore confirmed-disarm;
   I/O error treated as disabled. Fix transaction to restore prior fully + surface disarm failures. (mine)
8. **C9 arbiter promote-before-build** — promotion can precede window build → server-active-but-unraised
   popup. Order build-before-promote / re-raise. + poll: resume on failure, guard stale overlaps. (mine)
9. **Release-CI**: dispatch-ref secret exposure (protected environments w/ required-reviewer on immutable
   SHA, or restrict), tag==github.sha verification, version-output-injection hardening. [shipped integrity]

## FIX-CHEAP (ships to users; low cost)
- copy-bb: bound expanded size (gzip bomb) + member-type check; absolute-path `xattr` (no bare-name PATH).
- build-frontend: include tool/bundler version in the SHA manifest (drift guard completeness). (mine)
- cert-rotation atomicity (transactional cert+key swap); concurrent cache-staging lock.
- update-prompt: source version from backend VerifiedUpdate (mirror the authorize get_pending_auth fix).

## DOCUMENT-ACCEPTED (out of stated threat model; rationale recorded)
- Cache verify→exec TOCTOU: same-user only; accept IFF cache ancestry is user-owned + not other-user-writable
  (VERIFY that invariant holds, then document). A same-user attacker has strictly easier attacks.
- Linux systemd supervision effectiveness: reliability, not authz/listener exposure. Document + track.

## NEEDS THE OWNER (product/ops — I can't fabricate these)
- **Vetted bb-version policy content** — I'll ship a safe default (bundled-floor + reject-prerelease +
  empty opt-in allowlist). You decide which older versions (if any) to allowlist.
- **Legacy-CA bounded rotation plan** — schedule a signed release that removes the old anchor (anti-rollback
  protected). Interim retention accepted only with that plan.
- **Legacy-role retirement + protected environments** — human-gated infra (C5 runbook); must actually be
  applied (a runbook is not mitigation while creds are live).
