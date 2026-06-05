# Research — server-core (server.rs + headless server/main.rs) · Q1, Q2, Q8, Q10-src, Q15

## Public surface
- Exports: `AppState` (7 Option fields, L32-43), `StatusCallback=Arc<dyn Fn(&str)>`, `VersionsChangedCallback`, `ShowAuthPopupCallback`, `PORT=59833`, `HTTPS_PORT=59834`, fns `start`/`start_https`/`healthy_aztec_on_port`/`router`.
- Constructs AppState: main.rs:347-366 (full), server/main.rs:62-67 (headless: on_status/show_auth_popup=None, semaphore=Some), tests (Default).
- **Option guards** to remove with Q1: on_status (436,464,530), auth_manager (292: None→auto-approve), config (248,324,382), show_auth_popup (334: None→headless deny), prove_semaphore (513), bundled_version (223), on_versions_changed (444).

## Invariants (wire contract — DO NOT BREAK; SDK consumes)
- `GET /health` → `{status:"ok",api_version:1,version,aztec_version,available_versions[],bb_available,https_port?}`.
- `POST /prove` (msgpack, 50MB limit) → `{proof:"<base64>"}` + header `x-prove-duration-ms`. Magic headers: req `x-aztec-version`,`content-type:application/octet-stream`,`origin`; resp `x-prove-duration-ms`,`cross-origin-resource-policy:cross-origin`. CORS allow `content-type,x-aztec-version`, expose `x-prove-duration-ms`.
- **Error JSON** = `{"error":"<id>","message":"<text>"}` — 11 distinct sites; **SDK parses `{error?,message?}` at accelerator-prover.ts:375-378; 403→WASM fallback**. Keep field names + shape byte-identical.
- Auth flow ordering (authorize_origin L288-406, 6 phases): no-auth-mgr→approve / no-origin→approve / invalid→400 / approved→pass / no-popup→403 / popup-wait→timeout(60s,L361)/cancel/allow(+remember persist)/deny.
- bind_with_retry (AddrInUse only, 5s budget, hard deadline); prove semaphore=1; **headless = None callbacks** (on_status silent, no-popup auto-denies w/ auth_mgr).

## Tests (29 in server.rs) — strong on HTTP contract
classifies_health_responses, bind_with_retry ×3, health ×4, cors ×3, prove auth ×7 (localhost/popup/denied/no-origin/remembered/headless-403/429/timeout), resolve_version ×3, compute_threads ×3, oversized/empty body.
**GAPS:** full /prove happy path (auth→resolve→semaphore→bb::prove→`{proof}`+header) UNtested (bb stubbed); exact error JSON bodies rarely asserted; on_status sequencing (Downloading→Proving→Idle, StatusGuard drop on all exits) untested; headless no-auth-mgr path + ALLOWED_ORIGINS env untested.

## Safe seams
- **Q1**: `HeadlessState{bundled_version,https_port,config,auth_manager,prove_semaphore}` + `GuiCallbacks{on_status,on_versions_changed,show_auth_popup}`; `AppState{core:Arc<HeadlessState>, gui:Option<Arc<GuiCallbacks>>}`. Router works both modes (headless gui:None). Makes headless/GUI explicit.
- **Q2**: extract `authorize_origin` (clean, no side effects bar tracing+auth cb) + prove core (L512-577) into handlers/ submodule; split server.rs → bind.rs/tls.rs/handlers/prove.rs, server.rs = thin router+start.
- **Q8**: `ProveErrorBody{error,message}: Serialize` + `IntoResponse for (StatusCode,ProveErrorBody)` → **same JSON `{error,message}`, same status**; content-type text/plain→application/json (SDK doesn't check — SAFE). Collapses 11 inline `json!`/`json_error` sites + the `.unwrap()`s.
- **Q10-src**: replace `StatusCallback=Arc<dyn Fn(&str)>` with `Arc<dyn Fn(ServerStatus)>`; emit `ServerStatus::{Downloading,Proving,Idle}` at server.rs:437,465,531; consumer (main.rs:356) matches variants not substrings. COORDINATED w/ desktop-shell.
- **Q15**: `pub const AUTH_DECISION_TIMEOUT: Duration` in lib, imported by windows.rs.

## Behavior-change risks
- Q8 = **HIGH if shape drifts** (breaks SDK); SAFE if `{error,message}` held byte-identical. Header names are wire contract.
- Q10 = MEDIUM (emit↔tray substring coupling; enum could change animation timing if emit faster than UI loop) → coordinated; pin phase strings via characterization test first.
- Q1/Q2 = LOW (internal) but headless server/main.rs is a hard constraint (must keep both modes green).
