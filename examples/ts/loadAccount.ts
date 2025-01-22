import * as dotenv from 'dotenv';
import { IotaClient } from '@iota/iota-sdk/client';
import { Ed25519Keypair, Ed25519PublicKey } from '@iota/iota-sdk/keypairs/ed25519';
import { decodeIotaPrivateKey } from '@iota/iota-sdk/cryptography';

// Define account interface
export interface Account {
  role: string;
  keypair: Ed25519Keypair;
  publicKey: Ed25519PublicKey;
  address: string;
  balance: string;
}

// Loads an account from a Bech32 formatted Iota private key "iotaprivkey..."
export async function loadAccountFromKey(role: string, key: string): Promise<Account | undefined> {
  try {
    // Load environment variables
    dotenv.config();

    // Import env variable: node url
    const nodeUrl = process.env.NODE as string;

    // Import env variable: faucet url
    const faucetUrl = process.env.FAUCET as string;

    // Create a new IotaClient object pointing to the network you want to use
    const client = new IotaClient({ url: nodeUrl });

    // Load account from the private key
    const decodedKey = decodeIotaPrivateKey(key);
    const keypair = Ed25519Keypair.fromSecretKey(decodedKey.secretKey);
    const publicKey = keypair.getPublicKey();
    const address = publicKey.toIotaAddress();
    const balance = await client.getBalance({ owner: address });

    // Map account
    const account: Account = {
      role: role,
      keypair: keypair,
      publicKey: publicKey,
      address: address,
      balance: balance.totalBalance
    };

    // Log account information and ensure funding
    console.log(`\nSuccessfully loaded account for role: ${account.role}`);
    console.log(`Address: ${account.address}`);
    console.log(`Balance: ${account.balance} NANOS\n`);

    return account;
  } catch (error) {
    console.log(`Error loading account: ${error}`);
  }
}
