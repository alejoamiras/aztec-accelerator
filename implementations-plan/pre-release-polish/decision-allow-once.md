# Why "allow once" was removed

**Decision**: the site-authorization popup is `[Deny] [Allow]`. Allow is unconditionally permanent.
The "Always allow this site" checkbox is gone, replaced by a line of copy that says approving is
permanent and where to undo it.

**This reverses F-014**, a change a security audit explicitly asked for. That is why this document
exists: a reversal that isn't written down looks like an oversight to the next reader.

## What F-014 decided, and why

Before F-014 the popup had "Remember" **checked by default**. The 2026-07-09 audit
(`audit/security/2026-07-09-5c788c0/raw/frontend-trust-ui-claude.md:32,52`) flagged it:

> *"Authorization: violated — a malicious origin can obtain a persisted (remember is checked by
> default) grant... 'Remember' defaulting to checked removes the natural rate-limit of re-prompting
> on each request."*

F-014 (PR #392, `90f9573`) made it default-**unchecked**, with "Allow once" as the primary action.
Stated threat model: *"the user Allows a malicious look-alike origin… believing it's trusted; or
grants accidental persistent trust."*

## Why it was removed anyway

**"Allow once" was never a session.** With `remember == false`, `core/src/server/auth.rs` persisted
*nothing* — no in-memory table, no TTL, no cookie. The only reason it covered more than one HTTP
request was a piggyback: concurrent `/prove` calls arriving *while the popup was still open* shared
the decision. Once it closed, the next proof re-prompted from scratch. `README.md` claimed "approved
for this session only"; that was false, and is now corrected.

So a regular user of one dApp faced a prompt per proof, forever.

## The argument we did NOT get to use

The first draft of this change argued that "Allow once" therefore wasn't a real control. **Two
independent audits rejected that, and they were right.** Codex:

> *"Re-prompting on every later proof is exactly the audit's rate-limit: it requires renewed user
> presence and intent. Its severity as UX friction proves it functions. Prompt fatigue may make it
> counterproductive, but that is an unmeasured behavioral hypothesis, not evidence that the control
> 'does not function.'"*

That is correct. The friction *is* the mechanism. The reasoning was motivated — it argued toward a
conclusion already chosen.

## The actual trade

A control that works by **attrition** was exchanged for one that works by **disclosure**.

- What is lost: renewed user presence on every later proof. An attacker who wins one Allow click on a
  look-alike origin now gains a *durable* capability — able to return on later visits, exploit a
  future XSS or dependency compromise on that origin, or retain access after the domain changes
  hands, all without another prompt.
- What is gained: one considered decision instead of an endless stream of identical ones. The
  predicted failure of the old model — click-through habituation, or ticking "Always allow" out of
  irritation, both landing on a permanent grant by a worse path — **is a prediction, not a
  measurement.** Recorded as such.
- The owner accepted this trade explicitly, with both audit objections in front of them.

## What has to hold for the trade to be sound

1. **The popup discloses permanence.** *"Stays approved until you remove it in Settings."* This is now
   the primary control. Deliberately not "will be saved": persisting is best-effort — `authorize_origin`
   warns and continues on a config-write error, so an approved proof is never failed by a disk
   problem. The failure mode is being asked again, which is safer than promised, never less safe.
   Pinned by `e2e/authorize.spec.ts`.
2. **The origin display carries the weight.** F-014's treatment — full canonical origin, never
   truncated, `dir=ltr` + bidi isolation, punycode never decoded — is unchanged and still tested.
3. **Revocation is real.** Settings → Approved Sites → Remove. It was promoted to load-bearing by
   this change, and it had *none* of the popup's hardening: `settings.js` rendered origins with a bare
   `textContent`. It now gets the same bidi isolation, `dir=ltr` and selectable text, because two
   visually-confusable rows means removing the wrong one.
4. **Persistence is actually tested.** `allow_persists_the_origin_to_disk` drives a real `/prove`
   through the popup decision and reloads the config **from disk** via an injected temp path. The
   previous coverage was one slow WebDriver test; an in-memory assertion would have passed even when
   the disk write failed.

## What is explicitly NOT claimed

- The verified-sites badge is **not** counted as a control. `VERIFIED_SITES.md` says it is not a
  security guarantee, and it survives DNS or site compromise.
- The 700 ms click-steal guard stops click-stealing, not social engineering or a deliberate click.
- The popup is not the whole boundary: `auth.rs` still auto-approves a request with **no** `Origin`
  header (a non-browser caller, already behind the loopback `Host` allowlist). Unchanged here.

## What would make us revisit

- Evidence that permanent grants are being obtained by look-alike origins in practice.
- A cheap way to make revocation *discoverable* rather than reactive — a user who mis-clicks today
  has no signal unless they open Settings. That is the weakest remaining link, and it is a real gap,
  not a solved problem.
