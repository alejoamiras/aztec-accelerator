/**
 * Shared e2e test helpers.
 *
 * Two deployment helpers:
 *   - deploySchnorrAccountSimple(pxe) — simple sandbox deploy via getSchnorrAccount
 *   - deploySchnorrAccount(wallet, fpc, label?) — network-agnostic deploy via EmbeddedWallet + Sponsored FPC
 */

import { getSchnorrAccount } from "@aztec/accounts/testing/lazy";
import type { PXE, Wallet } from "@aztec/aztec.js";
import { AztecAddress, type AztecAddressLike } from "@aztec/aztec.js/addresses";
import type { SponsoredFeePaymentMethod } from "@aztec/aztec.js/fee";
import { Fr } from "@aztec/aztec.js/fields";
import type { EmbeddedWallet } from "@aztec/wallets/embedded";
import { getLogger } from "@logtape/logtape";

const logger = getLogger(["aztec-accelerator", "sdk", "e2e", "helpers"]);

/** Deploy a Schnorr account using simple sandbox pattern. */
export async function deploySchnorrAccountSimple(pxe: PXE): Promise<{
  wallet: Wallet;
  address: AztecAddressLike;
}> {
  const secret = Fr.random();
  const salt = Fr.random();
  const account = await getSchnorrAccount(pxe, secret, salt);
  const wallet = await account.waitSetup();
  return { wallet, address: wallet.getAddress() };
}

/** Deploy a new Schnorr account using the current prover with Sponsored FPC. */
export async function deploySchnorrAccount(
  wallet: EmbeddedWallet,
  feePaymentMethod: SponsoredFeePaymentMethod,
  label?: string,
) {
  const tag = label ? ` (${label})` : "";
  const secret = Fr.random();
  const salt = Fr.random();
  const accountManager = await wallet.createSchnorrAccount(secret, salt);

  logger.debug(`Deploying account${tag}`, { address: accountManager.address.toString() });

  const startTime = Date.now();
  const deployMethod = await accountManager.getDeployMethod();
  const { contract: deployedContract } = await deployMethod.send({
    from: AztecAddress.ZERO,
    skipClassPublication: true,
    fee: { paymentMethod: feePaymentMethod },
  });
  const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);

  logger.info(`Account deployed${tag}`, {
    contract: deployedContract.address?.toString(),
    durationSec: elapsed,
  });

  return deployedContract;
}
