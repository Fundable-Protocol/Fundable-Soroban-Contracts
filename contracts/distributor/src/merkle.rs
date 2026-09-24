//! Merkle proof verification for the Distributor contract.
//!
//! Leaves are constructed as `keccak256(claimant_xdr || amount_be_bytes)`.
//! The proof is verified by iterating up the tree, hashing pairs in
//! sorted order (smaller hash first) to produce a deterministic root.
//!
//! This approach matches the standard OpenZeppelin Merkle tree pattern.

use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{Address, BytesN, Env, Vec};

/// Compute the leaf hash for a claimant and amount.
///
/// Leaf = keccak256(claimant_xdr || amount_be_bytes)
///
/// The claimant address is XDR-serialized and concatenated with the
/// big-endian i128 amount bytes to form the preimage.
pub fn compute_leaf(env: &Env, claimant: &Address, amount: i128) -> BytesN<32> {
    let mut preimage = soroban_sdk::Bytes::new(env);

    // Serialize the claimant address to XDR bytes via the Soroban SDK
    let claimant_bytes = claimant.clone().to_xdr(env);
    preimage.append(&claimant_bytes);

    // Serialize the amount as big-endian 16-byte i128
    let amount_bytes = soroban_sdk::Bytes::from_slice(env, &amount.to_be_bytes());
    preimage.append(&amount_bytes);

    env.crypto().keccak256(&preimage).into()
}

/// Verify a Merkle proof against an expected root.
///
/// The proof consists of sibling hashes from leaf to root. At each level,
/// the current hash and the proof element are sorted (smaller first) before
/// hashing, ensuring the tree is order-independent.
pub fn verify_proof(
    env: &Env,
    proof: &Vec<BytesN<32>>,
    root: &BytesN<32>,
    leaf: &BytesN<32>,
) -> bool {
    let mut computed_hash = leaf.clone();

    for sibling in proof.iter() {
        computed_hash = hash_pair(env, &computed_hash, &sibling);
    }

    computed_hash == *root
}

/// Hash two 32-byte values together in sorted order.
///
/// Sorting ensures the same root regardless of left/right ordering,
/// which simplifies off-chain tree construction.
fn hash_pair(env: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let mut combined = soroban_sdk::Bytes::new(env);

    // Sort: smaller hash first for deterministic ordering
    if a <= b {
        combined.append(&soroban_sdk::Bytes::from_slice(
            env,
            a.to_array().as_slice(),
        ));
        combined.append(&soroban_sdk::Bytes::from_slice(
            env,
            b.to_array().as_slice(),
        ));
    } else {
        combined.append(&soroban_sdk::Bytes::from_slice(
            env,
            b.to_array().as_slice(),
        ));
        combined.append(&soroban_sdk::Bytes::from_slice(
            env,
            a.to_array().as_slice(),
        ));
    }

    env.crypto().keccak256(&combined).into()
}
