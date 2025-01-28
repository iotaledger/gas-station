import * as dotenv from 'dotenv';
import { IotaClient, TransactionEffects } from '@iota/iota-sdk/client';
import { ObjectRef, Transaction } from '@iota/iota-sdk/transactions';
import { toB64 } from '@iota/bcs';
import axios from 'axios';
import { loadAccountFromKey } from './loadAccount';

// Define interface for the gas reservation result returned by the gas station.
interface ReserveGasResult {
  sponsor_address: string;     // The sponsor’s on-chain address.
  reservation_id: number;      // An ID used to reference this particular gas reservation.
  gas_coins: ObjectRef[];      // References to the sponsor’s coins that will pay gas.
}

// Load environment variables from .env file
dotenv.config();

// Read environment variables for the node and gas station endpoints
const nodeUrl = process.env.NODE as string;
const explorerUrl = process.env.EXPLORER as string;
const gasStationUrl = process.env.GAS_STATION as string;
const gasStationToken = process.env.GAS_STATION_AUTH as string;

/**
 * Main entry point for the script.
 * 1. Loads a funded sender account.
 * 2. Constructs a transaction that calls a Move function to create an object with a message.
 * 3. Reserves gas from a sponsor and sets up the transaction to use it.
 * 4. Signs the transaction locally.
 * 5. Sends the transaction to the gas station to co-sign and submit it on-chain.
 */
async function main() {
  try {
    // Create a new IotaClient that points to the specified IOTA node URL
    const client = new IotaClient({ url: nodeUrl });

    // Load an account (keypair + address) from an environment variable
    // 'sender' must be a label recognized by loadAccountFromKey
    const sender = await loadAccountFromKey('user', process.env.KEY1 as string);
    if (!sender) {
      throw new Error('Sender account or address is undefined.');
    }

    // Contract details for calling the system time
    const packageId = '0x2';
    const moduleName = 'clock';
    const functionName = 'timestamp_ms';
    const clockObjectId = '0x6';

    // Create a new transaction builder
    const tx = new Transaction();

    // Add the Move function call to the transaction
    tx.moveCall({
      target: `${packageId}::${moduleName}::${functionName}`,
      arguments: [tx.object(clockObjectId)],
    });

    // Reserve gas from the gas station so the sender doesn't have to pay gas themselves
    const gasBudget = 50_000_000;
    const reservedSponsorGasData = await getSponsorGas(gasBudget);
    console.log("Reserved Gas Object in Gas Station");
    console.log(reservedSponsorGasData,"\n");

    // Set the sender and sponsor details on the transaction
    // - sender is the original user who calls the function
    // - gasOwner is the sponsor who actually covers the cost
    // - gasPayment is the sponsor's coins
    // - gasBudget is how much gas can be spent
    tx.setSender(sender.address);
    tx.setGasOwner(reservedSponsorGasData.sponsor_address);
    tx.setGasPayment(reservedSponsorGasData.gas_coins);
    tx.setGasBudget(gasBudget);

    // Build (serialize) the unsigned transaction bytes to be signed by the sender
    const unsignedTxBytes = await tx.build({ client });

    // The sender locally signs the transaction bytes
    // This ensures the user's intent is authenticated before sending to the sponsor
    const signedTx = await sender.keypair.signTransaction(unsignedTxBytes);
    console.log("Tx Bytes signed by Sender:")
    console.log(signedTx,"\n");
    const senderSignature = signedTx.signature;

    // Submit the transaction + sender's signature to the gas station,
    // which will add its own signature and broadcast the transaction to the network
    const transactionEffects = await sponsorSignAndSubmit(reservedSponsorGasData.reservation_id, unsignedTxBytes, senderSignature);
    console.log("Issue Transaction:");
    console.log(`${explorerUrl}/tx/${transactionEffects.transactionDigest}\n`);

  } catch (error) {
    console.error('Error preparing unsigned transaction:', error);
  }
}

/**
 * Requests gas from the gas station, reserving a certain gas budget for a set duration.
 *
 * @param gasBudget - The maximum gas units we want to allocate for this transaction
 * @returns An object containing sponsor address, reservation ID, and sponsor coin references
 */
async function getSponsorGas(gasBudget: number): Promise<ReserveGasResult> {
  // Configure the Axios instance with the bearer token required by the gas station
  axios.defaults.headers.common = { 'Authorization': `Bearer ${gasStationToken}` }

  // Prepare the reservation request
  const requestData = {
    gas_budget: gasBudget,
    reserve_duration_secs: 10,
  };

  // Call the gas station endpoint to reserve gas
  const reservation_response = await axios.post(gasStationUrl + '/v1/reserve_gas', requestData);

  // Return the result containing sponsor address, ID, and coin references
  return reservation_response.data.result;
}

/**
 * Sends the user-signed transaction bytes plus the signature to the gas station for co-signing
 * and submission to the network.
 *
 * @param reservationId - ID from the previously reserved gas
 * @param transaction - The unsigned transaction bytes that the sponsor will co-sign
 * @param senderSignature - The signature from the user (sender)
 * @returns The on-chain TransactionEffects, reflecting success/failure and final state changes
 */
async function sponsorSignAndSubmit(
  reservationId: number,
  transaction: Uint8Array,
  senderSignature: string
): Promise<TransactionEffects> {
  // Encode the transaction bytes to Base64, to pass along with the sender's signature
  const data = {
    reservation_id: reservationId,
    tx_bytes: toB64(transaction),
    user_sig: senderSignature
  };

  // The gas station signs the transaction with its own keys, then submits it on-chain
  const response = await axios.post(gasStationUrl + '/v1/execute_tx', data);

  // Return the resulting transaction effects (including object changes, event logs, etc.)
  return response.data.effects;
}

// Run the main function
main();
