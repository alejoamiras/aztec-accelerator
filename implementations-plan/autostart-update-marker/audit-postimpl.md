# Post-implementation review loop — codex session `019fae68-85b6-7a43-b1f5-db347145d4d8`

Four rounds: audit (REJECT, 4 blockers) → verify (partial: 2 sibling paths incl. the
foreign-marker-delete misclassification) → confirm (2 edge chains: unremovable leftover,
Windows partial create) → final (SAFE TO MERGE). Severity curve: blockers → edge chains →
clear. One residual documented, not fixed: the deterministic production-heal barrier needs a
test lever in shipped code (rejected pattern); lock-exclusion is exercised structurally by T6.

## Round 1 — audit
VERDICT: REJECT — three code blockers and one required-test blocker.

Blocking findings

1. [updater.rs:458] — Both `CreateErr` branches explicitly drop `autostart.lock` before the armed guard drops. A concurrent OFF can therefore land before the stale-snapshot rearm. Worse, `try_create_exclusive` can write complete valid JSON and then fail `sync_all()` ([update_marker.rs:235]); that returns `Io`, then Drop rearms with a live marker of ours. The transaction does not actually track post-publication failure as rev 3 required.

2. [update_marker.rs:420] — Cleanup converts an unreadable current intent into ON. On the install-Err path, intent may have changed to OFF while the first lock was released; a transient read error then removes the marker and arms recovery permanently against OFF intent. This is neither reconciliation to CURRENT intent nor a sanctioned table direction. Unreadable should retain the marker, remain disarmed, and defuse.

3. [update_marker.rs:356] — `canonicalize().ok()` maps every error to `expected_canon: None`; [removal_decision:309] consequently treats permission, sharing, malformed-path, and transient I/O errors as “expected path absent.” That is broader than the settled rename-tolerance clause and can remove from a different path incorrectly. Absence must be distinguished from other canonicalization failures.

4. [update_marker.rs:898] — T4 is not the mandated barrier test: it uses no lock or barrier and drives neither production heal nor `startup_rearm`. T7 likewise calls only the cleanup helper ([update_marker.rs:815]); it does not exercise handoff/install failure orchestration or connect cleanup to actual guard Drop. T6 also omits its specified matching-token Remove flow. These gaps directly missed blockers 1–2.

Non-blocking

- T6’s restorer maps read errors to absence and ignores restore failures ([autostart_heal.rs:755]). For real profile files, capture should fail on non-NotFound errors.

Genuinely sound

The normal `perform_update` sequence, tuple scope through the Err branch, authoritative locked re-checks, and lock ordering are otherwise correct. Reconcile’s failed-rearm, expiry-zero-reconciliation, corrupt suppression, and intent directions match the tables. The NSIS hook ordering/error handling and harness invocation are faithful. Scenario A’s JSON is valid; scenario B’s exact quoted comparison is honest; explicit `exit 0` closes the `$LASTEXITCODE` hazard.
## Round 2 — verification
NOT SAFE TO MERGE — finding 1 is only partially closed.

1. **Partially closed.** The explicit pre-Drop lock releases are gone, and `Live` correctly defuses without rearming into a foreign window. However, [updater.rs:469] still treats every `CreateErr::Io` as potentially post-publication.

   Open sibling paths:

   - If `Io` occurred before any marker was created and intent is unreadable, cleanup keeps nothing and defuses. Recovery remains off with no marker to drive reconciliation.
   - On `create_new`’s second attempt, `AlreadyExists` becomes `Io` without reclassification ([update_marker.rs:212]). A foreign live marker appearing on that retry can therefore enter cleanup, be deleted, and be followed by a rearm.

   `CreateErr` must distinguish pre-publication, our-published marker, and live-foreign retry outcomes. The `Live`-defuse branch itself is the correct conservative direction; its recovery depends on foreign completion or a later launch/expiry reconciliation.

2. **Fully closed.** [update_marker.rs:449] keeps the marker, performs zero arm/disarm calls, and returns for guard defusal when intent is unreadable. T7 pins that behavior.

3. **Fully closed.** Only `try_exists() == Ok(false)` reaches rename tolerance. Stat and canonicalization failures suppress. Expiry now dominates before path verification at [update_marker.rs:357], so an unverifiable path cannot suppress forever; the dedicated expiry test confirms zero reconciliation.

4. **Partially closed; residual acceptable by itself.** T6 now covers matching-token/version/path Remove against default-path files, and the restorer refuses unreadable captures. The deterministic production barrier and actual updater/guard failure orchestration remain absent. The barrier residual is regression exposure, not a present code defect—the locked rechecks are correct by inspection—but the missing orchestration is why finding 1’s sibling paths remain untested.
## Round 3 — confirmation
1. **Finding 1 — still partial.** On first-attempt `AlreadyExists`, `remove_all()` remains unchecked. If removal fails, the same expired/corrupt marker remains; second-attempt `AlreadyExists` is then misclassified as `Live`. `perform_update` defuses, leaving recovery disarmed even though no live foreign window exists.

2. **New defects:** Expiry dominance is correct and introduces none. The split also mislabels Windows `secure_create_file` ACL/readback failures as `NotPublished`; that function can create an empty file before returning an error. This leaves an unexpected corrupt marker, though snapshot rearm remains safe because it cannot contain valid JSON.

3. **Safe to merge: no.**
## Round 4 — final
1. **Closed: yes.** Checked cleanup prevents an unremovable expired/corrupt leftover from becoming `Live`; snapshot rearm is correct.

2. **Closed: yes.** Non-`AlreadyExists` open failures now remove any Windows partial-create residue best-effort, without touching foreign existing files.

3. **Safe to merge: yes.** No new blocking or material residual introduced.