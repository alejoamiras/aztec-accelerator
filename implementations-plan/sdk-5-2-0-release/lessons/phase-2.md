# Phase 2 lessons — publish dispatch (2026-08-26)

**Outcome: ⛔ BLOCKED on npm credentials. Nothing published, nothing stranded, playground deployed.**

## What happened
Every pre-flight check passed and the dispatch ran on the exact approved SHA. `npm publish` packed
the tarball, then the registry rejected the PUT:

```
npm error 404 The requested resource '@alejoamiras/aztec-accelerator@5.2.0'
              could not be found or you do not have permission to access it.
```

This is the **same signature as 2026-05-27** (`implementations-plan/release-2026-05-27/lessons/
phase-3b.md`), where the cause was an `NPM_TOKEN` that had expired/rotated without the GitHub
secret being updated. npm answers **404, not 403**, for an unauthorized write to a scoped package —
it refuses to confirm the package exists to a caller lacking rights, so *404 on publish reads as a
permission problem, not a missing package*. Do not let the word "not found" send you looking for a
naming or registry-config bug.

## The protocol earned its keep
The plan's hardened rule — **read the registry BEFORE classifying** — ran first and returned
`5.2.0` absent with dist-tags unchanged. That single check ruled out the expensive branch (a
succeeded-but-reported-failed publish, which would have made any redispatch mint `-revision.1`) and
made "stop, don't retry" provably safe rather than merely cautious. Cost: one `npm view`.

## Partial success is real and must be reported precisely
`deploy-app` carries no `needs:` edge on `publish-sdk`, so the playground deployed anyway — the
live bundle serves `VITE_AZTEC_SDK_VERSION:"5.2.0"`. Half the release landed. Reporting "the
release failed" would have been false; the accurate statement is "npm publish blocked, playground
5.2.0 live". (The workflow's top comment claims publish runs before deploy; the DAG does not
enforce it — already ledgered.)

## Owner-only diagnostics (agent must NOT run these)
Repurposing the CI token for out-of-band npm operations, or driving an OTP/2FA flow, is a
hard limit (precedent: `aztec-5.0.1-2026-07-16/lessons/phase-4.md`). The owner's checklist:

```bash
npm whoami                                        # is the local session alive?
npm token list                                    # is the CI token expired/revoked?
npm owner ls @alejoamiras/aztec-accelerator       # does the token's identity still own it?
npm access list packages <scope-or-user>          # write access intact?
```

Two plausible causes beyond simple expiry, both worth checking before minting a replacement:
1. **Granular token expiry** — npm granular tokens carry a maximum lifetime and expire silently;
   the owner confirmed at approval that this is a granular, package-scoped token, which makes a
   time-based expiry the leading hypothesis.
2. **Trusted-publisher enforcement** — if the package has been switched to require trusted
   publishing (or token publishes were disallowed), *valid* tokens start failing exactly this way.
   That would make the fix a workflow change (OIDC trusted publishing), not a new token — and it
   is already the ledgered security follow-up.

Re-dispatch after the fix is safe and needs no version change: nothing was published, so the
derived version is still bare `5.2.0`.

LESSONS_FILE=implementations-plan/sdk-5-2-0-release/lessons/phase-2.md
