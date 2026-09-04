/**
 * Focused full-stack recovery gate: the browser starts HTTPS-only against the real HTTP-only
 * headless server, requires explicit consent, performs one native proof, then reloads back to
 * HTTPS-only. This intentionally does not run a second proof after reload.
 */
import { expect, test } from "@playwright/test";
import { assertServicesAvailable } from "./fullstack.fixture";
import { deployAndAssert } from "./fullstack.helpers";

function isHttpProve(urlString: string): boolean {
  const url = new URL(urlString);
  return url.protocol === "http:" && url.pathname === "/prove";
}

test("HTTP proving requires per-tab consent and resets on reload", async ({ browser }) => {
  test.skip(!process.env.ACCELERATOR_URL, "ACCELERATOR_URL env var not set");
  test.setTimeout(15 * 60 * 1000);
  await assertServicesAvailable();

  const page = await browser.newPage();
  const proveRequests: { method: string; url: string; bodyBytes: number }[] = [];
  const proveHits: { status: number; durationHeader: string | undefined }[] = [];
  page.on("request", (request) => {
    if (isHttpProve(request.url())) {
      proveRequests.push({
        method: request.method(),
        url: request.url(),
        bodyBytes: request.postDataBuffer()?.byteLength ?? 0,
      });
    }
  });
  page.on("response", (response) => {
    if (isHttpProve(response.url())) {
      proveHits.push({
        status: response.status(),
        durationHeader: response.headers()["x-prove-duration-ms"],
      });
    }
  });

  await page.goto("/?forceProofs=true");
  const walletState = page.locator("#wallet-state");
  await expect(walletState).not.toHaveText("not initialized", { timeout: 30_000 });
  await expect(walletState).not.toHaveText("initializing...", { timeout: 5 * 60 * 1000 });
  await expect(walletState).toHaveText("ready");

  await expect(page.locator("#accelerator-secure-title")).toHaveText(
    "Encrypted Connection is disabled",
  );
  expect(proveRequests, "diagnosis must never send a proof request").toEqual([]);

  await page.locator("#accelerator-use-http").click();
  await expect(page.locator("#http-session-confirmation")).toBeVisible();
  await page.locator("#http-session-confirm").click();
  await expect(page.locator("#accelerator-label")).toHaveText("available");

  await page.locator("#mode-accelerated").click();
  await deployAndAssert(page, "accelerated");

  expect(
    proveRequests.length,
    "consent must enable at least one HTTP proof request",
  ).toBeGreaterThan(0);
  expect(proveRequests.every((request) => request.method === "POST" && request.bodyBytes > 0)).toBe(
    true,
  );
  expect(
    proveHits.some((hit) => {
      const duration = Number(hit.durationHeader);
      return hit.status === 200 && Number.isFinite(duration) && duration > 0;
    }),
    `expected a successful native HTTP proof after consent; saw ${JSON.stringify(proveHits)}`,
  ).toBe(true);
  const phases = await page.evaluate(
    () => (window as Window & { __ACCEL_PHASES__?: string[] }).__ACCEL_PHASES__ ?? [],
  );
  expect(phases).toContain("receive");
  expect(phases).not.toContain("fallback");
  expect(phases).not.toContain("denied");

  const requestCount = proveRequests.length;
  await page.reload();
  await expect(page.locator("#accelerator-secure-title")).toHaveText(
    "Encrypted Connection is disabled",
  );
  await expect(page.locator("#accelerator-label")).toContainText("secure connection unavailable");
  expect(proveRequests).toHaveLength(requestCount);

  await page.close();
});
