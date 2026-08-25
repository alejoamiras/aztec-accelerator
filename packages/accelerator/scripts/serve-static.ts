/**
 * Static file server for the desktop-UI Playwright webServer (replaces the `serve` package).
 * Loopback-only by design — it serves the built frontend to a local browser under test, never
 * external traffic. Bun's directory routes handle Content-Type, ETag/304, Range, and reject
 * path traversal outside the directory.
 */
const dir = process.argv[2] ?? ".";
const port = Number(process.argv[3] ?? 3456);

const server = Bun.serve({
  port,
  hostname: "127.0.0.1",
  routes: { "/*": { dir } },
});

console.log(`serving ${dir} at ${server.url}`);
