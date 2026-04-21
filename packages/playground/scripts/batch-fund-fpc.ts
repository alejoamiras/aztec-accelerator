/**
 * Batch fund a SponsoredFPC with large amounts of Fee Juice.
 *
 * Overcomes the 1000 FJ per-mint cap by batch-minting on L1 with
 * concurrent nonces, then bridging the total in a single L1→L2 deposit.
 *
 * Usage:
 *   bun run packages/playground/scripts/batch-fund-fpc.ts --salt 0x1234... --amount 1000000
 *
 *   # Custom batch size (default 50):
 *   bun run packages/playground/scripts/batch-fund-fpc.ts --salt 0x1234... --amount 1000000 --batch-size 100
 *
 * Environment (or .env file in packages/playground/scripts/):
 *   L1_PRIVATE_KEY=0x...   Sepolia private key (for minting test FJ on L1)
 *   L1_RPC_URL=https://... Sepolia RPC endpoint
 */

import { NO_FROM } from "@aztec/aztec.js/account";
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
import { createWalletClient, getContract, http, publicActions } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { sepolia } from "viem/chains";

// ── CLI args ──────────────────────────────────────────────────────────
const cliArgs = process.argv.slice(2);
const saltIndex = cliArgs.indexOf("--salt");
if (saltIndex === -1 || !cliArgs[saltIndex + 1]) {
  console.error("--salt 0x... is required\n");
  console.error(
    "  bun run packages/playground/scripts/batch-fund-fpc.ts --salt 0x1234... --amount 1000000\n",
  );
  process.exit(1);
}
const salt = Fr.fromHexString(cliArgs[saltIndex + 1]);

const amountIndex = cliArgs.indexOf("--amount");
const targetFJ = amountIndex !== -1 ? Number(cliArgs[amountIndex + 1]) : 1_000_000;

const batchSizeIndex = cliArgs.indexOf("--batch-size");
const batchSize = batchSizeIndex !== -1 ? Number(cliArgs[batchSizeIndex + 1]) : 50;

// ── Environment ───────────────────────────────────────────────────────
const nodeUrl = process.env.AZTEC_NODE_URL || "https://rpc.testnet.aztec-labs.com";
const l1RpcUrl = process.env.L1_RPC_URL;
const l1PrivateKey = process.env.L1_PRIVATE_KEY;

if (!l1RpcUrl || !l1PrivateKey) {
  console.error("L1_RPC_URL and L1_PRIVATE_KEY are required.\n");
  console.error("  L1_PRIVATE_KEY=0x... L1_RPC_URL=https://... \\");
  console.error(
    "    bun run packages/playground/scripts/batch-fund-fpc.ts --salt 0x... --amount 1000000\n",
  );
  process.exit(1);
}

// ── L1 wallet ────────────────────────────────────────────────────────
const l1Account = privateKeyToAccount(l1PrivateKey as `0x${string}`);
const l1Client = createWalletClient({
  account: l1Account,
  chain: sepolia,
  transport: http(l1RpcUrl),
}).extend(publicActions);

const logger = createLogger("batch-fund-fpc");
const node = createAztecNodeClient(nodeUrl);

// ── Derive FPC address ───────────────────────────────────────────────
const fpcInstance = await getContractInstanceFromInstantiationParams(
  SponsoredFPCContract.artifact,
  { salt },
);

// ── Get L1 contract addresses from Aztec node ───────────────────────
const nodeInfo = await node.getNodeInfo();

const portalManager = await L1FeeJuicePortalManager.new(node, l1Client, logger);
const tokenManager = portalManager.getTokenManager();

console.log("\n  Batch Fund SponsoredFPC");
console.log("  ══════════════════════════");
console.log(`  FPC Address:  ${fpcInstance.address.toString()}`);
console.log(`  Target:       ${targetFJ.toLocaleString()} FJ`);
console.log(`  L1 Account:   ${l1Account.address}`);
console.log(`  Batch Size:   ${batchSize}`);
console.log(`  L2 Node:      ${nodeUrl}`);
console.log(`  Connected — chain ${nodeInfo.l1ChainId}, version ${nodeInfo.nodeVersion}\n`);

// ── L1 contracts for direct batch minting ───────────────────────────
// Minimal ABIs — only the functions we need for batch minting
const handlerAbi = [
  {
    type: "function",
    name: "mint",
    inputs: [{ name: "_recipient", type: "address" }],
    outputs: [],
    stateMutability: "nonpayable",
  },
  {
    type: "function",
    name: "mintAmount",
    inputs: [],
    outputs: [{ name: "", type: "uint256" }],
    stateMutability: "view",
  },
] as const;

const erc20Abi = [
  {
    type: "function",
    name: "balanceOf",
    inputs: [{ name: "account", type: "address" }],
    outputs: [{ name: "", type: "uint256" }],
    stateMutability: "view",
  },
] as const;

const handler = getContract({
  address: tokenManager.handlerAddress!.toString() as `0x${string}`,
  abi: handlerAbi,
  client: l1Client,
});

const token = getContract({
  address: tokenManager.tokenAddress.toString() as `0x${string}`,
  abi: erc20Abi,
  client: l1Client,
});

// ── Step 1: Batch mint on L1 ────────────────────────────────────────
const mintAmount = await handler.read.mintAmount();
const fjPerMint = Number(mintAmount / 10n ** 18n);
const targetWei = mintAmount * BigInt(Math.ceil(targetFJ / fjPerMint));

// Check existing balance (supports resuming after partial mints)
const existingBalance = await token.read.balanceOf([l1Account.address]);
const existingFJ = Number(existingBalance / 10n ** 18n);
const remainingWei = targetWei > existingBalance ? targetWei - existingBalance : 0n;
const numMints = Number(remainingWei / mintAmount);

if (existingFJ > 0) {
  console.log(`  Existing L1 balance: ${existingFJ.toLocaleString()} FJ`);
}
if (numMints === 0) {
  console.log(`  Already have enough tokens — skipping minting.\n`);
} else {
  console.log(
    `Step 1: Batch minting ${numMints} × ${fjPerMint} FJ = ${(numMints * fjPerMint).toLocaleString()} FJ on L1...\n`,
  );
}

const startTime = Date.now();

// Helper: wait for tx receipt with retries (Sepolia mempool can be slow)
async function waitForReceipt(hash: `0x${string}`, maxRetries = 3) {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      return await l1Client.waitForTransactionReceipt({ hash, timeout: 300_000 });
    } catch (err) {
      if (attempt === maxRetries) throw err;
      console.log(`  Receipt timeout (attempt ${attempt}/${maxRetries}), retrying...`);
    }
  }
}

// Submit txs sequentially within small batches (avoids mempool congestion).
// Wait for mining at end of each batch. Smaller batches keep pending count low.
const effectiveBatch = Math.min(batchSize, 20); // cap at 20 to avoid mempool issues
for (let batch = 0; batch < numMints; batch += effectiveBatch) {
  const count = Math.min(effectiveBatch, numMints - batch);
  const batchNum = Math.floor(batch / effectiveBatch) + 1;
  const totalBatches = Math.ceil(numMints / effectiveBatch);
  const elapsed = ((Date.now() - startTime) / 1000).toFixed(0);
  console.log(`  Batch ${batchNum}/${totalBatches}: submitting ${count} mint txs... (${elapsed}s)`);

  const hashes: `0x${string}`[] = [];
  for (let i = 0; i < count; i++) {
    const hash = await handler.write.mint([l1Account.address]);
    hashes.push(hash);
  }

  // Wait for the last tx — all prior nonces must be mined for it to confirm
  await waitForReceipt(hashes[hashes.length - 1]);
}

const l1Balance = await token.read.balanceOf([l1Account.address]);
const bridgeAmount = l1Balance; // bridge everything we have
console.log(`\n  L1 balance: ${Number(l1Balance / 10n ** 18n).toLocaleString()} FJ\n`);

// ── Step 2: Bridge total to FPC (single L1 deposit) ─────────────────
console.log(
  `Step 2: Bridging ${Number(bridgeAmount / 10n ** 18n).toLocaleString()} FJ to FPC on L1...`,
);
const claim = await portalManager.bridgeTokensPublic(
  fpcInstance.address,
  bridgeAmount,
  false, // skip mint — we already have the tokens
);
console.log(`  Bridged ${claim.claimAmount} wei to FPC, leaf: ${claim.messageLeafIndex}\n`);

// ── Step 3: Bootstrap ephemeral L2 account (for claiming) ───────────
console.log("Step 3: Bootstrapping ephemeral L2 account...");

const FEE_JUICE_ADDRESS = AztecAddress.fromBigInt(5n);

const explorerUrl = (txHash: string) => `https://testnet.aztecscan.xyz/tx-effects/${txHash}`;

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

const { EmbeddedWallet: EW } = await import("@aztec/wallets/embedded");
const wallet = await EW.create(node, {
  ephemeral: true,
  pxe: {
    proverEnabled: true,
    proverOrOptions: new WASMSimulator(),
  },
});
const accountManager = await wallet.createSchnorrAccount(Fr.random(), Fr.random());
const deployerAddress = accountManager.address;
console.log(`  Deployer: ${deployerAddress.toString()}`);

console.log("  Bridging 1000 FJ to deployer for tx fees...");
const deployerClaim = await portalManager.bridgeTokensPublic(
  deployerAddress,
  undefined,
  true, // mint 1000 FJ for deployer
);
console.log(
  `  Bridged ${deployerClaim.claimAmount} to deployer, leaf: ${deployerClaim.messageLeafIndex}`,
);

await waitForBlocks(3);

console.log("  Deploying deployer account (WASM proving)...");
const deployMethod = await accountManager.getDeployMethod();
const feeMethod = new FeeJuicePaymentMethodWithClaim(deployerAddress, {
  claimAmount: deployerClaim.claimAmount,
  claimSecret: deployerClaim.claimSecret,
  messageLeafIndex: deployerClaim.messageLeafIndex,
});
const { receipt: deployReceipt } = await deployMethod.send({
  from: NO_FROM,
  fee: { paymentMethod: feeMethod },
  wait: { returnReceipt: true },
});
console.log(`  Account deployed in block ${deployReceipt.blockNumber}`);
console.log(`  TX: ${explorerUrl(deployReceipt.txHash.toString())}\n`);

// ── Step 4: Claim bridged FJ for FPC on L2 ──────────────────────────
const fundedFJ = Number(bridgeAmount / 10n ** 18n);
console.log(`Step 4: Claiming ${fundedFJ.toLocaleString()} FJ for FPC on L2...`);

// Wait for the FPC bridge message to be processable (may need more blocks)
await waitForBlocks(2);

const feeJuice = await FeeJuiceContract.at(FEE_JUICE_ADDRESS, wallet as any);
const { receipt: claimReceipt } = await feeJuice.methods
  .claim(fpcInstance.address, claim.claimAmount, claim.claimSecret, claim.messageLeafIndex)
  .send({ from: deployerAddress, wait: { returnReceipt: true } });
console.log(
  `  Claimed! tx fee: ${claimReceipt.transactionFee}, block: ${claimReceipt.blockNumber}`,
);
console.log(`  TX: ${explorerUrl(claimReceipt.txHash.toString())}\n`);

// ── Done ─────────────────────────────────────────────────────────────
const totalElapsed = ((Date.now() - startTime) / 1000).toFixed(0);
console.log("  ══════════════════════════════════════════════════");
console.log("  DONE!");
console.log(`  FPC Address: ${fpcInstance.address.toString()}`);
console.log(`  Funded:      ${fundedFJ.toLocaleString()} FJ`);
console.log(`  Time:        ${totalElapsed}s`);
console.log("  ══════════════════════════════════════════════════\n");
