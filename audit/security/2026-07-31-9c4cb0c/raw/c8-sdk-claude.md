# c8-sdk — Claude (Opus) raw findings

Three findings. Two of the main agent's Phase-1 leads were REFUTED with reasoning (see NON-FINDINGS).

## F-C8-1 — `/prove` success body read with no size cap, no deadline, no abort path; ky's timeout is already disarmed by then

**Impact.** Availability (primary) — the consuming dApp's entire browser tab (wallet session, PXE
IndexedDB, unsaved state); repeatable, so a persistent silent denial of the proving flow. Secondary
integrity of the UI trust signal: the `"proved"` phase fires with an attacker-supplied duration BEFORE the
body is read. Vector local (bind loopback, no elevation, all three OSes); complexity low; privileges none;
no user interaction beyond normal proving. **Confidence HIGH** — the absence of any bound was verified in
ky's shipped source, not inferred. **CWE-400**, CWE-770; OWASP API4:2023.

**Trace.** Attacker `Response` from the local listener → `accelerator-transport.ts:455` `ky.post(...)` →
only bound requested is `timeout: PROVE_TIMEOUT_MS` (`:456`, `ms("10 min")` at `:28`) → **ky clears its
timer the moment `fetch()` resolves, i.e. AT HEADERS**; the `AbortController` is never armed past that
point (`node_modules/ky/distribution/utils/timeout.js`) → no ambient backstop (`totalTimeout` defaults
false, `Ky.js:283`, never set) → ky's `.json()` override applies ONLY when `options.parseJson` is set
(`Ky.js:454-465`), which the SDK never does, so `res.json` is the raw platform `Response.json()` →
**SINK** `accelerator-prover.ts:508` `(await res.json()) as { proof: string }` — unbounded in bytes AND in
time → second allocation `:510` `Buffer.from(response.proof, "base64")`. The UI is told the proof succeeded
BEFORE the sink, using the attacker's own header (`:502-506`).

**Missing control — everything `/health` has and `/prove` does not.** Byte cap
(`HEALTH_BODY_MAX_BYTES = 64 KB`, `accelerator-transport.ts:26`, enforced chunk-by-chunk `:123-126`);
body deadline (`HEALTH_BODY_TIMEOUT_MS = 2000`, `:25`, timer + in-loop wall-clock `:109-113`);
zero-length-chunk starvation guard (`MAX_EMPTY_CHUNKS = 64`, `:101,115-121`); content-type check; and any
caller-supplied `AbortSignal` — `postProve` (`:450-467`) accepts and forwards none, so a dApp cannot
cancel a hung proof at all. The `/health` bound was added deliberately (`:21-24`: *"a responder that
returns 200 and then stalls (or streams forever) would otherwise hang the probe indefinitely and buffer
unbounded bytes (post-impl codex High)"*) — **the identical threat on the far larger `/prove` body was not
carried over. The asymmetry is the evidence this is an oversight, not an accepted residual.**

**Exploit.** Squat `127.0.0.1:59833` while the app isn't holding it → answer `/health` with exactly the
2-field contract body (`isRecognizedHealthBody`, `:52-56`, whose own comment concedes it is "collision
resistance, not authentication") → HTTPS `:59834` unbound so the HTTPS probe rejects in ~0 ms and
`#probePreferHttps` returns the healthy HTTP result (`:383-392`) → pin `http` → dApp calls
`createChonkProof`, witness POSTed → attacker replies `200`, `x-prove-duration-ms: 1`,
`Transfer-Encoding: chunked`, then emits 1 MB chunks forever (or headers and never a byte) → UI renders
"Proved natively in 1 ms" then `:508` blocks forever (promise never settles, transaction flow wedged with
a success-looking UI) or the tab OOMs and Chrome kills it, taking the wallet session. Neither outcome
reaches the `catch (decodeErr)` at `:485-493`, so the documented fail-safe contract (README:157,
SKILL.md:147) does not hold on the one path an attacker controls.

**Why mitigations fail.** ky's `timeout` is disarmed at headers (verified). ky's error-body cap
(`maxErrorResponseBodySize = 10 MB`, `Ky.js:16,534`) is reached only via `#getResponseData` on the
NON-2xx path (`Ky.js:143`) — a 200 never touches it. `redirect:"error"` stops forwarding, irrelevant to a
directly served body. The WASM-fallback `catch` catches THROWS; a stalled stream throws nothing and a
huge-but-valid stream throws nothing until allocation fails, by which point the tab is gone.

**Instances.** `accelerator-prover.ts:508` (sink), `:484` (primary `#decodeProof` call), `:448`
(HTTP-demotion retry call, identical exposure), `accelerator-transport.ts:450-467` (`postProve`, the
enclosing request that should carry the cap/deadline/signal).

## F-C8-2 — `/health` JSON is type-ASSERTED, not validated, so attacker-controlled values escape into the public `AcceleratorStatus` typed as `string`/`string[]`

**Impact.** Integrity — the published type contract is false → Confidentiality/Integrity at the consumer
when rendered. The SDK's own docs point consumers at these fields for UI (README:63, :117-122,
SKILL.md:94-108), and an Aztec dApp's origin is the one holding the embedded wallet. Vector local;
complexity low; privileges none. **Confidence HIGH** for the in-cluster violation (unvalidated data emitted
under a lying type); **MODERATE** for the downstream XSS — it depends on the consumer choosing
`innerHTML`, and no first-party consumer does (`packages/playground/src/aztec.ts:193` constructs the prover
but never calls `checkAcceleratorStatus`), so no proven end-to-end script execution is claimed.
**CWE-20**, CWE-1287; downstream CWE-79. OWASP A03:2021.

**Trace.** `JSON.parse` with no schema (`accelerator-transport.ts:139`, `:81`) → gate checks ONLY two
fields (`:52-56` `b.status === "ok" && b.api_version === 1`) → `accelerator-prover.ts:211` gate passes →
**validation gap** `:218` `const data = body as { aztec_version?: string; available_versions?: string[] }`
— a bare `as`, zero runtime effect → reads `:247-248` → truthiness branch `:251` (any non-empty
string/non-zero number enters the multi-version path) → `:252` `availableVersions.includes(...)` where
`String.prototype.includes` silently substitutes for the array method, and a number/object throws →
attacker values logged verbatim `:253-258` → **SINK** returned as `AcceleratorStatus` `:259-266`,
`:280-286`, `:289`, whose contract promises `string`/`string[]` (`types.ts:66-69`, `:93`, re-exported
`index.ts:2-9`).

**Missing control.** No runtime narrowing between the gate and emission: no `typeof === "string"`, no
`Array.isArray(...) && every(...)`, no length cap, no charset restriction matching the accelerator's own
`is_valid_version` policy — which the SDK explicitly references at `accelerator-prover.ts:547-549` but
applies only to the version it SENDS, never to versions it RECEIVES and republishes. Also absent: any
README/SKILL statement that these fields are attacker-influenced and must be escaped before rendering.

**Exploit.** Same port-squat precondition. Serve
`{"status":"ok","api_version":1,"aztec_version":"<img src=x onerror=…>","available_versions":{"toString":"x"}}`
→ gate passes → `available_versions` is a truthy object → `.includes` is not a function → TypeError →
blanket `catch` at `:227` → **status silently becomes `offline` and the pin is cleared: a hostile responder
can force the SDK to permanently misreport a running accelerator as offline and downgrade every proof to
WASM, attributing it to "both probes failed"** (fully concrete, in-cluster availability sub-case). For the
XSS variant send a valid `available_versions` array and put the payload in `aztec_version`; a consumer
following README:63 / SKILL.md:105 with `el.innerHTML = \`Accelerator v${status.acceleratorVersion}\``
executes attacker script in the wallet origin. The string-typed variant (`"available_versions":"5.0.1"`)
is quieter: `"5.0.1".includes("5.0.1")` → true → `needsDownload:false`, and a consumer doing
`.map(...)`/`.join(", ")` over a declared `string[]` gets per-character iteration or a throw.

**Why mitigations fail.** `isRecognizedHealthBody` by construction checks two fields (it was hardened to
stop a foreign responder winning the PROTOCOL PIN, `:47-51`, not to sanitize other keys); it does correctly
reject arrays and `null` (`:53`), so prototype-shape tricks fail, but every unlisted key passes untouched.
`readJsonBounded` caps bytes/time, not types — 64 KB is ample for any payload. TypeScript's `as` is erased;
the consumer's `strict` mode is actively HARMFUL here because it asserts these values are safe.
Origin-gated `/health` detail lives in the REAL accelerator (`core/src/server.rs:325` vs `:343-346`); an
impersonator implements neither gate. Prototype pollution checked and not reachable (`JSON.parse` makes
`__proto__` an own data property; the body is never spread/merged).

**Instances.** `accelerator-prover.ts:218` (root cause), `:247-248`, `:251-252`, `:259-266`, `:280-286`,
`:289`; `types.ts:66-69`, `:93`.

## F-C8-3 — Private witnesses transmitted in cleartext by default, to an endpoint address the caller may set to any host, with no loopback constraint and no URL parsing

**Impact.** Confidentiality of private transaction witnesses — the whole product's security property on a
privacy chain. Blast radius: every transaction, every user, for as long as the misconfiguration or
impersonating listener persists. Vector LOCAL for the port-squat facet, NETWORK/ADJACENT for the `host`
facet where the payload leaves the machine entirely. Complexity low; privileges none; no user interaction.
**Confidence MODERATE** — the code facts are certain; the rating reflects that part of this is a
deliberate, partly-documented trade-off (see below for precisely which part is NOT covered).
**CWE-319**, CWE-940, CWE-1188, CWE-20. OWASP A02:2021, A05:2021.

**Facet A (insecure default).** `httpsOnly` resolves false when neither option nor env is set
(`accelerator-prover.ts:126-127`) → non-strict mode probes plaintext HTTP alongside HTTPS
(`accelerator-transport.ts:317-319`) → winner selection accepts a plaintext responder satisfying the
2-field shape check (`:333-335`, `:383-392`) → pin `http` (`accelerator-prover.ts:222-226` →
`accelerator-transport.ts:242`) → `baseUrl` yields `http://…` (`:253-258`) → **SINK** the full serialized
witness POSTed in cleartext to an endpoint authenticated by nothing (`accelerator-prover.ts:362` → `:381`
→ `accelerator-transport.ts:455`).

**Facet B (unconstrained `host`).** Accepted from constructor options with no validation
(`accelerator-prover.ts:107`, default only when absent `:124`) and again at runtime
(`accelerator-transport.ts:188`, reachable from the public `setAcceleratorConfig`
`accelerator-prover.ts:137-141`) → **raw template interpolation at six sites, no `new URL()`, no loopback
check** (`accelerator-transport.ts:255`, `:257`, `:302`, `:318`, `:420-421`, `:439-441`) → witness POSTed
(`:455`) → the URL is logged in a way that can CONCEAL the true host (`accelerator-prover.ts:359`).
A value containing `@`, `/`, `?`, or `#` produces a URL whose authority is not what it appears:
`host = "127.0.0.1@attacker.tld"` → `http://127.0.0.1@attacker.tld:59833/prove` — userinfo `127.0.0.1`,
**actual host `attacker.tld`** — and that exact string is what gets logged. **Any future naive
`host.startsWith("127.")` guard would be bypassable; the fix must parse, not prefix-match.**

**Missing control.** No default-secure transport for the witness (no forced HTTPS, no warning, no
intermediate "plaintext permitted for loopback only" posture); no loopback/private-address constraint on
`host` despite loopback being the entire basis on which plaintext is considered acceptable; no structural
URL construction; no endpoint authentication in the default path. Documentation gap: README:82-87
documents `host` as `// Host. Default: "127.0.0.1"` with no security note, and that same
`AcceleratorConfig` block **omits `httpsOnly` entirely**.

**Exploit.** *A — impersonation:* squat the port, answer `/health` with the contract body, receive the
complete witness on the next `createChonkProof`, then proxy to the real accelerator and return its genuine
proof so the dApp works perfectly and the user never notices. Zero elevation, zero interaction, silent
total confidentiality breach of every proved transaction. *B — off-machine exfiltration by configuration:*
a team runs the Accelerator on a shared build box and follows the documented API
(`setAcceleratorConfig({ host: "10.0.3.44" })` — README:216 shows `host` in exactly this position with no
caveat) → every user's witness crosses the LAN in cleartext, readable by any passive observer. No attacker
required; this is the "accidental violation / unintended exposure" class the brief scopes in.

**Why this is NOT already-accepted.** README:197-202 discloses a squatting residual, but read exactly:
*"any same-machine process that **obtained a browser-trusted certificate for localhost** and squats the
HTTPS port is past this line"* — that describes the residual INSIDE `httpsOnly` mode and sets the attacker
bar at forging browser trust. In the DEFAULT mode the bar is *binding a TCP port and returning ~31 bytes
of JSON*. The disclosed residual does not describe the default posture. The already-accepted list covers
SERVER-side decisions (unauthenticated `/health`, absent-Origin auto-approval, permissive CORS,
verified-sites); none of them is "the SDK will hand a private witness to whatever answers the port, in
plaintext, by default" — a CLIENT-side posture choice, in this cluster. The Accelerator ships a whole
name-constrained local CA + OS trust-store integration specifically so the loopback endpoint CAN be
authenticated; the SDK defaults to not requiring it. Loopback-plaintext-is-safe-enough holds only against
a passive sniffer (needs CAP_NET_RAW/admin/Npcap), which is why the impersonation vector — needing nothing
— is the one that matters, and it evaporates entirely under facet B where the traffic isn't on loopback.
The generation-guard hardening (`accelerator-prover.ts:309-322,373-376,402-410,424`) is genuinely solid —
traced, no window — but it prevents the witness going to an UNPROBED endpoint, not to a PROBED but
UNAUTHENTICATED one.

**Instances.** `accelerator-prover.ts:126-127`, `:107`, `:124`; `accelerator-transport.ts:188`, `:255`,
`:257`, `:302`, `:318`, `:420-421`, `:439-441`, `:455`; `types.ts:29-30`; `README.md:82-87`, `:216`.

## NON-FINDINGS (leads REFUTED + sweep)

- **Lead: npm tarball ships `src/**` incl. `test-setup.ts` — REFUTED as a security issue.**
  `package.json:11` sets `"exports": "./src/index.ts"` as a PLAIN STRING, which under Node's resolution
  exposes only the `"."` subpath; every exports-aware resolver (Node ESM, Vite, webpack 5, Rollup +
  node-resolve) refuses `@alejoamiras/aztec-accelerator/src/test-setup.ts` and every other deep path.
  `test-setup.ts` is not imported by `index.ts` or anything it reaches, so its `globalThis.expect`
  mutation (`:8-10`) is unreachable in a consumer. Packaging hygiene for the QUALITY run, not security.
- **Lead: `@aztec/simulator/client` dynamically imported but only a devDependency — REFUTED at the
  security bar.** The real consequence is that the module performing witness simulation has NO version
  constraint of any kind (not deps, peer, or optional), so it binds to whatever the consumer hoisted; the
  failure path is a clean actionable error (`:38-42`). No attacker-controlled step — exploiting it requires
  already controlling the consumer's `node_modules`. Real supply-chain hygiene defect, not a finding here.
- `Buffer.from` in a browser library (`:510`) — needs the consumer's polyfill, which SKILL.md:164
  instructs; absent it, `ReferenceError` → caught `:485` → WASM fallback. Documented, graceful.
- Generation guard vs `setAcceleratorConfig` races — traced; correct, no exploitable window.
  `#inflightProbe` assignment (`:170-174`) safe (the `.finally` runs in a later microtask).
- HTTPS→HTTP demotion re-validation (`:417-433`) — present and generation-checked; its weakness is that
  the "validation" is a shape check, which is F-C8-3's root cause. The residual TOCTOU (real accelerator
  dies between `isProtocolHealthy` `:423` and `postProve` `:443`) needs a millisecond-scale re-bind window
  — too narrow to claim.
- Prototype pollution via the health body — not reachable.
- Secret leakage to logs — clean; the serialized witness (`:362`) is never logged. Sole caveat is the
  log-vs-actual-host divergence in F-C8-3 facet B.
- Env-var parsing (`:111-123`) — `parseInt` accepts `"0"`/`"-1"`/`"59833junk"` with no range check, but the
  values are build-time/dApp-controlled and the worst outcome is a failed fetch → WASM fallback.
- `createLazySimulator` Proxy (`:48-61`) returns a function for every string property, so a
  truthiness/feature-detect probe gets a false positive — correctness, for the bugs run.
- `.claude/skills/` shipped in the tarball — agent instructions distributed via npm that the README tells
  users to copy into their config; not auto-loaded from `node_modules`, and a package compromised enough to
  alter it already executes arbitrary code in the consumer's build. Awareness only.
