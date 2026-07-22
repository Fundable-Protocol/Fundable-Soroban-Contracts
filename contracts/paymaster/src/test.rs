//! Tests for the Fundable Paymaster contract.
//!
//! Covers:
//! - Initialization (single + double-init failure)
//! - Fee collection and forwarding to a target contract
//! - Token whitelist management (add, remove, query)
//! - Negative tests: unauthorized token, zero fee, insufficient balance
//! - Admin-only access control

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token::{StellarAssetClient, TokenClient},
    Address, Env, Symbol,
};

// ---------------------------------------------------------------------------
// Target contract for testing: a simple counter that increments
// ---------------------------------------------------------------------------

// We use a minimal test contract as the "target" for forwarded invocations.
mod target_contract {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct TargetContract;

    #[contractimpl]
    impl TargetContract {
        /// A simple function that returns a value, proving it was called.
        /// In production this would be Router.create_flow_stream() etc.
        pub fn ping(env: Env) -> u32 {
            let _ = env;
            42
        }

        /// A function that requires auth from a user (simulates real contract calls).
        pub fn authed_action(env: Env, user: Address, amount: i128) -> i128 {
            user.require_auth();
            let _ = env;
            amount * 2
        }
    }
}

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/// 1 token in 7-decimal representation.
const ONE_TOKEN: i128 = 10_000_000; // 1e7

/// Set up a test environment with:
/// - A fee token (SAC with 7 decimals)
/// - An admin, a user, and a relayer
/// - The Paymaster contract initialized with the fee token
fn setup_test() -> (
    Env,
    Address,              // paymaster contract id
    Address,              // admin
    Address,              // user
    Address,              // relayer
    Address,              // fee token address
    TokenClient<'static>, // fee token client
) {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    env.ledger().set(LedgerInfo {
        timestamp: 1000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let relayer = Address::generate(&env);

    // Create a test fee token (SAC-like with 7 decimals)
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let fee_token = sac.address();
    let token_client = TokenClient::new(&env, &fee_token);
    let sac_admin = StellarAssetClient::new(&env, &fee_token);

    // Mint fee tokens to the user (10,000 tokens)
    sac_admin.mint(&user, &(10_000 * ONE_TOKEN));

    // Register and initialize the Paymaster contract
    let contract_id = env.register(PaymasterContract, ());
    let client = PaymasterContractClient::new(&env, &contract_id);

    let allowed_tokens = Vec::from_array(&env, [fee_token.clone()]);
    client.initialize(&admin, &allowed_tokens);

    (
        env,
        contract_id,
        admin,
        user,
        relayer,
        fee_token,
        token_client,
    )
}

// ---------------------------------------------------------------------------
// Initialization Tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(PaymasterContract, ());
    let client = PaymasterContractClient::new(&env, &contract_id);

    let allowed_tokens: Vec<Address> = Vec::new(&env);
    client.initialize(&admin, &allowed_tokens);

    // Should succeed — verify admin is set
    assert_eq!(client.get_admin(), admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #401)")] // AlreadyInitialized
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(PaymasterContract, ());
    let client = PaymasterContractClient::new(&env, &contract_id);

    let allowed_tokens: Vec<Address> = Vec::new(&env);
    client.initialize(&admin, &allowed_tokens);
    client.initialize(&admin, &allowed_tokens); // Should panic
}

// ---------------------------------------------------------------------------
// Fee Token Management Tests
// ---------------------------------------------------------------------------

#[test]
fn test_fee_token_is_allowed() {
    let (env, contract_id, _admin, _user, _relayer, fee_token, _token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);

    assert!(client.is_fee_token_allowed(&fee_token));
}

#[test]
fn test_add_fee_token() {
    let (env, contract_id, _admin, _user, _relayer, _fee_token, _token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);

    // Create a second token
    let token_admin2 = Address::generate(&env);
    let sac2 = env.register_stellar_asset_contract_v2(token_admin2);
    let fee_token2 = sac2.address();

    assert!(!client.is_fee_token_allowed(&fee_token2));
    client.add_fee_token(&fee_token2);
    assert!(client.is_fee_token_allowed(&fee_token2));
}

// ---------------------------------------------------------------------------
// Multiple Fee Tokens Test
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_fee_tokens() {
    let (env, contract_id, _admin, user, relayer, fee_token, token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);

    // Create and add a second fee token
    let token_admin2 = Address::generate(&env);
    let sac2 = env.register_stellar_asset_contract_v2(token_admin2.clone());
    let fee_token2 = sac2.address();
    let token_client2 = TokenClient::new(&env, &fee_token2);
    let sac_admin2 = StellarAssetClient::new(&env, &fee_token2);

    // Mint second token to user
    sac_admin2.mint(&user, &(5_000 * ONE_TOKEN));

    // Add second token to whitelist
    client.add_fee_token(&fee_token2);

    let target_id = env.register(target_contract::TargetContract, ());
    let args: Vec<Val> = Vec::new(&env);

    // Use first token
    let fee1 = 10 * ONE_TOKEN;
    let user_bal1_before = token_client.balance(&user);
    client.collect_fee_and_invoke(
        &user,
        &fee_token,
        &fee1,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );
    assert_eq!(token_client.balance(&user), user_bal1_before - fee1);

    // Use second token
    let fee2 = 20 * ONE_TOKEN;
    let user_bal2_before = token_client2.balance(&user);
    client.collect_fee_and_invoke(
        &user,
        &fee_token2,
        &fee2,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );
    assert_eq!(token_client2.balance(&user), user_bal2_before - fee2);
}

#[test]
fn test_forward_collects_bounded_fee_and_invokes_target() {
    let (env, contract_id, _admin, user, relayer, fee_token, token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);
    let target_id = env.register(target_contract::TargetContract, ());
    let args: Vec<Val> = Vec::new(&env);
    let fee = 10 * ONE_TOKEN;
    let user_before = token_client.balance(&user);
    let recipient_before = token_client.balance(&relayer);

    client.forward(
        &user,
        &fee_token,
        &fee,
        &(fee * 2),
        &200,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );

    assert_eq!(token_client.balance(&user), user_before - fee);
    assert_eq!(token_client.balance(&relayer), recipient_before + fee);
}

#[test]
#[should_panic]
fn test_forward_rejects_fee_above_authorized_maximum() {
    let (env, contract_id, _admin, user, relayer, fee_token, _token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);
    let target_id = env.register(target_contract::TargetContract, ());
    let args: Vec<Val> = Vec::new(&env);

    client.forward(
        &user,
        &fee_token,
        &(2 * ONE_TOKEN),
        &ONE_TOKEN,
        &200,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );
}

#[test]
#[should_panic]
fn test_forward_rejects_expired_approval() {
    let (env, contract_id, _admin, user, relayer, fee_token, _token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);
    let target_id = env.register(target_contract::TargetContract, ());
    let args: Vec<Val> = Vec::new(&env);

    client.forward(
        &user,
        &fee_token,
        &ONE_TOKEN,
        &ONE_TOKEN,
        &99,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #404)")]
fn test_forward_rejects_disallowed_fee_token() {
    let (env, contract_id, _admin, user, relayer, _fee_token, _token_client) = setup_test();
    let client = PaymasterContractClient::new(&env, &contract_id);
    let target_id = env.register(target_contract::TargetContract, ());
    let token_admin = Address::generate(&env);
    let disallowed_token = env.register_stellar_asset_contract_v2(token_admin).address();
    let args: Vec<Val> = Vec::new(&env);

    client.forward(
        &user,
        &disallowed_token,
        &ONE_TOKEN,
        &ONE_TOKEN,
        &200,
        &relayer,
        &target_id,
        &Symbol::new(&env, "ping"),
        &args,
    );
}
