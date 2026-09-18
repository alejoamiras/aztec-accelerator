# C1 (workflow-input-hardening / F-006) — audit trail

## GATE 1 plan-audit (Codex gpt-5.6-sol@xhigh): REJECT → folded
See clusters/C1-audit-codex.md (merged with C1) for adopted/deferred.

## GATE 3 post-impl audit: APPROVE-WITH-NITS → 2 nits folded
```
Verdict: **APPROVE-WITH-NITS**

- F-006 is closed: no `inputs.dist_tag` or `steps.*.outputs.*` expression remains inside any `run:`/Node body. Values enter through `env:` and every shell use is double-quoted ([lines 94–108](<.github/workflows/_publish-sdk.yml:94>), [118–140](<.github/workflows/_publish-sdk.yml:118>)). Line 126’s `if:` expression is structural GitHub evaluation, not shell injection.
- Validator is first, before checkout ([lines 45–55](<.github/workflows/_publish-sdk.yml:45>)). `LC_ALL=C`, anchored Bash matching, and env routing correctly reject empty, multiline, non-ASCII, leading-dash, numeric-semver, whitespace, and shell payloads.
- `--tag="$DIST_TAG"` becomes one `--tag=value` argv element and is sufficient against option reinterpretation ([line 123](<.github/workflows/_publish-sdk.yml:123>)).
- Publish, `dist-tag add`, git tag/push, and `gh release create` retain equivalent arguments. `process.env.SDK_VERSION` preserves the Node assignment without JS interpolation ([line 108](<.github/workflows/_publish-sdk.yml:108>)). `${SDK_VERSION}` inside double quotes is correct.
- Nit: the “no semver-like” claim is overstated. `v1.2.3`, `v1.4`, `x`, and `x.x` pass the regex at [line 50](<.github/workflows/_publish-sdk.yml:50>); npm later rejects them as valid SemVer ranges. This causes late failure, not injection.
- New low-impact nit: malformed multiline input is echoed raw into GitHub’s workflow-command stream ([line 51](<.github/workflows/_publish-sdk.yml:51>)), permitting annotation/log-command injection. The step still fails before checkout or secrets, so F-006 is not reopened.

`actionlint` passes. Deferred items were not counted. The supplied cwd is on another branch; this verdict targets the clean linked worktree for `sechard/workflow-input-hardening` at `731be5b`.```

Nits folded: (1) dropped raw DIST_TAG from the ::error:: echo (log-command-injection); (2) corrected the 'no semver-like' comment (letter-led regex; semver-range slips → fails later at npm, not injection).
