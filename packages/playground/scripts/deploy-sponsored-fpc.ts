/**
 * All-in-one setup: deploy + fund a private SponsoredFPC on Aztec testnet.
 *
 * Both paths (deploy and fund-only) bootstrap an ephemeral Schnorr account
 * funded via L1 bridge. This account pays for all L2 transactions including
 * the FeeJuice.claim() that credits the FPC's balance.
 *
 * Usage:
 *   bun run packages/playground/scripts/deploy-sponsored-fpc.ts
 *
 *   # Reuse a known salt:
 *   bun run packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x1234...
 *
 *   # Skip deploy (FPC already exists), just bridge + claim more Fee Juice:
 *   bun run packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x1234... --fund-only
 *
 *   # Deploy without updating the SPONSORED_FPC_SALT GitHub secret:
 *   bun run packages/playground/scripts/deploy-sponsored-fpc.ts --no-secret
 *
 * Environment (or .env file in packages/playground/scripts/):
 *   L1_PRIVATE_KEY=0x...   Sepolia private key (for minting test FJ on L1)
 *   L1_RPC_URL=https://... Sepolia RPC endpoint
 */

import { execSync } from "node:child_process";
import { AztecAddress } from "@aztec/aztec.js/addresses";
import { L1FeeJuicePortalManager } from "@aztec/aztec.js/ethereum";
import { FeeJuicePaymentMethodWithClaim } from "@aztec/aztec.js/fee";
import { Fr } from "@aztec/aztec.js/fields";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { createLogger } from "@aztec/foundation/log";
import { FeeJuiceContract } from "@aztec/noir-contracts.js/FeeJuice";
import { SponsoredFPCContract } from "@aztec/noir-contracts.js/SponsoredFPC";
import { WASMSimulator } from "@aztec/simulator/client";
import { getContractInstanceFromInstantiationParams } from "@aztec/stdlib/contract";
import type { EmbeddedWallet } from "@aztec/wallets/embedded";
import { createWalletClient, http, publicActions } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { sepolia } from "viem/chains";

// ── CLI args ──────────────────────────────────────────────────────────
const cliArgs = process.argv.slice(2);
const fundOnly = cliArgs.includes("--fund-only");
const noSecret = cliArgs.includes("--no-secret");
const saltIndex = cliArgs.indexOf("--salt");
const salt = saltIndex !== -1 ? Fr.fromHexString(cliArgs[saltIndex + 1]) : Fr.random();

// ── Environment ───────────────────────────────────────────────────────
const nodeUrl = process.env.AZTEC_NODE_URL || "https://rpc.testnet.aztec-labs.com";
const l1RpcUrl = process.env.L1_RPC_URL;
const l1PrivateKey = process.env.L1_PRIVATE_KEY;
const bridgeAmount = process.env.BRIDGE_AMOUNT ? BigInt(process.env.BRIDGE_AMOUNT) : undefined;

if (!l1RpcUrl || !l1PrivateKey) {
  console.error("L1_RPC_URL and L1_PRIVATE_KEY are required.\n");
  console.error("  L1_PRIVATE_KEY=0x... L1_RPC_URL=https://... \\");
  console.error("    bun run packages/playground/scripts/deploy-sponsored-fpc.ts\n");
  process.exit(1);
}

// ── L1 wallet ────────────────────────────────────────────────────────
const l1Account = privateKeyToAccount(l1PrivateKey as `0x${string}`);
const l1Client = createWalletClient({
  account: l1Account,
  chain: sepolia,
  transport: http(l1RpcUrl),
}).extend(publicActions);

const logger = createLogger("deploy-fpc");
const node = createAztecNodeClient(nodeUrl);

// ── Derive FPC address ────────────────────────────────────────────────
const fpcInstance = await getContractInstanceFromInstantiationParams(
  SponsoredFPCContract.artifact,
  {
    salt,
  },
);

console.log("\n  SponsoredFPC Setup");
console.log("  ══════════════════");
console.log(`  Mode:        ${fundOnly ? "fund-only" : "deploy + fund"}`);
console.log(`  L2 Node:     ${nodeUrl}`);
console.log(`  L1 Account:  ${l1Account.address}`);
console.log(`  Salt:        ${salt.toString()}`);
console.log(`  FPC Address: ${fpcInstance.address.toString()}`);
console.log("");

const nodeInfo = await node.getNodeInfo();
console.log(`  Connected — chain ${nodeInfo.l1ChainId}, version ${nodeInfo.nodeVersion}`);

const portalManager = await L1FeeJuicePortalManager.new(node, l1Client, logger);
console.log(`  Fee Juice token: ${portalManager.getTokenManager().tokenAddress.toString()}\n`);

// FeeJuice protocol contract at canonical address 0x05
const FEE_JUICE_ADDRESS = AztecAddress.fromBigInt(5n);

const explorerUrl = (txHash: string) => `https://testnet.aztecscan.xyz/tx-effects/${txHash}`;

// ── Helper: wait for L2 to advance N blocks ───────────────────────────
async function waitForBlocks(n: number, timeoutMs = 600_000) {
  const startBlock = await node.getBlockNumber();
  const target = startBlock + n;
  console.log(`  Waiting for L2 block ${target} (currently ${startBlock})...`);

  const pollStart = Date.now();
  while (Date.now() - pollStart < timeoutMs) {
    const current = await node.getBlockNumber();
    if (current >= target) {
      console.log(`  L2 at block ${current} — ready.`);
      return;
    }
    const elapsed = ((Date.now() - pollStart) / 1000).toFixed(0);
    console.log(`  Block ${current} / ${target}... (${elapsed}s)`);
    await Bun.sleep(15_000);
  }
  throw new Error(`Timed out waiting for L2 block ${target}`);
}

// ── Helper: bootstrap an ephemeral funded account ─────────────────────
// Bridges Fee Juice from L1, deploys a Schnorr account that claims in the same tx.
// Returns the wallet and deployer address for subsequent L2 transactions.
async function bootstrapAccount(): Promise<{
  wallet: EmbeddedWallet;
  deployerAddress: AztecAddress;
}> {
  console.log("  Bootstrapping ephemeral funded account...");
  const { EmbeddedWallet: EW } = await import("@aztec/wallets/embedded");
  const wallet = await EW.create(node, {
    ephemeral: true,
    pxeConfig: { proverEnabled: true },
    pxeOptions: { proverOrOptions: new WASMSimulator() },
  });
  const accountManager = await wallet.createSchnorrAccount(Fr.random(), Fr.random());
  const deployerAddress = accountManager.address;
  console.log(`  Deployer: ${deployerAddress.toString()}`);

  console.log("  Bridging Fee Juice to deployer on L1 (minting)...");
  const claim = await portalManager.bridgeTokensPublic(deployerAddress, bridgeAmount, true);
  console.log(`  Bridged ${claim.claimAmount} to deployer, leaf: ${claim.messageLeafIndex}`);

  await waitForBlocks(3);

  console.log("  Deploying deployer account (WASM proving)...");
  const deployMethod = await accountManager.getDeployMethod();
  const feeMethod = new FeeJuicePaymentMethodWithClaim(deployerAddress, {
    claimAmount: claim.claimAmount,
    claimSecret: claim.claimSecret,
    messageLeafIndex: claim.messageLeafIndex,
  });
  const { receipt } = await deployMethod.send({
    from: AztecAddress.ZERO,
    fee: { paymentMethod: feeMethod },
    wait: { returnReceipt: true },
  });
  console.log(`  Account deployed in block ${receipt.blockNumber}`);
  console.log(`  TX: ${explorerUrl(receipt.txHash.toString())}\n`);

  return { wallet: wallet as EmbeddedWallet, deployerAddress };
}

// ── Helper: bridge Fee Juice to FPC and claim it on L2 ────────────────
async function bridgeAndClaimForFpc(wallet: EmbeddedWallet, deployerAddress: AztecAddress) {
  console.log("  Bridging Fee Juice to FPC on L1 (minting)...");
  const claim = await portalManager.bridgeTokensPublic(fpcInstance.address, bridgeAmount, true);
  console.log(`  Bridged ${claim.claimAmount} to FPC, leaf: ${claim.messageLeafIndex}`);

  await waitForBlocks(3);

  console.log("  Claiming bridged Fee Juice for FPC on L2...");
  const feeJuice = await FeeJuiceContract.at(FEE_JUICE_ADDRESS, wallet as any);
  const { receipt } = await feeJuice.methods
    .claim(fpcInstance.address, claim.claimAmount, claim.claimSecret, claim.messageLeafIndex)
    .send({ from: deployerAddress, wait: { returnReceipt: true } });
  console.log(`  Claimed! tx fee: ${receipt.transactionFee}, block: ${receipt.blockNumber}`);
  console.log(`  TX: ${explorerUrl(receipt.txHash.toString())}\n`);
}

// ── Main flow ─────────────────────────────────────────────────────────
if (!fundOnly) {
  console.log("Step 1: Deploying SponsoredFPC on L2...");

  // Check if already deployed
  let alreadyDeployed = false;
  try {
    const existing = await node.getContract(fpcInstance.address);
    if (existing) {
      console.log("  Already deployed! Skipping to funding.\n");
      alreadyDeployed = true;
    }
  } catch {
    // Not deployed
  }

  const { wallet, deployerAddress } = await bootstrapAccount();

  if (!alreadyDeployed) {
    console.log("  Deploying SponsoredFPC (WASM proving)...");
    const startTime = Date.now();
    const deployMethod = SponsoredFPCContract.deploy(wallet);
    console.log("  Sending deploy tx...");
    const { receipt } = await deployMethod.send({
      from: deployerAddress,
      contractAddressSalt: salt,
      universalDeploy: true,
      skipClassPublication: true,
      wait: { returnReceipt: true },
    });
    const elapsed = ((Date.now() - startTime) / 1000).toFixed(0);
    console.log(`  SponsoredFPC deployed in block ${receipt.blockNumber} (${elapsed}s)`);
    console.log(`  TX: ${explorerUrl(receipt.txHash.toString())}\n`);
  }

  console.log("Step 2: Funding FPC...");
  await bridgeAndClaimForFpc(wallet, deployerAddress);
}

if (fundOnly) {
  console.log("Fund-only mode: bridging + claiming Fee Juice for FPC...\n");
  const { wallet, deployerAddress } = await bootstrapAccount();
  await bridgeAndClaimForFpc(wallet, deployerAddress);
}

// ── Set GitHub secret ─────────────────────────────────────────────────
if (!noSecret) {
  console.log("Setting GitHub secret...");
  try {
    execSync(`gh secret set SPONSORED_FPC_SALT --body "${salt.toString()}"`, { stdio: "inherit" });
    console.log("  SPONSORED_FPC_SALT set.\n");
  } catch {
    console.error("  Failed. Run manually:");
    console.error(`  gh secret set SPONSORED_FPC_SALT --body "${salt.toString()}"\n`);
  }
} else {
  console.log("Skipping GitHub secret update (--no-secret).\n");
}

// ── Done ──────────────────────────────────────────────────────────────
console.log("  ══════════════════════════════════════════════════");
console.log("  DONE!");
console.log(`  Salt:    ${salt.toString()}`);
console.log(`  Address: ${fpcInstance.address.toString()}`);
console.log("  ══════════════════════════════════════════════════\n");
