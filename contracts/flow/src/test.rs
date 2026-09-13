//! Tests for the Fundable Flow contract.
//!
//! Covers:
//! - Full lifecycle: create → deposit → withdraw → refund
//! - Pause/restart cycle
//! - Rate adjustment with debt snapshot
//! - Void stream (solvent + insolvent)
//! - Authorization failure tests
//! - Edge cases: zero amounts, boundary values

#![cfg(test)]

extern crate std;

use super::*;
use proptest::prelude::*;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error,
    testutils::{
        Address as _, AuthorizedFunction, AuthorizedInvocation, EnvTestConfig, Ledger, LedgerInfo,
    },
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

#[contracttype]
#[derive(Clone)]
enum AdversarialTokenKey {
    Balance(Address),
    Mode,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
enum AdversarialTokenError {
    Rejected = 1,
    InsufficientBalance = 2,
}

#[contract]
struct AdversarialToken;

#[contractimpl]
impl AdversarialToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        env.storage()
            .persistent()
            .set(&AdversarialTokenKey::Balance(to), &amount);
    }

    pub fn set_mode(env: Env, mode: u32) {
        env.storage()
            .instance()
            .set(&AdversarialTokenKey::Mode, &mode);
    }

    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&AdversarialTokenKey::Balance(account))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let mode: u32 = env
            .storage()
            .instance()
            .get(&AdversarialTokenKey::Mode)
            .unwrap_or(0);
        if mode == 1 {
            panic_with_error!(&env, AdversarialTokenError::Rejected);
        }
        if mode == 2 {
            return;
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if amount < 0 || from_balance < amount {
            panic_with_error!(&env, AdversarialTokenError::InsufficientBalance);
        }
        let to_balance = Self::balance(env.clone(), to.clone());
        let credited = if mode == 3 { amount - 1 } else { amount };
        env.storage().persistent().set(
            &AdversarialTokenKey::Balance(from),
            &(from_balance - amount),
        );
        env.storage()
            .persistent()
            .set(&AdversarialTokenKey::Balance(to), &(to_balance + credited));
    }
}

mod current_flow_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/flow.wasm");
}

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/// Standard token decimals for tests (7, like most Stellar assets).
const TOKEN_DECIMALS: u32 = 7;

/// 1 token in 7-decimal representation.
const ONE_TOKEN: i128 = 10_000_000; // 1e7

/// Rate: 1 token per second in 18-decimal fixed-point.
/// For a 7-decimal token: 1e18 per second.
const RATE_1_PER_SEC: i128 = 1_000_000_000_000_000_000; // 1e18

/// Set up a test environment with a token contract and funded accounts.
fn setup_test() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    TokenClient<'static>,
) {
    setup_test_with_snapshots(true)
}

fn setup_test_with_snapshots(
    capture_snapshot_at_drop: bool,
) -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    TokenClient<'static>,
) {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop,
    });
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    // Set up ledger with a known timestamp
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
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Create a test token (SAC-like with 7 decimals)
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token = sac.address();
    let token_client = TokenClient::new(&env, &token);
    let sac_admin = StellarAssetClient::new(&env, &token);

    // Mint tokens to the sender (1,000,000 tokens)
    sac_admin.mint(&sender, &(1_000_000 * ONE_TOKEN));

    // Register the Flow contract with atomic constructor arguments.
    let contract_id = env.register(FlowContract, FlowContractArgs::__constructor(&admin));

    (env, contract_id, sender, recipient, token, token_client)
}

/// Create a helper to get a client from env + contract_id
fn get_client<'a>(env: &Env, contract_id: &Address) -> FlowContractClient<'a> {
    FlowContractClient::new(env, contract_id)
}

fn invocation(
    env: &Env,
    contract: &Address,
    function: &str,
    args: Vec<Val>,
    sub_invocations: std::vec::Vec<AuthorizedInvocation>,
) -> AuthorizedInvocation {
    AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            contract.clone(),
            Symbol::new(env, function),
            args,
        )),
        sub_invocations,
    }
}

fn assert_exact_auth(
    env: &Env,
    signer: &Address,
    contract: &Address,
    function: &str,
    args: Vec<Val>,
    sub_invocations: std::vec::Vec<AuthorizedInvocation>,
) {
    assert_eq!(
        env.auths(),
        std::vec![(
            signer.clone(),
            invocation(env, contract, function, args, sub_invocations),
        )]
    );
}

#[test]
fn test_exact_authorization_trees_for_sensitive_flow_calls() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let amount = 100 * ONE_TOKEN;

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &1_000,
        &amount,
    );
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "create_and_deposit",
        (
            sender.clone(),
            recipient.clone(),
            token.clone(),
            RATE_1_PER_SEC,
            TOKEN_DECIMALS,
            1_000_u64,
            amount,
        )
            .into_val(&env),
        std::vec![invocation(
            &env,
            &token,
            "transfer",
            (sender.clone(), contract_id.clone(), amount).into_val(&env),
            std::vec![],
        )],
    );

    let top_up = 10 * ONE_TOKEN;
    client.deposit(&stream_id, &sender, &top_up);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "deposit",
        (stream_id, sender.clone(), top_up).into_val(&env),
        std::vec![invocation(
            &env,
            &token,
            "transfer",
            (sender.clone(), contract_id.clone(), top_up).into_val(&env),
            std::vec![],
        )],
    );

    client.pause(&stream_id, &sender);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "pause",
        (stream_id, sender.clone()).into_val(&env),
        std::vec![],
    );
    client.restart(&stream_id, &sender, &(RATE_1_PER_SEC * 2));
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "restart",
        (stream_id, sender.clone(), RATE_1_PER_SEC * 2).into_val(&env),
        std::vec![],
    );
    client.adjust_rate(&stream_id, &sender, &(RATE_1_PER_SEC * 3));
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "adjust_rate",
        (stream_id, sender.clone(), RATE_1_PER_SEC * 3).into_val(&env),
        std::vec![],
    );

    env.ledger().set_timestamp(1_010);
    client.withdraw(&stream_id, &recipient, &recipient, &ONE_TOKEN);
    assert_exact_auth(
        &env,
        &recipient,
        &contract_id,
        "withdraw",
        (stream_id, recipient.clone(), recipient.clone(), ONE_TOKEN).into_val(&env),
        std::vec![],
    );
    client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_exact_auth(
        &env,
        &recipient,
        &contract_id,
        "withdraw_max",
        (stream_id, recipient.clone(), recipient.clone()).into_val(&env),
        std::vec![],
    );

    client.refund(&stream_id, &sender, &ONE_TOKEN);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "refund",
        (stream_id, sender.clone(), ONE_TOKEN).into_val(&env),
        std::vec![],
    );
    client.refund_max(&stream_id, &sender);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "refund_max",
        (stream_id, sender.clone()).into_val(&env),
        std::vec![],
    );
    client.void_stream(&stream_id, &sender);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "void_stream",
        (stream_id, sender.clone()).into_val(&env),
        std::vec![],
    );

    let standalone_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &1_010,
    );
    assert_eq!(standalone_id, 2);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "create",
        (
            sender.clone(),
            recipient,
            token,
            RATE_1_PER_SEC,
            TOKEN_DECIMALS,
            1_010_u64,
        )
            .into_val(&env),
        std::vec![],
    );
}

#[test]
fn test_exact_authorization_tree_for_flow_admin_rotation() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let contract_id = env.register(FlowContract, FlowContractArgs::__constructor(&admin));
    let client = FlowContractClient::new(&env, &contract_id);

    let wasm_hash = env.deployer().upload_contract_wasm(current_flow_wasm::WASM);
    client.upgrade(&wasm_hash);
    assert_exact_auth(
        &env,
        &admin,
        &contract_id,
        "upgrade",
        (wasm_hash,).into_val(&env),
        std::vec![],
    );

    client.set_admin(&new_admin);
    assert_exact_auth(
        &env,
        &admin,
        &contract_id,
        "set_admin",
        (new_admin,).into_val(&env),
        std::vec![],
    );
}

fn assert_flow_accounting_invariants(
    client: &FlowContractClient<'_>,
    token_client: &TokenClient<'_>,
    contract_id: &Address,
    stream_id: u64,
) {
    let stream = client.get_stream(&stream_id);
    let total_debt = client.total_debt_of(&stream_id);
    let covered = client.covered_debt_of(&stream_id);
    let uncovered = client.uncovered_debt_of(&stream_id);
    let refundable = client.refundable_amount_of(&stream_id);

    assert!(stream.balance >= 0);
    assert!(total_debt >= 0);
    assert!(covered >= 0 && covered <= stream.balance);
    assert!(uncovered >= 0);
    assert!(refundable >= 0);
    assert_eq!(total_debt, covered + uncovered);
    assert_eq!(stream.balance, covered + refundable);
    assert_eq!(token_client.balance(contract_id), stream.balance);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn prop_flow_conserves_assets_and_partitions_debt(
        deposit in 1_i128..=1_000_000_000_000_i128,
        native_rate in 1_i128..=1_000_000_i128,
        elapsed in 0_u64..=100_000_u64,
        withdraw_percent in 0_i128..=100_i128,
        refund_percent in 0_i128..=100_i128,
    ) {
        let (env, contract_id, sender, recipient, token, token_client) =
            setup_test_with_snapshots(false);
        let client = get_client(&env, &contract_id);
        let initial_supply = token_client.balance(&sender);
        let rate_scaled = native_rate * 100_000_000_000_i128;
        let stream_id = client.create_and_deposit(
            &sender,
            &recipient,
            &token,
            &rate_scaled,
            &TOKEN_DECIMALS,
            &1_000,
            &deposit,
        );
        env.ledger().set_timestamp(1_000 + elapsed);

        assert_flow_accounting_invariants(&client, &token_client, &contract_id, stream_id);

        let covered = client.covered_debt_of(&stream_id);
        let withdrawal = covered * withdraw_percent / 100;
        if withdrawal > 0 {
            client.withdraw(&stream_id, &recipient, &recipient, &withdrawal);
        }
        assert_flow_accounting_invariants(&client, &token_client, &contract_id, stream_id);

        let refundable = client.refundable_amount_of(&stream_id);
        let refund = refundable * refund_percent / 100;
        if refund > 0 {
            client.refund(&stream_id, &sender, &refund);
        }
        assert_flow_accounting_invariants(&client, &token_client, &contract_id, stream_id);

        prop_assert_eq!(
            token_client.balance(&sender)
                + token_client.balance(&recipient)
                + token_client.balance(&contract_id),
            initial_supply
        );
    }

    #[test]
    fn prop_flow_pause_preserves_debt_and_restart_accrues_from_snapshot(
        deposit in 1_000_000_i128..=1_000_000_000_000_i128,
        first_rate in 1_i128..=10_000_i128,
        second_rate in 1_i128..=10_000_i128,
        first_elapsed in 0_u64..=10_000_u64,
        paused_elapsed in 0_u64..=10_000_u64,
        second_elapsed in 0_u64..=10_000_u64,
    ) {
        let (env, contract_id, sender, recipient, token, token_client) =
            setup_test_with_snapshots(false);
        let client = get_client(&env, &contract_id);
        let first_rate_scaled = first_rate * 100_000_000_000_i128;
        let second_rate_scaled = second_rate * 100_000_000_000_i128;
        let stream_id = client.create_and_deposit(
            &sender,
            &recipient,
            &token,
            &first_rate_scaled,
            &TOKEN_DECIMALS,
            &1_000,
            &deposit,
        );

        env.ledger().set_timestamp(1_000 + first_elapsed);
        client.pause(&stream_id, &sender);
        let debt_at_pause = client.total_debt_of(&stream_id);
        env.ledger().set_timestamp(1_000 + first_elapsed + paused_elapsed);
        prop_assert_eq!(client.total_debt_of(&stream_id), debt_at_pause);

        client.restart(&stream_id, &sender, &second_rate_scaled);
        env.ledger().set_timestamp(1_000 + first_elapsed + paused_elapsed + second_elapsed);
        prop_assert_eq!(
            client.total_debt_of(&stream_id),
            debt_at_pause + second_rate * second_elapsed as i128
        );
        assert_flow_accounting_invariants(&client, &token_client, &contract_id, stream_id);
    }
}

// ---------------------------------------------------------------------------
// Initialization Tests
// ---------------------------------------------------------------------------

#[test]
fn test_constructor_sets_admin() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(FlowContract, FlowContractArgs::__constructor(&admin));
    let client = FlowContractClient::new(&env, &contract_id);
    client.set_admin(&admin);
}

// ---------------------------------------------------------------------------
// Stream Creation Tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_stream() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64, // start now
    );

    assert_eq!(stream_id, 1);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.sender, sender);
    assert_eq!(stream.recipient, recipient);
    assert_eq!(stream.token, token);
    assert_eq!(stream.rate_per_second, RATE_1_PER_SEC);
    assert_eq!(stream.balance, 0);
    assert!(!stream.is_voided);
}

#[test]
fn test_create_multiple_streams() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let id1 = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    let id2 = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
}

#[test]
fn test_create_with_future_start() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // Start 100 seconds in the future
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &1100u64,
    );

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::Pending);
}

#[test]
fn test_create_and_deposit() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let amount = 100 * ONE_TOKEN;
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &amount,
    );

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.balance, amount);

    // Contract should hold the tokens
    assert_eq!(token_client.balance(&contract_id), amount);
}

// ---------------------------------------------------------------------------
// Deposit Tests
// ---------------------------------------------------------------------------

#[test]
fn test_deposit() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );

    let amount = 50 * ONE_TOKEN;
    client.deposit(&stream_id, &sender, &amount);

    assert_eq!(client.get_balance(&stream_id), amount);
    assert_eq!(token_client.balance(&contract_id), amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")] // DepositAmountZero
fn test_deposit_zero_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.deposit(&stream_id, &sender, &0i128);
}

// ---------------------------------------------------------------------------
// Withdraw Tests
// ---------------------------------------------------------------------------

#[test]
fn test_withdraw_after_time() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    // Create and deposit 100 tokens
    let deposit_amount = 100 * ONE_TOKEN;
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &deposit_amount,
    );

    // Advance time by 10 seconds
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // After 10 seconds at 1 token/sec, 10 tokens should be withdrawable
    let withdrawable = client.withdrawable_amount_of(&stream_id);
    assert_eq!(withdrawable, 10 * ONE_TOKEN);

    // Withdraw 5 tokens
    let withdraw_amount = 5 * ONE_TOKEN;
    client.withdraw(&stream_id, &recipient, &recipient, &withdraw_amount);

    // Recipient should have received the tokens
    assert_eq!(token_client.balance(&recipient), withdraw_amount);

    // Stream balance should be reduced
    assert_eq!(
        client.get_balance(&stream_id),
        deposit_amount - withdraw_amount
    );
}

#[test]
fn test_withdraw_max() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let deposit_amount = 100 * ONE_TOKEN;
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &deposit_amount,
    );

    // Advance time by 20 seconds
    env.ledger().set(LedgerInfo {
        timestamp: 1020,
        protocol_version: 25,
        sequence_number: 120,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let withdrawn = client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_eq!(withdrawn, 20 * ONE_TOKEN);
    assert_eq!(token_client.balance(&recipient), 20 * ONE_TOKEN);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")] // Overdraw
fn test_withdraw_overdraw_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(10 * ONE_TOKEN),
    );

    // Advance time by 5 seconds (5 tokens owed)
    env.ledger().set(LedgerInfo {
        timestamp: 1005,
        protocol_version: 25,
        sequence_number: 105,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Try to withdraw 6 tokens (only 5 available)
    client.withdraw(&stream_id, &recipient, &recipient, &(6 * ONE_TOKEN));
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")] // Unauthorized
fn test_withdraw_wrong_caller_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Sender tries to withdraw (only recipient should be able to)
    client.withdraw(&stream_id, &sender, &sender, &(5 * ONE_TOKEN));
}

// ---------------------------------------------------------------------------
// Pause / Restart Tests
// ---------------------------------------------------------------------------

#[test]
fn test_pause_and_restart() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    // Advance 10 seconds
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Pause the stream
    client.pause(&stream_id, &sender);

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::PausedSolvent);

    // Debt should be frozen at 10 tokens
    assert_eq!(client.get_rate_per_second(&stream_id), 0);

    // Advance another 10 seconds — debt should NOT increase
    env.ledger().set(LedgerInfo {
        timestamp: 1020,
        protocol_version: 25,
        sequence_number: 120,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let total_debt = client.total_debt_of(&stream_id);
    assert_eq!(total_debt, 10 * ONE_TOKEN); // Still 10, not 20

    // Restart with a new rate (2 tokens/sec)
    let new_rate = 2 * RATE_1_PER_SEC;
    client.restart(&stream_id, &sender, &new_rate);

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::StreamingSolvent);
    assert_eq!(client.get_rate_per_second(&stream_id), new_rate);

    // Advance 5 more seconds — should accrue 10 additional tokens (2/sec × 5)
    env.ledger().set(LedgerInfo {
        timestamp: 1025,
        protocol_version: 25,
        sequence_number: 125,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let total_debt = client.total_debt_of(&stream_id);
    assert_eq!(total_debt, 20 * ONE_TOKEN); // 10 (pre-pause) + 10 (5s × 2/s)
}

// ---------------------------------------------------------------------------
// Rate Adjustment Tests
// ---------------------------------------------------------------------------

#[test]
fn test_adjust_rate() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    // Advance 10 seconds (10 tokens accrued at 1/sec)
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Double the rate
    let new_rate = 2 * RATE_1_PER_SEC;
    client.adjust_rate(&stream_id, &sender, &new_rate);

    // Advance another 5 seconds (10 tokens at 2/sec)
    env.ledger().set(LedgerInfo {
        timestamp: 1015,
        protocol_version: 25,
        sequence_number: 115,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let total_debt = client.total_debt_of(&stream_id);
    assert_eq!(total_debt, 20 * ONE_TOKEN); // 10 (pre-adjust) + 10 (5s × 2/s)
}

// ---------------------------------------------------------------------------
// Refund Tests
// ---------------------------------------------------------------------------

#[test]
fn test_refund() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let deposit_amount = 100 * ONE_TOKEN;
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &deposit_amount,
    );

    // Advance 10 seconds (10 tokens owed, 90 refundable)
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let refundable = client.refundable_amount_of(&stream_id);
    assert_eq!(refundable, 90 * ONE_TOKEN);

    // Refund 50 tokens
    let refund_amount = 50 * ONE_TOKEN;
    let sender_balance_before = token_client.balance(&sender);
    client.refund(&stream_id, &sender, &refund_amount);

    assert_eq!(
        token_client.balance(&sender),
        sender_balance_before + refund_amount
    );
    assert_eq!(
        client.get_balance(&stream_id),
        deposit_amount - refund_amount
    );
}

#[test]
fn test_refund_max() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let deposit_amount = 100 * ONE_TOKEN;
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &deposit_amount,
    );

    // Advance 10 seconds
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let sender_balance_before = token_client.balance(&sender);
    let refunded = client.refund_max(&stream_id, &sender);
    assert_eq!(refunded, 90 * ONE_TOKEN);
    assert_eq!(
        token_client.balance(&sender),
        sender_balance_before + 90 * ONE_TOKEN
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")] // RefundOverflow
fn test_refund_too_much_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    // Advance 10 seconds (10 owed, 90 refundable)
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Try to refund 95 tokens (only 90 refundable)
    client.refund(&stream_id, &sender, &(95 * ONE_TOKEN));
}

// ---------------------------------------------------------------------------
// Void Tests
// ---------------------------------------------------------------------------

#[test]
fn test_void_solvent_stream() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    // Advance 10 seconds
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Sender voids the stream
    client.void_stream(&stream_id, &sender);

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::Voided);
    assert_eq!(client.get_rate_per_second(&stream_id), 0);

    // Total debt should be frozen at 10 tokens
    let total_debt = client.total_debt_of(&stream_id);
    assert_eq!(total_debt, 10 * ONE_TOKEN);
}

#[test]
fn test_void_insolvent_stream() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // Deposit only 5 tokens
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(5 * ONE_TOKEN),
    );

    // Advance 10 seconds (10 tokens owed but only 5 in balance)
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Verify insolvent
    let uncovered = client.uncovered_debt_of(&stream_id);
    assert_eq!(uncovered, 5 * ONE_TOKEN);

    // Recipient voids (both sender and recipient can void)
    client.void_stream(&stream_id, &recipient);

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::Voided);

    // Uncovered debt should be written off (total debt = balance)
    let total_debt = client.total_debt_of(&stream_id);
    assert_eq!(total_debt, 5 * ONE_TOKEN); // Written down from 10 to 5
    assert_eq!(client.uncovered_debt_of(&stream_id), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")] // StreamVoided
fn test_void_twice_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    client.void_stream(&stream_id, &sender);
    client.void_stream(&stream_id, &sender); // Should panic
}

// ---------------------------------------------------------------------------
// Status Tests
// ---------------------------------------------------------------------------

#[test]
fn test_status_streaming_solvent() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::StreamingSolvent);
}

#[test]
fn test_status_streaming_insolvent() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // Small deposit
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(5 * ONE_TOKEN),
    );

    // Advance past the balance
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let status = client.status_of(&stream_id);
    assert_eq!(status, StreamStatus::StreamingInsolvent);
}

// ---------------------------------------------------------------------------
// Full Lifecycle Test
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    // 1. Create stream
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    assert_eq!(client.status_of(&stream_id), StreamStatus::StreamingSolvent);

    // 2. Deposit 50 tokens
    client.deposit(&stream_id, &sender, &(50 * ONE_TOKEN));
    assert_eq!(client.get_balance(&stream_id), 50 * ONE_TOKEN);

    // 3. Advance 10 seconds, withdraw 10 tokens
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    client.withdraw(&stream_id, &recipient, &recipient, &(10 * ONE_TOKEN));
    assert_eq!(token_client.balance(&recipient), 10 * ONE_TOKEN);

    // 4. Pause
    client.pause(&stream_id, &sender);
    assert_eq!(client.status_of(&stream_id), StreamStatus::PausedSolvent);

    // 5. Advance time while paused — no debt increase
    env.ledger().set(LedgerInfo {
        timestamp: 1020,
        protocol_version: 25,
        sequence_number: 120,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    // Total debt should still be 10 tokens (snapshot at pause)
    // But we already withdrew 10 tokens, so covered_debt_of should be 0
    // Wait — let's check: total_debt = 10, already withdrawn = 10 (from balance reduction)
    // The snapshot captured 10 tokens of debt, we withdrew 10, so snapshot_debt went down

    // 6. Restart with half rate
    let half_rate = RATE_1_PER_SEC / 2;
    client.restart(&stream_id, &sender, &half_rate);
    assert_eq!(client.status_of(&stream_id), StreamStatus::StreamingSolvent);

    // 7. Advance 10 more seconds (5 tokens at 0.5/sec)
    env.ledger().set(LedgerInfo {
        timestamp: 1030,
        protocol_version: 25,
        sequence_number: 130,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // 8. Refund excess
    let refundable = client.refundable_amount_of(&stream_id);
    assert!(refundable > 0);

    // 9. Void the stream
    client.void_stream(&stream_id, &sender);
    assert_eq!(client.status_of(&stream_id), StreamStatus::Voided);
}

// ---------------------------------------------------------------------------
// Edge Cases & Error Tests
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #20)")] // SenderEqualsRecipient
fn test_create_sender_equals_recipient() {
    let (env, contract_id, sender, _recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    client.create(
        &sender,
        &sender,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")] // InvalidTokenDecimals
fn test_create_invalid_decimals() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &19u32, &0u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #21)")] // NegativeRate
fn test_create_negative_rate() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    client.create(&sender, &recipient, &token, &-1i128, &TOKEN_DECIMALS, &0u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")] // CreateRatePerSecondZero
fn test_create_pending_with_zero_rate() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    client.create(
        &sender,
        &recipient,
        &token,
        &0i128,
        &TOKEN_DECIMALS,
        &2000u64,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")] // WithdrawAmountZero
fn test_withdraw_zero_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );
    client.withdraw(&stream_id, &recipient, &recipient, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // StreamPaused
fn test_pause_already_paused_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.pause(&stream_id, &sender);
    client.pause(&stream_id, &sender); // Should fail
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")] // StreamPending
fn test_pause_pending_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &2000u64,
    );
    client.pause(&stream_id, &sender);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")] // StreamVoided
fn test_pause_voided_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.void_stream(&stream_id, &sender);
    client.pause(&stream_id, &sender);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")] // Unauthorized
fn test_pause_unauthorized() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.pause(&stream_id, &recipient);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")] // StreamNotPaused
fn test_restart_not_paused_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.restart(&stream_id, &sender, &RATE_1_PER_SEC);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")] // StreamVoided
fn test_restart_voided_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.void_stream(&stream_id, &sender);
    client.restart(&stream_id, &sender, &RATE_1_PER_SEC);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // RatePerSecondZero
fn test_restart_zero_rate_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.pause(&stream_id, &sender);
    client.restart(&stream_id, &sender, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")] // RateNotDifferent
fn test_adjust_rate_same_rate_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.adjust_rate(&stream_id, &sender, &RATE_1_PER_SEC);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // RatePerSecondZero
fn test_adjust_rate_zero_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.adjust_rate(&stream_id, &sender, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")] // RefundAmountZero
fn test_refund_zero_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(100 * ONE_TOKEN),
    );
    client.refund(&stream_id, &sender, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")] // BalanceZero
fn test_depletion_time_zero_balance_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
    );
    client.depletion_time_of(&stream_id);
}

#[test]
fn test_depletion_time_calculation() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0u64,
        &(10 * ONE_TOKEN),
    );
    let dt = client.depletion_time_of(&stream_id);
    assert!(dt > 1000);
}

#[test]
fn test_failed_and_non_standard_token_calls_roll_back_flow_state() {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.ledger().set_protocol_version(25);
    env.ledger().set_timestamp(1_000);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = env.register(AdversarialToken, ());
    let adversarial = AdversarialTokenClient::new(&env, &token);
    adversarial.mint(&sender, &(1_000 * ONE_TOKEN));
    let contract_id = env.register(FlowContract, FlowContractArgs::__constructor(&admin));
    let client = FlowContractClient::new(&env, &contract_id);
    let stream_id = client.create(
        &sender,
        &recipient,
        &token,
        &RATE_1_PER_SEC,
        &TOKEN_DECIMALS,
        &0,
    );

    for mode in [1_u32, 2, 3] {
        adversarial.set_mode(&mode);
        assert!(client
            .try_deposit(&stream_id, &sender, &(100 * ONE_TOKEN))
            .is_err());
        assert_eq!(client.get_balance(&stream_id), 0);
        assert_eq!(adversarial.balance(&sender), 1_000 * ONE_TOKEN);
        assert_eq!(adversarial.balance(&contract_id), 0);
    }

    adversarial.set_mode(&0);
    client.deposit(&stream_id, &sender, &(100 * ONE_TOKEN));
    env.ledger().set_timestamp(1_010);
    let balance_before = client.get_balance(&stream_id);
    let debt_before = client.total_debt_of(&stream_id);

    for mode in [1_u32, 2, 3] {
        adversarial.set_mode(&mode);
        assert!(client
            .try_withdraw(&stream_id, &recipient, &recipient, &ONE_TOKEN)
            .is_err());
        assert_eq!(client.get_balance(&stream_id), balance_before);
        assert_eq!(client.total_debt_of(&stream_id), debt_before);
        assert_eq!(adversarial.balance(&recipient), 0);

        assert!(client.try_refund(&stream_id, &sender, &ONE_TOKEN).is_err());
        assert_eq!(client.get_balance(&stream_id), balance_before);
        assert_eq!(adversarial.balance(&contract_id), balance_before);
    }
}
