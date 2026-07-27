1. **Unsigned update metadata enables rollback by replaying an older signed artifact**

   **Impact factors:** Integrity/Authorization; blast radius is one user per compromised update feed; sensitive asset is the installed desktop binary that brokers proving requests. Exploitability: network/upstream or CI/feed-control vector, low complexity once feed is controlled, no local privileges required, user interaction required only when auto-update is off.

   **Evidence confidence:** Moderate.

   **OWASP/CWE:** OWASP A08:2021 Software and Data Integrity Failures; CWE-347 Improper Verification of Cryptographic Signature.

   **Trace:** Untrusted update metadata enters through `tauri_plugin_updater::UpdaterExt` at `packages/accelerator/src-tauri/src/updater.rs:9`, then `app.updater()` / `updater.check().await` returns an `Update` from the feed at `packages/accelerator/src-tauri/src/updater.rs:19-27`. The code treats feed-controlled `update.version` as authoritative at `packages/accelerator/src-tauri/src/updater.rs:39-41`, then either auto-installs at `packages/accelerator/src-tauri/src/updater.rs:46-50` or returns the same update for a user prompt at `packages/accelerator/src-tauri/src/updater.rs:52-56`. The downloaded artifact bytes come from `update.download(...)` at `packages/accelerator/src-tauri/src/updater.rs:126-137`; the sink is `update.install(bytes)` at `packages/accelerator/src-tauri/src/updater.rs:170`. The crypto dependency path is `tauri-plugin-updater` with `minisign-verify` in `packages/accelerator/src-tauri/Cargo.lock:4791-4805`.

   **Missing control:** The signed artifact is not bound in this code to the feed’s claimed `version`, nor is there an app-maintained monotonic rollback floor. There is no post-download check that the artifact’s embedded app version equals `update.version` and is newer than the installed version.

   **Exploit/violation scenario:**  
   1. An attacker controls the updater JSON/feed publishing path, but not the minisign private key.  
   2. They publish a feed entry with `version: "999.0.0"` so `updater.check()` considers it newer.  
   3. They set `url` and `signature` to an older, still-valid Aztec Accelerator artifact and its historical minisign signature.  
   4. `perform_update()` downloads those bytes and `update.install(bytes)` installs them because the artifact signature is valid.  
   5. The user is rolled back to an older signed build, potentially reintroducing fixed vulnerabilities.

   **Preconditions:** Attacker can modify the updater feed or its CI/storage/CDN output; an older signed artifact and matching signature are available; the target has auto-update enabled or accepts the update prompt; the platform installer accepts replacement with the older package.

   **Why existing mitigations fail:** Minisign verification protects artifact bytes from unsigned modification, but it does not prove that the artifact corresponds to the feed’s claimed `update.version`. The version comparison can be bypassed by declaring a higher feed version while replaying older signed bytes. The size cap at `packages/accelerator/src-tauri/src/updater.rs:109-122` only addresses download size and does not authenticate version-to-artifact binding.

   **Instances:** `packages/accelerator/src-tauri/src/updater.rs:27`, `packages/accelerator/src-tauri/src/updater.rs:39-41`, `packages/accelerator/src-tauri/src/updater.rs:46-56`, `packages/accelerator/src-tauri/src/updater.rs:126-137`, `packages/accelerator/src-tauri/src/updater.rs:170`.