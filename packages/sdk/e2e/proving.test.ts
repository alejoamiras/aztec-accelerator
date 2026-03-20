/**
 * AcceleratorProver proving e2e tests
 *
 * One shared setup (prover + wallet + Sponsored FPC), then deploys an account
 * in each mode:
 *   - Accelerated: real accelerator desktop app (skipped when ACCELERATOR_URL not set)
 *   - Local (WASM): fallback via unreachable port
 *
 * Network-agnostic: always uses Sponsored FPC + from: AztecAddress.ZERO.
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
        pxeConfig: { proverEnabled: true },
        pxeOptions: { proverOrOptions: prover },
      });

      // Derive Sponsored FPC address and register in PXE.
      // Uses SPONSORED_FPC_SALT when set (private FPC on live networks),
      // defaults to salt=0 (canonical FPC for local sandbox).
      const saltHex = process.env.SPONSORED_FPC_SALT;
      const fpcSalt = saltHex ? Fr.fromHexString(saltHex) : new Fr(0);
      const fpcInstance = await getContractInstanceFromInstantiationParams(
        SponsoredFPCContract.artifact,
        { salt: fpcSalt },
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

    test("should deploy account with accelerated proving", async () => {
      expect(wallet).toBeDefined();

      const deployed = await deploySchnorrAccount(wallet, feePaymentMethod, "accelerated");
      expect(deployed).toBeDefined();
    }, 600_000);
  });

  describe("Local (WASM)", () => {
    test("should deploy account with local proving", async () => {
      expect(wallet).toBeDefined();

      // Force WASM fallback by pointing at an unreachable port
      prover.setAcceleratorConfig({ port: 1 });

      const deployed = await deploySchnorrAccount(wallet, feePaymentMethod, "local/WASM");
      expect(deployed).toBeDefined();
    }, 600_000);
  });
});
