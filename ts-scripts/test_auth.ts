import { Keypair, authorizeEntry, Contract, nativeToScVal } from '@stellar/stellar-sdk';

const bob = Keypair.random();
console.log(typeof authorizeEntry);
