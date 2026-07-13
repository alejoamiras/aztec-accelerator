import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { defineConfig, loadEnv, type Plugin } from "vite";
import { nodePolyfills } from "vite-plugin-node-polyfills";

const require = createRequire(import.meta.url);

/**
 * Vite plugin: redirect dependency worker file requests to their real location.
 *
 * bb.js and @aztec/kv-store (sqlite-opfs) spawn Web Workers via:
 *   new Worker(new URL('./main.worker.js', import.meta.url), { type: 'module' })
 *
 * When Vite's dep optimizer pre-bundles those packages, import.meta.url changes
 * to point at `.vite/deps/` — but the worker files aren't copied there. Serve
 * them from their real node_modules location instead (matched by exact basename,
 * since kv-store's worker is literally `worker.js` — a substring of the bb ones).
 */
function bbWorkerPlugin(): Plugin {
  const workerFiles: Record<string, string> = {};

  return {
    name: "bb-worker-redirect",
    configResolved(config) {
      try {
        const bbProverPath = require.resolve("@aztec/bb-prover");
        const bbRequire = createRequire(bbProverPath);
        const bbEntry = bbRequire.resolve("@aztec/bb.js");
        const bbRoot = bbEntry.slice(0, bbEntry.indexOf("@aztec/bb.js/") + "@aztec/bb.js/".length);
        const bbBrowserDir = resolve(bbRoot, "dest", "browser", "barretenberg_wasm");
        workerFiles["main.worker.js"] = resolve(
          bbBrowserDir,
          "barretenberg_wasm_main",
          "factory",
          "browser",
          "main.worker.js",
        );
        workerFiles["thread.worker.js"] = resolve(
          bbBrowserDir,
          "barretenberg_wasm_thread",
          "factory",
          "browser",
          "thread.worker.js",
        );
        config.logger.info(`[bb-worker-redirect] Resolved worker files in ${bbBrowserDir}`);
      } catch (err) {
        config.logger.warn(`[bb-worker-redirect] Could not resolve @aztec/bb.js workers: ${err}`);
      }
      try {
        const kvEntry = require.resolve("@aztec/kv-store/sqlite-opfs");
        workerFiles["worker.js"] = resolve(kvEntry, "..", "worker.js");
        config.logger.info(`[bb-worker-redirect] Resolved kv-store sqlite-opfs worker`);
      } catch (err) {
        config.logger.warn(`[bb-worker-redirect] Could not resolve @aztec/kv-store worker: ${err}`);
      }
    },
    configureServer(server) {
      server.middlewares.use((req, _res, next) => {
        if (!req.url) return next();

        if (req.url.includes(".vite/deps")) {
          const basename = req.url.split("?")[0].split("/").pop();
          const realPath = basename && workerFiles[basename];
          if (realPath) req.url = `/@fs/${realPath}`;
        }
        next();
      });
    },
  };
}

export default defineConfig(({ mode, command }) => {
  const allEnv = loadEnv(mode, process.cwd(), "");
  const env = {
    AZTEC_NODE_URL: allEnv.AZTEC_NODE_URL,
  };

  // Read @aztec/stdlib version from SDK package.json at build time
  const sdkPkg = JSON.parse(readFileSync(resolve(__dirname, "../sdk/package.json"), "utf8"));
  const aztecSdkVersion: string = sdkPkg.dependencies["@aztec/stdlib"] ?? "unknown";

  return {
    plugins: [
      nodePolyfills({
        include: ["buffer", "path"],
        globals: { Buffer: true },
      }),
      bbWorkerPlugin(),
    ],
    optimizeDeps: {
      exclude: ["@aztec/noir-acvm_js", "@aztec/noir-noirc_abi"],
      esbuildOptions: {
        // @aztec/kv-store's sqlite-opfs backend (the 5.0 browser default) uses package-internal
        // `#...` subpath imports, which Vite's dep optimizer can't resolve through the package's
        // `imports` map — map them to their browser-condition targets. Production rollup resolves
        // them natively; this only affects the dev-server prebundle.
        plugins: [
          {
            name: "aztec-kv-store-subpath-imports",
            setup(build) {
              build.onResolve({ filter: /^#msgpackr$/ }, () => ({
                path: require.resolve("msgpackr/index-no-eval"),
              }));
              build.onResolve({ filter: /^#ordered-binary$/ }, () => {
                const kvEntry = require.resolve("@aztec/kv-store/sqlite-opfs");
                const kvRoot = kvEntry.slice(
                  0,
                  kvEntry.indexOf("@aztec/kv-store/") + "@aztec/kv-store/".length,
                );
                return {
                  path: resolve(
                    kvRoot,
                    "dest",
                    "sqlite-opfs",
                    "internal",
                    "ordered-binary-browser.js",
                  ),
                };
              });
            },
          },
        ],
      },
    },
    server: {
      headers: {
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "credentialless",
      },
      proxy: {
        "/aztec": {
          target: env.AZTEC_NODE_URL || "http://localhost:8080",
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/aztec/, ""),
        },
      },
      fs: {
        allow: ["../.."],
      },
    },
    build: {
      target: "esnext",
    },
    esbuild: {
      target: "esnext",
    },
    resolve: {
      alias: {
        ...(command === "build" && {
          "vite-plugin-node-polyfills/shims/buffer": require.resolve(
            "vite-plugin-node-polyfills/shims/buffer",
          ),
          "vite-plugin-node-polyfills/shims/process": require.resolve(
            "vite-plugin-node-polyfills/shims/process",
          ),
        }),
      },
      dedupe: ["@aztec/bb-prover"],
    },
    define: {
      "process.env": JSON.stringify({
        AZTEC_NODE_URL: env.AZTEC_NODE_URL,
        VITE_AZTEC_SDK_VERSION: aztecSdkVersion,
      }),
    },
  };
});
