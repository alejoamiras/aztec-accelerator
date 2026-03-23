import {
  type AcceleratorPhase,
  type AcceleratorPhaseData,
  AcceleratorProver,
} from "@alejoamiras/aztec-accelerator";
import { getInitialTestAccountsData } from "@aztec/accounts/testing/lazy";
import { AztecAddress } from "@aztec/aztec.js/addresses";
import { NO_WAIT } from "@aztec/aztec.js/contracts";
import { SponsoredFeePaymentMethod } from "@aztec/aztec.js/fee";
import { Fr } from "@aztec/aztec.js/fields";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import type { TxHash } from "@aztec/aztec.js/tx";
import type { Wallet } from "@aztec/aztec.js/wallet";
import { createStore } from "@aztec/kv-store/indexeddb";
import { SponsoredFPCContract } from "@aztec/noir-contracts.js/SponsoredFPC";
import { TokenContract } from "@aztec/noir-contracts.js/Token";
import { createPXE, getPXEConfig } from "@aztec/pxe/client/lazy";
import { getContractInstanceFromInstantiationParams } from "@aztec/stdlib/contract";
import { EmbeddedWallet, WalletDB } from "@aztec/wallets/embedded";

export type LogFn = (
  msg: string,
  level?: "info" | "warn" | "error" | "success",
  url?: string,
) => void;

/** Truncate an address for display: first 6 + last 4 chars. */
function shortAddr(addr: string): string {
  return addr.length > 14 ? `${addr.slice(0, 6)}...${addr.slice(-4)}` : addr;
}

/** Extract a human-readable message from an unknown error value. */
function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export type UiMode = "local" | "accelerated";

const AZTEC_NODE_URL = process.env.AZTEC_NODE_URL || "/aztec";

/** Display-friendly Aztec node URL for the services panel (reads env set by Vite define). */
export const AZTEC_DISPLAY_URL = process.env.AZTEC_NODE_URL || "localhost:8080";

// ── Retry & timeout constants ──
const MAX_INIT_ATTEMPTS = 3;
const MAX_SEND_ATTEMPTS = 3;
const TX_TIMEOUT_MS = 10 * 60 * 1_000; // 10 minutes
const TX_POLL_INTERVAL_MS = 1_000;
const BLOCK_HEADER_NOT_FOUND = "Block header not found";
/** Retry stale block headers only in E2E tests — not safe in production
 *  because re-simulating may pick up different contract state. */
const RETRY_STALE_HEADER = !!process.env.E2E_RETRY_STALE_HEADER;

/** @aztec/stdlib version from SDK package.json, embedded at build time */
export const AZTEC_SDK_VERSION = process.env.VITE_AZTEC_SDK_VERSION || "unknown";

export interface AztecState {
  node: ReturnType<typeof createAztecNodeClient> | null;
  prover: AcceleratorProver | null;
  wallet: Wallet | null;
  embeddedWallet: EmbeddedWallet | null;
  registeredAddresses: AztecAddress[];
  selectedAccountIndex: number;
  uiMode: UiMode;
  /** True when the network requires real proofs (not simulated). */
  proofsRequired: boolean;
  feePaymentMethod: SponsoredFeePaymentMethod | undefined;
}

/**
 * Global mutable application state. Concurrent mutations are prevented at
 * the UI layer via the `deploying` flag in main.ts, which disables action
 * buttons while an async operation is in flight.
 */
export const state: AztecState = {
  node: null,
  prover: null,
  wallet: null,
  embeddedWallet: null,
  registeredAddresses: [],
  selectedAccountIndex: 0,
  uiMode: "accelerated",
  proofsRequired: false,
  feePaymentMethod: undefined,
};

export async function checkAccelerator(): Promise<boolean> {
  try {
    // Probe both HTTP (59833) and HTTPS (59834) in parallel.
    // Chrome/Firefox: HTTP responds instantly. Safari: HTTPS needed (mixed-content block).
    const { res } = await Promise.any([
      fetch("http://127.0.0.1:59833/health", { signal: AbortSignal.timeout(2000) }).then((res) => ({
        res,
        protocol: "http" as const,
      })),
      fetch("https://127.0.0.1:59834/health", { signal: AbortSignal.timeout(2000) }).then(
        (res) => ({ res, protocol: "https" as const }),
      ),
    ]);
    return res.ok;
  } catch {
    return false;
  }
}

export async function checkAztecNode(): Promise<{ reachable: boolean; nodeVersion?: string }> {
  try {
    const res = await fetch(`${AZTEC_NODE_URL}/status`, {
      signal: AbortSignal.timeout(5000),
    });
    if (!res.ok) return { reachable: false };

    // Try to get the node version via JSON-RPC
    try {
      const rpc = await fetch(AZTEC_NODE_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", method: "node_getNodeInfo", params: [], id: 1 }),
        signal: AbortSignal.timeout(5000),
      });
      if (rpc.ok) {
        const data = await rpc.json();
        return { reachable: true, nodeVersion: data.result?.nodeVersion };
      }
    } catch {
      // RPC failed but /status was ok — node is reachable but version unknown
    }

    return { reachable: true };
  } catch {
    return { reachable: false };
  }
}

async function clearIndexedDB(): Promise<void> {
  const dbs = await indexedDB.databases();
  const aztecPrefixes = ["pxe-", "wallet-", "aztec-"];
  await Promise.all(
    dbs
      .filter((db) => db.name && aztecPrefixes.some((prefix) => db.name!.startsWith(prefix)))
      .map((db) => indexedDB.deleteDatabase(db.name!)),
  );
}

/**
 * Lazy account contracts provider — mirrors @aztec/wallets internal implementation.
 * Uses dynamic imports so Vite can code-split the account contract artifacts.
 */
const lazyAccountContracts = {
  async getSchnorrAccountContract(signingKey: import("@aztec/foundation/curves/bn254").Fq) {
    const { SchnorrAccountContract } = await import("@aztec/accounts/schnorr/lazy");
    return new SchnorrAccountContract(signingKey);
  },
  async getEcdsaRAccountContract(signingKey: Buffer) {
    const { EcdsaRAccountContract } = await import("@aztec/accounts/ecdsa/lazy");
    return new EcdsaRAccountContract(signingKey);
  },
  async getEcdsaKAccountContract(signingKey: Buffer) {
    const { EcdsaKAccountContract } = await import("@aztec/accounts/ecdsa/lazy");
    return new EcdsaKAccountContract(signingKey);
  },
  async getStubAccountContractArtifact() {
    const { getStubAccountContractArtifact } = await import("@aztec/accounts/stub/lazy");
    return getStubAccountContractArtifact();
  },
  async createStubAccount(address: import("@aztec/stdlib/contract").CompleteAddress) {
    const { createStubAccount } = await import("@aztec/accounts/stub/lazy");
    return createStubAccount(address);
  },
  async getMulticallContract() {
    const { getCanonicalMultiCallEntrypoint } = await import(
      "@aztec/protocol-contracts/multi-call-entrypoint/lazy"
    );
    return getCanonicalMultiCallEntrypoint();
  },
};

/**
 * Connect to the Aztec node and set up the AcceleratorProver.
 * Shared by both embedded and external wallet paths.
 */
export async function initializeNode(log: LogFn): Promise<{
  rollupAddress: import("@aztec/aztec.js/addresses").EthAddress;
  l1Contracts: Awaited<
    ReturnType<ReturnType<typeof createAztecNodeClient>["getL1ContractAddresses"]>
  >;
}> {
  log("Creating AcceleratorProver...");
  state.prover = new AcceleratorProver();

  log("Connecting to Aztec node...");
  state.node = createAztecNodeClient(AZTEC_NODE_URL);
  const [nodeInfo, l1Contracts] = await Promise.all([
    state.node.getNodeInfo(),
    state.node.getL1ContractAddresses(),
  ]);

  const rollupAddress = l1Contracts.rollupAddress;
  state.proofsRequired = nodeInfo.l1ChainId !== 31337;

  // Allow forcing proofs via ?forceProofs=true for testing IVC locally
  const forceProofs = new URLSearchParams(window.location.search).get("forceProofs") === "true";
  if (forceProofs && !state.proofsRequired) {
    state.proofsRequired = true;
    log("Forced proverEnabled=true via ?forceProofs query param", "warn");
  }

  log(
    `Connected — chain ${nodeInfo.l1ChainId} (proofs ${state.proofsRequired ? "required" : "simulated"})`,
    "success",
  );

  return { rollupAddress, l1Contracts };
}

/**
 * Derive the Sponsored FPC address and register it in the PXE.
 * Shared by both embedded and external wallet paths.
 *
 * Uses SPONSORED_FPC_SALT env var when set (for private FPC instances on live
 * networks). Defaults to salt=0 (canonical FPC) for local sandbox.
 */
export async function initializeFPC(wallet: Wallet, log: LogFn): Promise<void> {
  log("Setting up Sponsored FPC...");
  const saltHex = process.env.SPONSORED_FPC_SALT;
  const salt = saltHex ? Fr.fromHexString(saltHex) : new Fr(0);
  const fpcInstance = await getContractInstanceFromInstantiationParams(
    SponsoredFPCContract.artifact,
    { salt },
  );
  await wallet.registerContract(fpcInstance, SponsoredFPCContract.artifact);
  state.feePaymentMethod = new SponsoredFeePaymentMethod(fpcInstance.address);
  log("Sponsored FPC registered", "success");
}

async function doInitializeWallet(log: LogFn): Promise<boolean> {
  const { rollupAddress, l1Contracts } = await initializeNode(log);

  log("Creating wallet (may take a moment)...");
  // Mirrors BrowserEmbeddedWallet.create() but injects our AcceleratorProver.
  // Pass l1Contracts in the config to avoid extra fetches during PXE init.
  // Only enable proving on live networks (sandbox uses simulated proofs).
  const pxeConfig = getPXEConfig();
  pxeConfig.dataDirectory = `pxe-${rollupAddress}`;
  pxeConfig.proverEnabled = state.proofsRequired;
  pxeConfig.l1Contracts = l1Contracts;

  log("Initializing PXE...");
  const pxe = await createPXE(state.node, pxeConfig, {
    proverOrOptions: state.prover,
  });
  log("PXE initialized", "success");

  log("Creating wallet DB...");
  const walletDBStore = await createStore(`wallet-${rollupAddress}`, {
    dataDirectory: "wallet",
    dataStoreMapSizeKb: 2e10,
  });
  const walletDB = WalletDB.init(walletDBStore, (msg) => log(msg));
  log("Wallet DB created", "success");

  state.embeddedWallet = new EmbeddedWallet(pxe, state.node, walletDB, lazyAccountContracts);
  state.wallet = state.embeddedWallet;
  log("Wallet created", "success");

  await initializeFPC(state.wallet!, log);

  if (!state.proofsRequired) {
    log("Registering sandbox accounts...");
    // Register accounts serially to avoid IndexedDB TransactionInactiveError.
    const testAccounts = await getInitialTestAccountsData();
    state.registeredAddresses = [];
    for (const account of testAccounts) {
      const mgr = await state.embeddedWallet!.createSchnorrAccount(
        account.secret,
        account.salt,
        account.signingKey,
      );
      state.registeredAddresses.push(mgr.address);
    }
    log(`Registered ${state.registeredAddresses.length} accounts`, "success");
  }

  return true;
}

export async function initializeWallet(log: LogFn): Promise<boolean> {
  for (let attempt = 1; attempt <= MAX_INIT_ATTEMPTS; attempt++) {
    try {
      return await doInitializeWallet(log);
    } catch (err) {
      if (attempt < MAX_INIT_ATTEMPTS) {
        log(
          `Wallet init failed (attempt ${attempt}/${MAX_INIT_ATTEMPTS}), clearing stale data...`,
          "warn",
        );
        await clearIndexedDB();
      } else {
        log(`Wallet initialization failed: ${errorMessage(err)}`, "error");
        return false;
      }
    }
  }
  return false;
}

export function setUiMode(mode: UiMode): void {
  state.uiMode = mode;
  state.prover?.setForceLocal(mode === "local");
}

export interface SimStepDetail {
  syncMs: number;
  totalMs: number;
  perFunction: { name: string; ms: number }[];
}

export interface StepTiming {
  step: string;
  durationMs: number;
  simulation?: SimStepDetail;
  proveMs?: number;
  proveSendMs?: number;
  confirmMs?: number;
}

const EXPLORER_BASE = "https://testnet.aztecscan.xyz/tx-effects";

/** Captured from the `"proved"` phase callback — read and reset after each send. */
let _lastProveMs: number | undefined;

export interface DeployResult {
  address: string;
  steps: StepTiming[];
  totalDurationMs: number;
  mode: UiMode;
}

export interface TokenFlowResult {
  mode: UiMode;
  steps: StepTiming[];
  totalDurationMs: number;
  aliceBalance: bigint;
  bobBalance: bigint;
  tokenAddress: string;
}

interface SimTimings {
  sync?: number;
  total?: number;
  perFunction?: { functionName: string; time: number }[];
}

/** Extract our SimStepDetail from a simulate() result with includeMetadata: true. */
function extractSimDetail(simResult: { stats: { timings: SimTimings } }): SimStepDetail {
  const t = simResult.stats.timings;
  return {
    syncMs: t.sync ?? 0,
    totalMs: t.total ?? 0,
    perFunction: (t.perFunction ?? []).map((f) => ({ name: f.functionName, ms: f.time })),
  };
}

/**
 * Send a tx, optionally retrying on "Block header not found" when enabled
 * via E2E_RETRY_STALE_HEADER. Re-simulates to refresh the block header when
 * proving takes long enough for it to go stale on live networks.
 */
async function sendWithRetry(
  method: { simulate: (opts: any) => Promise<any>; send: (opts: any) => Promise<any> },
  sendOpts: Record<string, unknown>,
  log: LogFn,
): Promise<TxHash> {
  const maxAttempts = RETRY_STALE_HEADER ? MAX_SEND_ATTEMPTS : 1;
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      if (attempt > 1) {
        log(`Retrying (attempt ${attempt}/${maxAttempts}) — refreshing block header...`, "warn");
        await method.simulate(sendOpts);
      }
      const { txHash } = await method.send({ ...sendOpts, wait: NO_WAIT });
      return txHash;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (attempt < maxAttempts && msg.includes(BLOCK_HEADER_NOT_FOUND)) {
        log(`Block header went stale during proving (attempt ${attempt}/${maxAttempts})`, "warn");
        continue;
      }
      throw err;
    }
  }
  throw new Error("Unreachable");
}

/** Simulate → prove/send → confirm a transaction and return a StepTiming. */
async function executeStep(opts: {
  step: string;
  method: { simulate: (opts: any) => Promise<any>; send: (opts: any) => Promise<any> };
  sendOpts: Record<string, unknown>;
  log: LogFn;
  onConfirming: () => void;
}): Promise<{ timing: StepTiming; txHash: string }> {
  const { step, method, sendOpts, log, onConfirming } = opts;
  const stepStart = Date.now();

  const simResult = await method.simulate({ ...sendOpts, includeMetadata: true });
  const simulation = extractSimDetail(simResult);

  const sendStart = Date.now();
  const hash = await sendWithRetry(method, sendOpts, log);
  const proveSendMs = Date.now() - sendStart;
  const proveMs = _lastProveMs;
  _lastProveMs = undefined;

  onConfirming();
  const confirmStart = Date.now();
  await waitForTx(hash);
  const confirmMs = Date.now() - confirmStart;

  return {
    timing: {
      step,
      durationMs: Date.now() - stepStart,
      simulation,
      proveMs,
      proveSendMs,
      confirmMs,
    },
    txHash: hash.toString(),
  };
}

/** Poll until a transaction is no longer pending. Throws on dropped or timed-out txs. */
async function waitForTx(txHash: TxHash): Promise<void> {
  const deadline = Date.now() + TX_TIMEOUT_MS;
  while (true) {
    const receipt = await state.node!.getTxReceipt(txHash);
    if (!receipt.isPending()) {
      if (receipt.isDropped()) throw new Error("Transaction dropped");
      return;
    }
    if (Date.now() > deadline) {
      throw new Error("Transaction confirmation timed out after 10 minutes");
    }
    await new Promise((r) => setTimeout(r, TX_POLL_INTERVAL_MS));
  }
}

export async function deployTestAccount(
  log: LogFn,
  onTick: (elapsedMs: number) => void,
  onStep: (stepName: string) => void,
  onPhase?: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void,
): Promise<DeployResult> {
  if (!state.embeddedWallet) {
    throw new Error("Embedded wallet not initialized");
  }
  if (!state.proofsRequired && !state.registeredAddresses.length) {
    throw new Error("Wallet not initialized — no registered addresses");
  }

  const mode = state.uiMode;
  const steps: StepTiming[] = [];
  const totalStart = Date.now();
  state.prover?.setOnPhase(
    onPhase
      ? (phase, data) => {
          if (phase === "proved" && data?.durationMs) _lastProveMs = data.durationMs;
          onPhase(phase, data);
        }
      : null,
  );

  const interval = setInterval(() => {
    onTick(Date.now() - totalStart);
  }, 100);

  try {
    // Step 1: Create account
    onStep("creating account");
    log("Creating Schnorr account...");
    let stepStart = Date.now();

    const secret = Fr.random();
    const salt = Fr.random();
    const accountManager = await state.embeddedWallet!.createSchnorrAccount(secret, salt);
    const deployMethod = await accountManager.getDeployMethod();

    steps.push({ step: "create account", durationMs: Date.now() - stepStart });
    log(`Account: ${shortAddr(accountManager.address.toString())}`);

    const sendOpts = {
      from: state.proofsRequired ? AztecAddress.ZERO : state.registeredAddresses[0],
      skipClassPublication: true,
      fee: { paymentMethod: state.feePaymentMethod! },
      // Account constructor initializes private storage — needs its own nullifier key in scope.
      additionalScopes: [accountManager.address],
    };

    // Step 2: Simulate (captures witness gen timing)
    // Simulate may fail with AztecAddress.ZERO (first deploy on live networks)
    onStep("simulating deploy");
    log("Simulating deploy...");
    stepStart = Date.now();

    let simDetail: SimStepDetail | undefined;
    try {
      const simResult = await deployMethod.simulate({ ...sendOpts, includeMetadata: true });
      simDetail = extractSimDetail(simResult);
    } catch {
      log("Simulation stats unavailable (first deploy)", "warn");
    }

    steps.push({
      step: "simulate",
      durationMs: Date.now() - stepStart,
      simulation: simDetail,
    });

    // Step 3: Prove + send + confirm
    onStep("proving + sending");
    log("Proving and sending...");
    stepStart = Date.now();

    const txHash = await sendWithRetry(deployMethod, sendOpts, log);
    const proveSendMs = Date.now() - stepStart;
    const proveMs = _lastProveMs;
    _lastProveMs = undefined;

    onStep("confirming");
    const confirmStart = Date.now();
    await waitForTx(txHash);
    const confirmMs = Date.now() - confirmStart;

    const deployTxHash = txHash.toString();

    steps.push({
      step: "prove + send",
      durationMs: proveSendMs + confirmMs,
      proveMs,
      proveSendMs,
      confirmMs,
    });

    const totalDurationMs = Date.now() - totalStart;
    const address = accountManager.address.toString();
    log(
      `Deployed in ${(totalDurationMs / 1000).toFixed(1)}s → ${shortAddr(address)}`,
      "success",
      `${EXPLORER_BASE}/${deployTxHash}`,
    );

    // On live networks, store the deployed address for use in subsequent operations
    if (state.proofsRequired) {
      state.registeredAddresses.push(accountManager.address);
    }

    return { address, steps, totalDurationMs, mode };
  } finally {
    state.prover?.setOnPhase(null);
    clearInterval(interval);
  }
}

export async function deployToken(
  log: LogFn,
  onTick: (elapsedMs: number) => void,
  onStep: (stepName: string) => void,
  onPhase?: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void,
): Promise<DeployResult> {
  if (!state.wallet || state.registeredAddresses.length < 1) {
    throw new Error("Wallet not initialized — no registered addresses");
  }

  const mode = state.uiMode;
  const alice = state.registeredAddresses[state.selectedAccountIndex];
  const steps: StepTiming[] = [];
  const totalStart = Date.now();
  state.prover?.setOnPhase(
    onPhase
      ? (phase, data) => {
          if (phase === "proved" && data?.durationMs) _lastProveMs = data.durationMs;
          onPhase(phase, data);
        }
      : null,
  );

  const interval = setInterval(() => {
    onTick(Date.now() - totalStart);
  }, 100);

  try {
    onStep("deploying token");
    log("Deploying TokenContract (admin=Alice)...");
    const tokenDeploy = TokenContract.deploy(state.wallet, alice, "Accelerator", "ACEL", 18);
    const { timing: tokenStep, txHash: tokenTxHash } = await executeStep({
      step: "deploy token",
      method: tokenDeploy,
      sendOpts: { from: alice, fee: { paymentMethod: state.feePaymentMethod! } },
      log,
      onConfirming: () => onStep("confirming token deploy"),
    });
    steps.push(tokenStep);

    const address = tokenDeploy.address!.toString();
    const totalDurationMs = Date.now() - totalStart;
    log(
      `Token deployed in ${(totalDurationMs / 1000).toFixed(1)}s → ${shortAddr(address)}`,
      "success",
      `${EXPLORER_BASE}/${tokenTxHash}`,
    );

    return { address, steps, totalDurationMs, mode };
  } finally {
    state.prover?.setOnPhase(null);
    clearInterval(interval);
  }
}

export async function runTokenFlow(
  log: LogFn,
  onTick: (elapsedMs: number) => void,
  onStep: (stepName: string) => void,
  onPhase?: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void,
): Promise<TokenFlowResult> {
  if (!state.wallet || state.registeredAddresses.length < 1) {
    throw new Error("Wallet not initialized — deploy at least one account first");
  }

  const mode = state.uiMode;
  const alice = state.registeredAddresses[state.selectedAccountIndex];
  const fee = { paymentMethod: state.feePaymentMethod! };
  const steps: StepTiming[] = [];
  const totalStart = Date.now();
  state.prover?.setOnPhase(
    onPhase
      ? (phase, data) => {
          if (phase === "proved" && data?.durationMs) _lastProveMs = data.durationMs;
          onPhase(phase, data);
        }
      : null,
  );

  const interval = setInterval(() => {
    onTick(Date.now() - totalStart);
  }, 100);

  try {
    // On live networks, we may need to deploy a second account (bob) for the transfer step
    let bob: AztecAddress;
    const bobCandidate = state.registeredAddresses.find((_, i) => i !== state.selectedAccountIndex);
    if (bobCandidate) {
      bob = bobCandidate;
    } else {
      onStep("deploying bob account");
      log("Deploying second account (Bob) for transfer...");
      const bobManager = await state.embeddedWallet!.createSchnorrAccount(Fr.random(), Fr.random());
      const bobDeploy = await bobManager.getDeployMethod();
      // Account constructor initializes private storage — needs its own nullifier key in scope.
      const bobSendOpts = {
        from: AztecAddress.ZERO,
        skipClassPublication: true,
        fee,
        additionalScopes: [bobManager.address],
      };
      const { timing: bobStep, txHash: bobTxHash } = await executeStep({
        step: "deploy bob",
        method: bobDeploy,
        sendOpts: bobSendOpts,
        log,
        onConfirming: () => onStep("confirming bob"),
      });
      bob = bobManager.address;
      state.registeredAddresses.push(bob);
      steps.push(bobStep);
      log(
        `Bob deployed in ${(bobStep.durationMs / 1000).toFixed(1)}s`,
        "success",
        `${EXPLORER_BASE}/${bobTxHash}`,
      );
    }

    // Step 1: Deploy TokenContract
    onStep("deploying token");
    log("Deploying TokenContract (admin=Alice)...");
    const tokenDeploy = TokenContract.deploy(state.wallet, alice, "Accelerator", "ACEL", 18);
    const { timing: tokenStep, txHash: tokenTxHash } = await executeStep({
      step: "deploy token",
      method: tokenDeploy,
      sendOpts: { from: alice, fee },
      log,
      onConfirming: () => onStep("confirming token deploy"),
    });
    const token = TokenContract.at(tokenDeploy.address!, state.wallet);
    steps.push(tokenStep);
    log(
      `Token deployed in ${(tokenStep.durationMs / 1000).toFixed(1)}s → ${shortAddr(token.address.toString())}`,
      "success",
      `${EXPLORER_BASE}/${tokenTxHash}`,
    );

    // Step 2: Mint 1000 ACEL to Alice (private)
    onStep("minting 1000 ACEL");
    log("Minting 1000 ACEL to Alice...");
    const { timing: mintStep, txHash: mintTxHash } = await executeStep({
      step: "mint to private",
      method: token.methods.mint_to_private(alice, 1000n),
      sendOpts: { from: alice, fee },
      log,
      onConfirming: () => onStep("confirming mint"),
    });
    steps.push(mintStep);
    log(
      `Minted in ${(mintStep.durationMs / 1000).toFixed(1)}s`,
      "success",
      `${EXPLORER_BASE}/${mintTxHash}`,
    );

    // Step 3: Transfer 500 ACEL Alice → Bob (private)
    onStep("transferring 500 ACEL");
    log("Transferring 500 ACEL Alice → Bob...");
    const { timing: transferStep, txHash: transferTxHash } = await executeStep({
      step: "private transfer",
      method: token.methods.transfer(bob, 500n),
      sendOpts: { from: alice, fee },
      log,
      onConfirming: () => onStep("confirming transfer"),
    });
    steps.push(transferStep);
    log(
      `Transferred in ${(transferStep.durationMs / 1000).toFixed(1)}s`,
      "success",
      `${EXPLORER_BASE}/${transferTxHash}`,
    );

    // Step 4: Check balances (simulate, no proof needed)
    onStep("checking balances");
    log("Checking balances...");
    const balanceStart = Date.now();
    const [{ result: aliceBalance }, { result: bobBalance }] = await Promise.all([
      token.methods.balance_of_private(alice).simulate({ from: alice }),
      token.methods.balance_of_private(bob).simulate({ from: bob }),
    ]);
    const balanceDuration = Date.now() - balanceStart;
    steps.push({ step: "check balances", durationMs: balanceDuration });
    log(
      `Balances — Alice: ${aliceBalance}, Bob: ${bobBalance} (${(balanceDuration / 1000).toFixed(1)}s)`,
      "success",
    );

    const totalDurationMs = Date.now() - totalStart;
    log(`Token flow complete in ${(totalDurationMs / 1000).toFixed(1)}s`, "success");

    return {
      mode,
      steps,
      totalDurationMs,
      aliceBalance: BigInt(aliceBalance.toString()),
      bobBalance: BigInt(bobBalance.toString()),
      tokenAddress: token.address.toString(),
    };
  } finally {
    state.prover?.setOnPhase(null);
    clearInterval(interval);
  }
}
