#![cfg(test)]
extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, BytesN, Env, Vec,
};

use crate::merkle;
use crate::{DistributorContract, DistributorContractClient};

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/// Register the distributor contract and return a client.
fn setup_contract(env: &Env) -> (DistributorContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(DistributorContract, ());
    let client = DistributorContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let fee_address = Address::generate(env);
    let token_admin = Address::generate(env);

    (client, admin, fee_address, token_admin)
}

/// Create a test token and mint to the given address.
fn create_token(env: &Env, admin: &Address, mint_to: &Address, amount: i128) -> Address {
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token_admin_client = token::StellarAssetClient::new(env, &token_contract.address());
    token_admin_client.mint(mint_to, &amount);
    token_contract.address()
}

/// Build a simple 2-leaf Merkle tree and return (root, proofs).
///
/// Leaves: [leaf_a, leaf_b]
/// Tree:     root
///          /    \
///       leaf_a  leaf_b
///
/// Proof for leaf_a = [leaf_b]
/// Proof for leaf_b = [leaf_a]
fn build_two_leaf_tree(
    env: &Env,
    addr_a: &Address,
    amount_a: i128,
    addr_b: &Address,
    amount_b: i128,
) -> (BytesN<32>, Vec<BytesN<32>>, Vec<BytesN<32>>) {
    let leaf_a = merkle::compute_leaf(env, addr_a, amount_a);
    let leaf_b = merkle::compute_leaf(env, addr_b, amount_b);

    // Root = hash_pair(leaf_a, leaf_b) — sorted internally
    let root = compute_root_from_two(env, &leaf_a, &leaf_b);

    let mut proof_a = Vec::new(env);
    proof_a.push_back(leaf_b.clone());

    let mut proof_b = Vec::new(env);
    proof_b.push_back(leaf_a.clone());

    (root, proof_a, proof_b)
}

/// Compute the root from two leaves (mirrors merkle::hash_pair).
fn compute_root_from_two(env: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let mut combined = soroban_sdk::Bytes::new(env);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, _) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    assert_eq!(client.get_protocol_fee_percent(), 0);
    assert_eq!(client.get_protocol_fee_address(), fee_address);
}

#[test]
#[should_panic(expected = "Error(Contract, #401)")]
fn test_initialize_twice_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, _) = setup_contract(&env);
    client.initialize(&admin, &fee_address);
    client.initialize(&admin, &fee_address); // should panic
}

#[test]
fn test_create_distribution_and_claim() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);
    let amount_a: i128 = 1_000_000;
    let amount_b: i128 = 2_000_000;
    let total = amount_a + amount_b;

    // Create token and mint to admin
    let token_addr = create_token(&env, &token_admin, &admin, total);

    // Build Merkle tree
    let (root, proof_a, proof_b) =
        build_two_leaf_tree(&env, &claimant_a, amount_a, &claimant_b, amount_b);

    // Create distribution
    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"test-dist-1");
    let dist_id = client.create_distribution(&admin, &token_addr, &root, &total, &0, &unique_ref);

    assert_eq!(dist_id, 1);

    // Verify distribution record
    let record = client.get_distribution(&dist_id);
    assert_eq!(record.total_amount, total);
    assert_eq!(record.claimed_amount, 0);
    assert!(!record.is_cancelled);

    // Claim as claimant_a
    client.claim(&claimant_a, &dist_id, &amount_a, &proof_a);
    assert!(client.has_claimed(&dist_id, &claimant_a));
    assert!(!client.has_claimed(&dist_id, &claimant_b));

    // Verify token balance
    let token_client = token::Client::new(&env, &token_addr);
    assert_eq!(token_client.balance(&claimant_a), amount_a);

    // Claim as claimant_b
    client.claim(&claimant_b, &dist_id, &amount_b, &proof_b);
    assert!(client.has_claimed(&dist_id, &claimant_b));
    assert_eq!(token_client.balance(&claimant_b), amount_b);

    // Verify distribution updated
    let record = client.get_distribution(&dist_id);
    assert_eq!(record.claimed_amount, total);
}

#[test]
fn test_claim_with_protocol_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    // Set 2.5% fee (250 basis points)
    client.set_protocol_fee_percent(&250);
    assert_eq!(client.get_protocol_fee_percent(), 250);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;

    let token_addr = create_token(&env, &token_admin, &admin, amount);

    // Build a single-leaf tree (proof is empty)
    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone(); // single leaf = root
    let proof: Vec<BytesN<32>> = Vec::new(&env);

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"fee-test");
    client.create_distribution(&admin, &token_addr, &root, &amount, &0, &unique_ref);

    client.claim(&claimant, &1, &amount, &proof);

    let token_client = token::Client::new(&env, &token_addr);
    // Expected fee: 1_000_000 * 250 / 10000 = 25_000
    let expected_fee: i128 = 25_000;
    let expected_claim = amount - expected_fee;

    assert_eq!(token_client.balance(&claimant), expected_claim);
    assert_eq!(token_client.balance(&fee_address), expected_fee);
}

#[test]
#[should_panic(expected = "Error(Contract, #406)")]
fn test_double_claim_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;
    let token_addr = create_token(&env, &token_admin, &admin, amount);

    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone();
    let proof: Vec<BytesN<32>> = Vec::new(&env);

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"double-claim");
    client.create_distribution(&admin, &token_addr, &root, &amount, &0, &unique_ref);

    client.claim(&claimant, &1, &amount, &proof);
    client.claim(&claimant, &1, &amount, &proof); // should panic
}

#[test]
#[should_panic(expected = "Error(Contract, #405)")]
fn test_invalid_proof_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;
    let token_addr = create_token(&env, &token_admin, &admin, amount);

    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone();

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"bad-proof");
    client.create_distribution(&admin, &token_addr, &root, &amount, &0, &unique_ref);

    // Try to claim with wrong amount
    let wrong_amount = 999_999i128;
    let proof: Vec<BytesN<32>> = Vec::new(&env);
    client.claim(&claimant, &1, &wrong_amount, &proof); // should panic
}

#[test]
fn test_cancel_distribution() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;
    let token_addr = create_token(&env, &token_admin, &admin, amount);

    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone();

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"cancel-test");
    let dist_id = client.create_distribution(&admin, &token_addr, &root, &amount, &0, &unique_ref);

    let token_client = token::Client::new(&env, &token_addr);
    // Admin balance should be 0 after depositing
    assert_eq!(token_client.balance(&admin), 0);

    // Cancel the distribution
    client.cancel_distribution(&dist_id);

    // Admin should get tokens back
    assert_eq!(token_client.balance(&admin), amount);

    // Distribution should be marked as cancelled
    let record = client.get_distribution(&dist_id);
    assert!(record.is_cancelled);
}

#[test]
#[should_panic(expected = "Error(Contract, #407)")]
fn test_claim_cancelled_distribution_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;
    let token_addr = create_token(&env, &token_admin, &admin, amount);

    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone();
    let proof: Vec<BytesN<32>> = Vec::new(&env);

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"cancelled");
    client.create_distribution(&admin, &token_addr, &root, &amount, &0, &unique_ref);
    client.cancel_distribution(&1);

    client.claim(&claimant, &1, &amount, &proof); // should panic
}

#[test]
#[should_panic(expected = "Error(Contract, #408)")]
fn test_claim_expired_distribution_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant = Address::generate(&env);
    let amount: i128 = 1_000_000;
    let token_addr = create_token(&env, &token_admin, &admin, amount);

    let leaf = merkle::compute_leaf(&env, &claimant, amount);
    let root = leaf.clone();
    let proof: Vec<BytesN<32>> = Vec::new(&env);

    // Set deadline to 1000
    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"expired");
    client.create_distribution(&admin, &token_addr, &root, &amount, &1000, &unique_ref);

    // Advance time past deadline
    env.ledger().with_mut(|li| {
        li.timestamp = 1001;
    });

    client.claim(&claimant, &1, &amount, &proof); // should panic
}

#[test]
fn test_admin_transfer() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, _) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let new_admin = Address::generate(&env);

    // Two-step transfer
    client.propose_admin(&new_admin);
    client.accept_admin();

    // Verify new admin can set fees
    client.set_protocol_fee_percent(&100);
    assert_eq!(client.get_protocol_fee_percent(), 100);
}

#[test]
#[should_panic(expected = "Error(Contract, #410)")]
fn test_invalid_fee_percent_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, _) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    // 10001 > 10000, should panic
    client.set_protocol_fee_percent(&10001);
}

#[test]
fn test_partial_claim_then_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);
    let amount_a: i128 = 1_000_000;
    let amount_b: i128 = 2_000_000;
    let total = amount_a + amount_b;

    let token_addr = create_token(&env, &token_admin, &admin, total);

    let (root, proof_a, _proof_b) =
        build_two_leaf_tree(&env, &claimant_a, amount_a, &claimant_b, amount_b);

    let unique_ref = soroban_sdk::Bytes::from_slice(&env, b"partial");
    let dist_id = client.create_distribution(&admin, &token_addr, &root, &total, &0, &unique_ref);

    // Only claimant_a claims
    client.claim(&claimant_a, &dist_id, &amount_a, &proof_a);

    // Admin cancels — should get back claimant_b's unclaimed portion
    client.cancel_distribution(&dist_id);

    let token_client = token::Client::new(&env, &token_addr);
    assert_eq!(token_client.balance(&claimant_a), amount_a);
    assert_eq!(token_client.balance(&admin), amount_b); // reclaimed
}

#[test]
fn test_oversized_distribution_cannot_drain_another_distribution() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, fee_address, token_admin) = setup_contract(&env);
    client.initialize(&admin, &fee_address);

    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);
    let deposit_per_distribution: i128 = 100;
    let oversized_allocation: i128 = 200;
    let token_addr = create_token(&env, &token_admin, &admin, deposit_per_distribution * 2);

    let root_a = merkle::compute_leaf(&env, &claimant_a, oversized_allocation);
    let root_b = merkle::compute_leaf(&env, &claimant_b, deposit_per_distribution);
    let empty_proof: Vec<BytesN<32>> = Vec::new(&env);

    let ref_a = soroban_sdk::Bytes::from_slice(&env, b"oversized");
    let ref_b = soroban_sdk::Bytes::from_slice(&env, b"protected");
    let dist_a = client.create_distribution(
        &admin,
        &token_addr,
        &root_a,
        &deposit_per_distribution,
        &0,
        &ref_a,
    );
    let dist_b = client.create_distribution(
        &admin,
        &token_addr,
        &root_b,
        &deposit_per_distribution,
        &0,
        &ref_b,
    );

    assert!(client
        .try_claim(&claimant_a, &dist_a, &oversized_allocation, &empty_proof,)
        .is_err());
    assert_eq!(client.get_distribution(&dist_a).claimed_amount, 0);

    client.claim(
        &claimant_b,
        &dist_b,
        &deposit_per_distribution,
        &empty_proof,
    );

    let token_client = token::Client::new(&env, &token_addr);
    assert_eq!(token_client.balance(&claimant_a), 0);
    assert_eq!(token_client.balance(&claimant_b), deposit_per_distribution);
    assert_eq!(
        token_client.balance(&client.address),
        deposit_per_distribution
    );
}

#[test]
fn test_deterministic_leaf_vector() {
    let env = Env::default();
    let addr = Address::from_string(&soroban_sdk::String::from_str(
        &env,
        "GDZJSPRSBTAJPAQ4NG6Y2ZCWHEX5HMS253TYVNAQJRPJHY27JPOHBIPZ",
    ));
    let amount: i128 = 10_000_000;
    let leaf = merkle::compute_leaf(&env, &addr, amount);
    assert_eq!(
        leaf.to_array(),
        [
            0x1c, 0x1c, 0xc7, 0x14, 0x0b, 0x08, 0xa1, 0xb2, 0x23, 0x87, 0xc6, 0x8d, 0x05, 0xba,
            0xd1, 0xb6, 0x18, 0x5f, 0x6e, 0xd7, 0x1c, 0x30, 0x0a, 0x3e, 0xde, 0x78, 0xab, 0x70,
            0x72, 0x56, 0xa8, 0xb5,
        ]
    );
}
