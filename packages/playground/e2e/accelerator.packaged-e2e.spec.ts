/**
 * B4 packaged-E2E: the COMPOSED proof.
 *
 * A real browser, loading the PACKED SDK (this playground built against the tarball), drives the
 * INSTALLED desktop app to a native `bb` proof over HTTPS — with HTTP-downgrade and WASM-fallback
 * disabled. The CI harness (see `.github/workflows`) installs the app, runs
 * `AztecAccelerator --generate-certs-only`, seeds the generated CA into the OS/browser trust store
 * out-of-band (Linux: user NSS via certutil; macOS: sudo System keychain), writes a config with
 * `https_enabled:true` + this origin pre-approved, and launches the app so it serves TLS on :59834.
 *
 * The DECISIVE witness of "native bb over HTTPS" (not a silent WASM/HTTP shortcut): a
 * `200 https://127.0.0.1:59834/prove` carrying an `x-prove-duration-ms` response header. The server
 * adds that header ONLY after a successful `bb::prove` (`core/src/server/prove.rs`), and a WASM
 * fallback or an HTTP path never issues that request at all — so its presence discriminates the
 * accelerated-over-HTTPS path from every fallback. We ALSO assert the deploy itself succeeds (a real
 * proof was returned + the tx mined) via the shared `deployAndAssert`.
 *
 * `?forceProofs=true` makes proving actually run even on a dev chain; `?httpsOnly=true` (see
 * `aztec.ts:initializeNode`) forces the prover to HTTPS-only with NO HTTP downgrade. We do NOT set
 * `ignoreHTTPSErrors` — the TLS handshake MUST succeed against the seeded trust, or the proof is not
 * a trust proof. Served from localhost, so Chrome 142+ Local Network Access exempts it.
 *
 * Usage: bun run --cwd packages/playground test:e2e:packaged
 */
import { expect, test } from "@playwright/test";
import { deployAndAssert } from "./fullstack.helpers";

const HTTPS_PROVE_URL = "https://127.0.0.1:59834/prove";

test.describe.configure({ mode: "serial" });

test("native bb proof over HTTPS via the installed desktop app", async ({ browser }) => {
  test.setTimeout(15 * 60 * 1000);
  const page = await browser.newPage();

  // Capture every /prove round-trip so we can prove the accelerated-over-HTTPS path ran.
  const proveHits: { url: string; status: number; durationHeader: string | undefined }[] = [];
  page.on("response", (res) => {
    if (res.url() === HTTPS_PROVE_URL) {
      proveHits.push({
        url: res.url(),
        status: res.status(),
        durationHeader: res.headers()["x-prove-duration-ms"],
      });
    }
  });
  page.on("console", (msg) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      console.log(`[browser:${msg.type()}] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => console.log(`[browser:pageerror] ${err.message}`));

  await page.goto("/?forceProofs=true&httpsOnly=true");

  // Wallet bring-up (proving is enabled by ?forceProofs, so this includes prover init).
  const walletState = page.locator("#wallet-state");
  await expect(walletState).not.toHaveText("not initialized", { timeout: 30_000 });
  await expect(walletState).not.toHaveText("initializing...", { timeout: 5 * 60 * 1000 });
  await expect(walletState).toHaveText("ready");

  await page.click("#mode-accelerated");
  await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);

  // Drives a real account deploy → a real private-kernel proof → routed to the installed app.
  // Fails loudly if the deploy reverts / errors (the helper asserts the success UI + "no Deploy failed:").
  await deployAndAssert(page, "accelerated");

  // THE decisive assertion: at least one successful native /prove happened over HTTPS to the
  // installed app, carrying the server's post-`bb::prove` header. Absent ⇒ a silent WASM/HTTP
  // fallback slipped through (or the TLS/ trust seed failed) — which must FAIL this gate.
  const nativeHttpsProve = proveHits.find(
    (h) => h.status === 200 && h.durationHeader !== undefined,
  );
  expect(
    nativeHttpsProve,
    `expected a 200 ${HTTPS_PROVE_URL} carrying x-prove-duration-ms (native bb over HTTPS). ` +
      `Saw /prove hits: ${JSON.stringify(proveHits)}. An empty list means the proof never reached ` +
      `the app over HTTPS (silent WASM fallback or a failed TLS handshake against the seeded CA).`,
  ).toBeTruthy();

  await page.close();
});
