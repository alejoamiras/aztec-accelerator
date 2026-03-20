import { getInitialTestAccountsData, getSchnorrAccount } from "@aztec/accounts/testing/lazy";
import type { PXE, Wallet } from "@aztec/aztec.js";
import { type AztecAddress, Fr } from "@aztec/aztec.js";

/** Deploy a Schnorr account and return the wallet + address. */
export async function deploySchnorrAccount(pxe: PXE): Promise<{
  wallet: Wallet;
  address: AztecAddress;
}> {
  const secret = Fr.random();
  const salt = Fr.random();
  const account = await getSchnorrAccount(pxe, secret, salt);
  const wallet = await account.waitSetup();
  return { wallet, address: wallet.getAddress() };
}

/** Get the initial test account data for the sandbox. */
export { getInitialTestAccountsData };
