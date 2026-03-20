import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { defineConfig, loadEnv, type Plugin } from "vite";
import { nodePolyfills } from "vite-plugin-node-polyfills";

const require = createRequire(import.meta.url);

/**
 * Vite plugin: redirect bb.js worker file requests to their real location.
 *
 * Barretenberg (bb.js) spawns Web Workers via:
 *   new Worker(new URL('./main.worker.js', import.meta.url), { type: 'module' })
 *
 * When Vite's dep optimizer pre-bundles bb.js, import.meta.url changes to
 * point at `.vite/deps/` — but the worker files aren't copied there.
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
    },
    configureServer(server) {
      server.middlewares.use((req, _res, next) => {
        if (!req.url) return next();

        for (const [filename, realPath] of Object.entries(workerFiles)) {
          if (req.url.includes(filename) && req.url.includes(".vite/deps")) {
            req.url = `/@fs/${realPath}`;
            break;
          }
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
    SPONSORED_FPC_SALT: allEnv.SPONSORED_FPC_SALT,
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
    resolve: {
      alias: {
        ...(command === "build" && {
          "vite-plugin-node-polyfills/shims/buffer": require.resolve(
            "vite-plugin-node-polyfills/shims/buffer",
          ),
        }),
      },
      dedupe: ["@aztec/bb-prover"],
    },
    define: {
      "process.env": JSON.stringify({
        AZTEC_NODE_URL: env.AZTEC_NODE_URL,
        SPONSORED_FPC_SALT: env.SPONSORED_FPC_SALT,
        VITE_AZTEC_SDK_VERSION: aztecSdkVersion,
      }),
    },
  };
});
