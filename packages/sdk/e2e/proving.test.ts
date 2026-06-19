/**
 * AcceleratorProver proving e2e tests
 *
 * One shared setup (prover + wallet + Sponsored FPC), then deploys an account
 * in each mode:
 *   - Accelerated: real accelerator desktop app (skipped when ACCELERATOR_URL not set)
 *   - Local (WASM): fallback via unreachable port
 *
 * Network-agnostic: always uses Sponsored FPC + from: NO_FROM.
 * Services must be running before tests start (asserted by e2e-setup.ts preload).
 */

import { describe, expect, test } from "bun:test";
import { SponsoredFeePaymentMethod } from "@aztec/aztec.js/fee";
import { Fr } from "@aztec/aztec.js/fields";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { SponsoredFPCContract } from "@aztec/noir-contracts.js/SponsoredFPC";
import { WASMSimulator } from "@aztec/simulator/client";
import { getContractInstanceFromInstantiationParams } from "@aztec/stdlib/contract";
import { EmbeddedWallet } from "@aztec/wallets/embedded";
import { getLogger } from "@logtape/logtape";
import { AcceleratorProver } from "../src/index";
import { deploySchnorrAccount } from "./e2e-helpers.js";
import { config } from "./e2e-setup.js";

const logger = getLogger(["aztec-accelerator", "sdk", "e2e", "proving"]);

// Shared state across all describes
let node: ReturnType<typeof createAztecNodeClient>;
let prover: AcceleratorProver;
let wallet: EmbeddedWallet;
let feePaymentMethod: SponsoredFeePaymentMethod;

describe("AcceleratorProver", () => {
  describe("Setup", () => {
    test("should create prover and connect to Aztec node", async () => {
      prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      node = createAztecNodeClient(config.nodeUrl);
      const nodeInfo = await node.getNodeInfo();

      expect(nodeInfo).toBeDefined();
      expect(nodeInfo.l1ChainId).toBeDefined();
      logger.info("Connected to Aztec node", { chainId: nodeInfo.l1ChainId });
    });

    test("should create EmbeddedWallet with Sponsored FPC", async () => {
      expect(node).toBeDefined();
      expect(prover).toBeDefined();

      wallet = await EmbeddedWallet.create(node, {
        ephemeral: true,
        // Always generate real proofs — dummy proofs hide real issues.
        pxe: {
          proverEnabled: true,
          proverOrOptions: prover,
        },
      });

      // Derive the canonical (salt=0) Sponsored FPC address and register in PXE —
      // deployed + funded on every network we target (sandbox auto-deploys; testnet has it).
      const fpcInstance = await getContractInstanceFromInstantiationParams(
        SponsoredFPCContract.artifact,
        { salt: new Fr(0) },
      );
      await wallet.registerContract(fpcInstance, SponsoredFPCContract.artifact);
      feePaymentMethod = new SponsoredFeePaymentMethod(fpcInstance.address);

      expect(wallet).toBeDefined();
      logger.info("Wallet ready with Sponsored FPC", {
        fpc: fpcInstance.address.toString().slice(0, 20),
      });
    });
  });

  describe.skipIf(!config.acceleratorUrl)("Accelerated", () => {
    test("should report accelerator as available", async () => {
      const status = await prover.checkAcceleratorStatus();
      expect(status.available).toBe(true);
      logger.info("Accelerator status", { available: status.available });
    });

    test("should deploy account through the NATIVE accelerator path (not WASM fallback)", async () => {
      expect(wallet).toBeDefined();

      // Positive native-path proof: the prover emits "transmit" only on the native /prove path and
      // "fallback" only on the WASM path. A mined tx alone does NOT prove native bb ran (a silent
      // fallback would also mine), so we assert the phase trail discriminates them.
      const phases: string[] = [];
      prover.setOnPhase((p) => phases.push(p));
      try {
        const deployed = await deploySchnorrAccount(wallet, feePaymentMethod, "accelerated");
        expect(deployed).toBeDefined();
      } finally {
        prover.setOnPhase(null);
      }
      expect(phases).toContain("transmit"); // native /prove round-trip to :59833
      expect(phases).not.toContain("fallback"); // never silently fell back to WASM
    }, 600_000);
  });

  describe("Local (WASM)", () => {
    test("should deploy account with local proving (WASM fallback path)", async () => {
      expect(wallet).toBeDefined();

      // Force WASM fallback by pointing at an unreachable port
      prover.setAcceleratorConfig({ port: 1 });

      const phases: string[] = [];
      prover.setOnPhase((p) => phases.push(p));
      try {
        const deployed = await deploySchnorrAccount(wallet, feePaymentMethod, "local/WASM");
        expect(deployed).toBeDefined();
      } finally {
        prover.setOnPhase(null);
      }
      // Confirms the fallback path actually engaged (and the discriminator above is meaningful).
      expect(phases).toContain("fallback");
    }, 600_000);
  });
});
