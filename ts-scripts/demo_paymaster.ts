/**
 * Fundable Paymaster Demo
 * 
 * To run this script, you must first generate the identities using the Stellar CLI.
 * The script expects the following identities to exist:
 * - alice_demo_2 (Relayer)
 * - bob_demo_2 (User)
 * - usdc_issuer_2 (Issuer)
 * 
 * To generate and fund these accounts on testnet, run the following commands:
 * stellar keys generate alice_demo_2 --network testnet
 * stellar keys generate bob_demo_2 --network testnet
 * stellar keys generate usdc_issuer_2 --network testnet
 * 
 * Note: If you want Bob to start with exactly 0 available XLM to prove gas abstraction,
 * this script automatically drains his wallet to the minimum reserve balance before the transaction!
 */

import {
    Keypair,
    TransactionBuilder,
    Networks,
    Contract,
    nativeToScVal,
    scValToNative,
    rpc,
    authorizeEntry,
    Address,
    xdr,
    Operation,
    Asset
} from '@stellar/stellar-sdk';
import { execSync } from 'child_process';
import * as fs from 'fs';

const NETWORK_PASSPHRASE = Networks.TESTNET;
const RPC_URL = "https://soroban-testnet.stellar.org:443";
const server = new rpc.Server(RPC_URL);

function runCommand(cmd: string): string {
    return execSync(cmd, { encoding: 'utf8', stdio: 'pipe' }).trim();
}

async function main() {
    console.log("==========================================================");
    console.log("    FUNDABLE PAYMASTER DEMO: GAS ABSTRACTION (USDC)       ");
    console.log("==========================================================");

    // 1. Get Identities
    console.log("[1/5] Fetching Identities...");
    const aliceSecret = runCommand('stellar keys show alice_demo_2');
    const bobSecret = runCommand('stellar keys show bob_demo_2');
    const issuerAddress = runCommand('stellar keys address usdc_issuer_2');

    const alice = Keypair.fromSecret(aliceSecret);
    const bob = Keypair.fromSecret(bobSecret);

    console.log(`Relayer (Alice): ${alice.publicKey()}`);
    console.log(`User (Bob):      ${bob.publicKey()}`);
    console.log(`Issuer:          ${issuerAddress}`);

    console.log("\n[1.5/6] Draining Bob's XLM to prove Gas Abstraction...");
    const nativeId = runCommand(`stellar contract id asset --asset native --network testnet`);
    const bobXlmStr = runCommand(`stellar contract invoke --id ${nativeId} --source-account alice_demo_2 --network testnet -- balance --id ${bob.publicKey()}`);
    // Extract the number from string if it's quoted
    const currentXlmStroops = parseInt(bobXlmStr.replace(/"/g, ''), 10);
    const currentXlm = currentXlmStroops / 10000000;
    if (currentXlm > 3) {
        // Reserve is 1 XLM base + 0.5 XLM per subentry
        // Let's leave exactly 2.5 XLM (which is mathematically ~0 available XLM but prevents account deletion)
        const amountToSend = (currentXlm - 2.5).toFixed(7);
        console.log(`Sending ${amountToSend} XLM from Bob to Alice to empty his wallet...`);
        let bobAccountInfo = await server.getAccount(bob.publicKey());
        let drainTx = new TransactionBuilder(bobAccountInfo, {
            fee: "10000",
            networkPassphrase: NETWORK_PASSPHRASE,
        })
            .addOperation(Operation.payment({
                destination: alice.publicKey(),
                asset: Asset.native(),
                amount: amountToSend
            }))
            .setTimeout(30)
            .build();
        drainTx.sign(bob);
        const drainRes = await server.sendTransaction(drainTx);
        if (drainRes.status === "PENDING") {
            console.log(`Drain transaction sent! Hash: ${drainRes.hash}`);
            console.log("Waiting for network confirmation...");
            let drainStatus = await server.getTransaction(drainRes.hash);
            let retries = 0;
            while (drainStatus.status === "NOT_FOUND" && retries < 10) {
                await new Promise(resolve => setTimeout(resolve, 2000));
                drainStatus = await server.getTransaction(drainRes.hash);
                retries++;
            }
        }
        console.log("Bob's XLM drained! He now only has the minimum network reserve (0 available XLM).");
    } else {
        console.log("Bob's XLM is already at the minimum network reserve.");
    }

    // 2. Get Contracts
    console.log("\n[2/5] Reading Ecosystem Contracts...");
    const usdcId = runCommand(`stellar contract id asset --asset DemoUSDC:${issuerAddress} --network testnet`);
    const deployedContracts = JSON.parse(fs.readFileSync('../deployed_contracts.json', 'utf8'));
    const routerId = deployedContracts.router;

    const flowId = deployedContracts.flow;

    if (!routerId || !flowId) {
        throw new Error("Router or Flow ID not found in deployed_contracts.json");
    }

    console.log(`USDC ID:         ${usdcId}`);
    console.log(`Router ID:       ${routerId}`);
    console.log(`Flow ID:         ${flowId}`);

    // 3. Deploy & Init Paymaster
    console.log("\n[3/6] Deploying Fresh Paymaster...");
    const paymasterId = runCommand(`stellar contract deploy --wasm ../paymaster.wasm --source alice_demo_2 --network testnet`);
    console.log(`Paymaster ID:    ${paymasterId}`);

    runCommand(`stellar contract invoke --id ${paymasterId} --source alice_demo_2 --network testnet -- initialize --admin ${alice.publicKey()} --allowed_fee_tokens "[\\"${usdcId}\\"]"`);
    console.log("Paymaster initialized with USDC allowed!");

    // 4. Initial Balances
    console.log("\n[4/6] Checking Initial State...");
    const bobUsdcBefore = runCommand(`stellar contract invoke --id ${usdcId} --source-account alice_demo_2 --network testnet -- balance --id ${bob.publicKey()}`);
    const aliceUsdcBefore = runCommand(`stellar contract invoke --id ${usdcId} --source-account alice_demo_2 --network testnet -- balance --id ${alice.publicKey()}`);
    const bobXlmBefore = runCommand(`stellar contract invoke --id ${nativeId} --source-account alice_demo_2 --network testnet -- balance --id ${bob.publicKey()}`);
    const aliceXlmBefore = runCommand(`stellar contract invoke --id ${nativeId} --source-account alice_demo_2 --network testnet -- balance --id ${alice.publicKey()}`);

    console.log(`Bob's XLM Balance:    ${bobXlmBefore}`);
    console.log(`Bob's USDC Balance:   ${bobUsdcBefore}`);
    console.log(`Alice's XLM Balance:  ${aliceXlmBefore}`);
    console.log(`Alice's USDC Balance: ${aliceUsdcBefore}`);

    // 5. Build Multi-sig Tx via TS SDK
    console.log("\n[5/6] Building & Simulating Paymaster Transaction...");
    const account = await server.getAccount(alice.publicKey());
    const contract = new Contract(paymasterId);

    const routerArgs = [
        nativeToScVal(bob.publicKey(), { type: 'address' }),
        nativeToScVal(alice.publicKey(), { type: 'address' }),
        nativeToScVal(usdcId, { type: 'address' }),
        nativeToScVal(1000000n, { type: 'i128' }), // rate_per_second
        nativeToScVal(7, { type: 'u32' }),
        nativeToScVal(0n, { type: 'u64' })
    ];

    const invokeArgs = [
        nativeToScVal(bob.publicKey(), { type: 'address' }),
        nativeToScVal(usdcId, { type: 'address' }),
        nativeToScVal(50000n, { type: 'i128' }), // max_fee
        nativeToScVal(alice.publicKey(), { type: 'address' }),
        nativeToScVal(routerId, { type: 'address' }),
        nativeToScVal('create_flow_stream', { type: 'symbol' }),
        xdr.ScVal.scvVec(routerArgs)
    ];

    let tx = new TransactionBuilder(account, {
        fee: "10000",
        networkPassphrase: NETWORK_PASSPHRASE,
    })
        .addOperation(contract.call("collect_fee_and_invoke", ...invokeArgs))
        .setTimeout(30)
        .build();

    console.log("Simulating transaction...");
    const simResult = await server.simulateTransaction(tx);
    if (!simResult || rpc.Api.isSimulationError(simResult)) {
        console.error("Simulation failed!");
        console.error(simResult);
        return;
    }
    console.log("Simulation successful. Resource fee:", (simResult as any).minResourceFee || "ok");

    let assembledTx = rpc.assembleTransaction(tx, simResult).build() as any;

    let targetStreamId = "unknown";
    try {
        const parsedSim = simResult as any;
        if (parsedSim.result && parsedSim.result.retval) {
            targetStreamId = scValToNative(parsedSim.result.retval).toString();
        }
    } catch (e) {
        console.warn("Could not parse stream ID from simResult:", e);
    }

    console.log("Signing Auth Entries for Bob...");
    const latestLedger = await server.getLatestLedger();
    const validUntilLedgerSeq = latestLedger.sequence + 100;

    // Sign Bob's auth entries
    for (const op of assembledTx.operations) {
        if (op.type === "invokeHostFunction" && op.auth) {
            for (let i = 0; i < op.auth.length; i++) {
                const authEntry = op.auth[i];
                const creds = authEntry.credentials();
                // Check if it's an address credential and matches Bob
                if (creds.switch().name === 'sorobanCredentialsAddress') {
                    const address = Address.fromScAddress(creds.address().address()).toString();
                    if (address === bob.publicKey()) {
                        console.log("Found Bob's auth entry. Signing...");
                        op.auth[i] = await authorizeEntry(
                            authEntry,
                            bob,
                            validUntilLedgerSeq,
                            NETWORK_PASSPHRASE
                        );
                    }
                }
            }
        }
    }

    console.log("Signing Transaction Wrapper as Alice...");
    assembledTx.sign(alice);

    console.log("\n[6/6] Submitting Transaction...");
    const response = await server.sendTransaction(assembledTx);

    if (response.status === "PENDING") {
        console.log(`Transaction sent! Hash: ${response.hash}`);
        console.log("Waiting for network confirmation...");
        let txStatus = await server.getTransaction(response.hash);
        let retries = 0;
        while (txStatus.status === "NOT_FOUND" && retries < 10) {
            await new Promise(resolve => setTimeout(resolve, 2000));
            txStatus = await server.getTransaction(response.hash);
            retries++;
        }

        if (txStatus.status === "SUCCESS") {
            console.log("✅ Transaction successfully included in ledger!");
            console.log(`View on Explorer: https://stellar.expert/explorer/testnet/tx/${response.hash}`);

            console.log("\n================ RESULTS ================");
            const bobUsdcAfter = runCommand(`stellar contract invoke --id ${usdcId} --source-account alice_demo_2 --network testnet -- balance --id ${bob.publicKey()}`);
            const aliceUsdcAfter = runCommand(`stellar contract invoke --id ${usdcId} --source-account alice_demo_2 --network testnet -- balance --id ${alice.publicKey()}`);
            const bobXlmAfter = runCommand(`stellar contract invoke --id ${nativeId} --source-account alice_demo_2 --network testnet -- balance --id ${bob.publicKey()}`);
            const aliceXlmAfter = runCommand(`stellar contract invoke --id ${nativeId} --source-account alice_demo_2 --network testnet -- balance --id ${alice.publicKey()}`);

            console.log(`Bob's XLM Balance:    ${bobXlmAfter}`);
            console.log(`Bob's USDC Balance:   ${bobUsdcAfter}`);
            console.log(`Alice's XLM Balance:  ${aliceXlmAfter}`);
            console.log(`Alice's USDC Balance: ${aliceUsdcAfter}`);

            console.log(`\nFetching Stream #${targetStreamId} details from Flow Contract (${flowId})...`);
            try {
                const streamDetails = runCommand(`stellar contract invoke --id ${flowId} --source-account alice_demo_2 --network testnet -- get_stream --stream_id ${targetStreamId}`);
                console.log(streamDetails);
            } catch (err) {
                console.error("Failed to fetch stream details!");
            }
            console.log("=========================================");

        } else {
            console.error("❌ Transaction failed or not found:", txStatus);
        }
    } else {
        console.error("❌ Send failed:", response);
    }
}

main().catch(console.error);
