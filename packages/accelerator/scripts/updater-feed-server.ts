/**
 * Local HTTPS feed server for the release-time updater smoke test.
 *
 * Impersonates `https://aztec-accelerator.dev` on the CI runner (paired with an
 * `/etc/hosts` entry + a trusted local CA — see updater-smoke.sh). Serves:
 *   - GET /releases/latest.json        → the synthesized feed for version N
 *   - GET /releases/download/<file>    → the (already prod-signed) N artifacts
 *
 * The Tauri updater in the N-1 binary fetches the hardcoded endpoint
 * (https://aztec-accelerator.dev/releases/latest.json), then downloads the
 * artifact URL from the feed and verifies its `.sig` against the embedded
 * prod pubkey. We serve the real signed artifacts, so NO signing key is needed.
 *
 * Usage:
 *   bun updater-feed-server.ts \
 *     --cert <leaf.pem> --key <leaf.key> \
 *     --latest-json <latest.json> --serve-dir <artifacts-dir> [--port 443]
 */

function arg(name: string, fallback?: string): string {
  const i = process.argv.indexOf(`--${name}`);
  if (i !== -1 && process.argv[i + 1]) return process.argv[i + 1];
  if (fallback !== undefined) return fallback;
  throw new Error(`missing required --${name}`);
}

const certPath = arg("cert");
const keyPath = arg("key");
const latestJsonPath = arg("latest-json");
const serveDir = arg("serve-dir");
const port = Number(arg("port", "443"));

const server = Bun.serve({
  port,
  tls: {
    cert: Bun.file(certPath),
    key: Bun.file(keyPath),
  },
  async fetch(req) {
    const url = new URL(req.url);
    const path = url.pathname;

    if (path === "/releases/latest.json") {
      const body = await Bun.file(latestJsonPath).text();
      // Logged so the smoke test can assert the feed was actually hit (guards
      // against a no-op "pass" where the updater never reached our feed).
      console.log(`feed-server: ${req.method} ${path} -> 200`);
      return new Response(body, {
        headers: { "content-type": "application/json" },
      });
    }

    // Any other path → serve the basename from the artifacts dir.
    // (latest.json points download URLs at /releases/download/<file>.)
    // URL-decode: the Tauri bundle name has a space ("Aztec Accelerator.app.tar.gz"),
    // which the updater requests %20-encoded — decode it to match the on-disk file.
    const name = decodeURIComponent(path.split("/").pop() ?? "");
    if (name) {
      const file = Bun.file(`${serveDir}/${name}`);
      if (await file.exists()) {
        console.log(`feed-server: ${req.method} ${path} -> 200 (${name})`);
        return new Response(file, {
          headers: { "content-type": "application/octet-stream" },
        });
      }
    }

    console.error(`feed-server: ${req.method} ${path} -> 404`);
    return new Response("not found", { status: 404 });
  },
});

console.log(`updater feed server listening on https://localhost:${server.port}`);
