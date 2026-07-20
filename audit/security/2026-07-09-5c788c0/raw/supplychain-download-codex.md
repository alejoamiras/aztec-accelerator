1. **Unverified `download-bb.ts` cache population bypasses runtime digest verification**

   **Impact factors:** Confidentiality and Integrity; blast radius one user; data sensitivity high because a malicious cached `bb` receives the proving witness path and can read private ZK inputs. Exploitability: network/supply-chain vector, low complexity, privileges required high for upstream/release compromise or equivalent malicious release delivery, user interaction required to run `bb:download`, then an authorized proving request triggers execution.

   **Evidence confidence:** high.

   **OWASP + CWE:** OWASP A08:2021 Software and Data Integrity Failures; CWE-494 Download of Code Without Integrity Check.

   **Trace:** `package.json:13` exposes `bb:download` -> `scripts/download-bb.ts:203-219` accepts version arguments -> `scripts/download-bb.ts:32-33` builds the GitHub release URL -> `scripts/download-bb.ts:76-87` fetches bytes and reads the whole response without digest/signature verification -> `scripts/download-bb.ts:88-93` extracts the tarball into `~/.aztec-accelerator/versions/{version}` -> `scripts/download-bb.ts:101-106` only checks that `bb` exists and chmods it. That is the same runtime cache layout as `packages/accelerator/core/src/versions/cache_layout.rs:7-13` and `packages/accelerator/core/src/versions/cache_layout.rs:27-30`. Runtime then skips verified download when the cache entry exists at `packages/accelerator/core/src/server/prove.rs:75` and executes the cached binary via `packages/accelerator/core/src/bb.rs:31-35` -> `packages/accelerator/core/src/bb.rs:83-98`.

   **Missing control:** `download-bb.ts` lacks the Rust downloader’s fail-closed SHA-256 digest check and does not mark/cache entries as verified. Runtime cache lookup trusts mere file existence.

   **Exploit/violation scenario:**  
   1. A compromised upstream release serves a malicious `barretenberg-*.tar.gz` for version `X`.  
   2. The user or maintainer runs `bun run bb:download X`.  
   3. The script extracts the malicious `bb` into the shared runtime cache without verifying a digest.  
   4. Later, an approved dApp sends `/prove` with `x-aztec-version: X`.  
   5. The app sees the cached binary and executes it; the malicious `bb` reads/exfiltrates the witness file passed through `--ivc_inputs_path`.

   **Preconditions:** victim runs `bb:download` for the attacker-controlled version; the malicious version remains cached; a later authorized proving request selects that version.

   **Why existing mitigations fail:** the Rust path is fail-closed only when `versions::download_bb` runs. A pre-populated cache entry causes `needs_download` to be false and `find_bb` returns the cached path directly, so `verify_digest` is never reached.

   **Instances:** `package.json:13`; `scripts/download-bb.ts:32-33`, `scripts/download-bb.ts:76-106`, `scripts/download-bb.ts:203-219`; `packages/accelerator/core/src/server/prove.rs:75`; `packages/accelerator/core/src/bb.rs:31-35`, `packages/accelerator/core/src/bb.rs:98`.

2. **Auto-pinning Windows `bb.exe` checksums can bless compromised first-seen bytes**

   **Impact factors:** Confidentiality and Integrity; blast radius all Windows users of a release built from the bad pin; data sensitivity high because the shipped `bb.exe` processes private witnesses. Exploitability: network/supply-chain vector, low attack complexity after upstream compromise, privileges required high for compromised upstream/release or update actor, user interaction required by maintainer/reviewer accepting the generated pin.

   **Evidence confidence:** high.

   **OWASP + CWE:** OWASP A08:2021 Software and Data Integrity Failures; CWE-494 Download of Code Without Integrity Check.

   **Trace:** `scripts/update-aztec-version.ts:79-93` fetches `barretenberg-amd64-windows.tar.gz`, hashes the exact fetched bytes, and writes that hash into `packages/accelerator/scripts/copy-bb.ts` -> `scripts/update-aztec-version.ts:137` runs that pinning during the version update flow. The prebuild then treats the inserted value as the trust anchor at `packages/accelerator/scripts/copy-bb.ts:56-70` and `packages/accelerator/scripts/copy-bb.ts:76-85`; it fetches the tarball at `packages/accelerator/scripts/copy-bb.ts:100-107`, verifies only against the auto-pinned hash at `packages/accelerator/scripts/copy-bb.ts:116-123`, extracts and copies `bb.exe` into the shipped sidecar at `packages/accelerator/scripts/copy-bb.ts:125-148`. This prebuild is release/CI wired through `packages/accelerator/package.json:8` and `.github/actions/setup-accelerator/action.yml:115-118`.

   **Missing control:** the checksum pin is derived from the same first-seen release asset it is supposed to authenticate. There is no independent signature, independent digest source, reproducible-build check, or enforced manual verification before inserting the trust anchor.

   **Exploit/violation scenario:**  
   1. The Windows Aztec release asset is compromised when a maintainer runs `bun run aztec:update X`.  
   2. `pinWindowsBbChecksum` computes the SHA-256 of the malicious tarball and writes it into `WINDOWS_BB_CHECKSUMS`.  
   3. CI and release prebuilds later verify the malicious tarball against that maliciously derived pin, so the gate passes.  
   4. The Windows app ships the malicious `bb.exe`, which can read private witness inputs during proving.

   **Preconditions:** maintainer uses the update script and commits the generated checksum; compromised bytes are available at pin time; a Windows build is produced for that version.

   **Why existing mitigations fail:** `copy-bb.ts` fails closed for unknown versions and hash mismatches, but after auto-pinning it only proves future downloads match the first fetched bytes. The “review-gated pin” comment is undermined because the script writes the pin automatically from GitHub release content.

   **Instances:** `scripts/update-aztec-version.ts:79-93`, `scripts/update-aztec-version.ts:137`; `packages/accelerator/scripts/copy-bb.ts:56-85`, `packages/accelerator/scripts/copy-bb.ts:100-148`; `packages/accelerator/package.json:8`; `.github/actions/setup-accelerator/action.yml:115-118`.