import { afterAll, describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * Contract test for serve-static.ts — the webServer behaviors the desktop-UI Playwright suite
 * relies on (index resolution, asset Content-Type, conditional requests) plus the two safety
 * properties (traversal rejection, loopback-only binding). No other suite covers this server.
 */
const dir = mkdtempSync(join(tmpdir(), "serve-static-"));
writeFileSync(join(dir, "index.html"), "<title>t</title>hello");
writeFileSync(join(dir, "app.js"), "console.log(1);");

const server = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  routes: { "/*": { dir } },
});
const base = `http://127.0.0.1:${server.port}`;

afterAll(() => server.stop(true));

describe("serve-static contract", () => {
  test("serves the index at /", async () => {
    const res = await fetch(`${base}/`);
    expect(res.status).toBe(200);
    expect(await res.text()).toContain("hello");
  });

  test("serves assets with a correct Content-Type", async () => {
    const res = await fetch(`${base}/app.js`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type") ?? "").toContain("javascript");
  });

  test("answers conditional requests with 304 via ETag", async () => {
    const first = await fetch(`${base}/app.js`);
    const etag = first.headers.get("etag");
    expect(etag).toBeTruthy();
    const second = await fetch(`${base}/app.js`, { headers: { "If-None-Match": etag ?? "" } });
    expect(second.status).toBe(304);
  });

  test("rejects path traversal out of the directory", async () => {
    // Raw socket: fetch normalizes ../ away before the request leaves the client.
    const raw = await new Promise<string>((resolve, reject) => {
      const chunks: string[] = [];
      Bun.connect({
        hostname: "127.0.0.1",
        port: server.port,
        socket: {
          open(s) {
            s.write(
              `GET /../../../../etc/passwd HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n`,
            );
          },
          data(_s, d) {
            chunks.push(d.toString());
          },
          close() {
            resolve(chunks.join(""));
          },
          error(_s, e) {
            reject(e);
          },
        },
      });
    });
    expect(raw).not.toContain("root:");
    expect(raw.split("\r\n")[0]).toMatch(/ (30[1-8]|40[0-9]|414) /);
  });

  test("binds loopback only", () => {
    expect(server.hostname).toBe("127.0.0.1");
  });
});
