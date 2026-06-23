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

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

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
fn setup_test() -> (Env, Address, Address, Address, Address, TokenClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    // Set up ledger with a known timestamp
    env.ledger().set(LedgerInfo {
        timestamp: 1000,
        protocol_version: 22,
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

    // Register and initialize the Flow contract
    let contract_id = env.register(FlowContract, ());
    let client = FlowContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // We return the contract address as the `admin` parameter position
    // and use `client` through the contract_id
    (env, contract_id, sender, recipient, token, token_client)
}

/// Create a helper to get a client from env + contract_id
fn get_client<'a>(env: &Env, contract_id: &Address) -> FlowContractClient<'a> {
    FlowContractClient::new(env, contract_id)
}

// ---------------------------------------------------------------------------
// Initialization Tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(FlowContract, ());
    let client = FlowContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    // Should succeed without panic
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")] // AlreadyInitialized
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(FlowContract, ());
    let client = FlowContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.initialize(&admin); // Should panic
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
    assert_eq!(stream.is_voided, false);
}

#[test]
fn test_create_multiple_streams() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let id1 = client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &TOKEN_DECIMALS, &0u64);
    let id2 = client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &TOKEN_DECIMALS, &0u64);

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

    let stream_id = client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &TOKEN_DECIMALS, &0u64);

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

    let stream_id = client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &TOKEN_DECIMALS, &0u64);
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
        protocol_version: 22,
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
    assert_eq!(client.get_balance(&stream_id), deposit_amount - withdraw_amount);
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
    assert_eq!(client.get_balance(&stream_id), deposit_amount - refund_amount);
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
    let stream_id =
        client.create(&sender, &recipient, &token, &RATE_1_PER_SEC, &TOKEN_DECIMALS, &0u64);
    assert_eq!(client.status_of(&stream_id), StreamStatus::StreamingSolvent);

    // 2. Deposit 50 tokens
    client.deposit(&stream_id, &sender, &(50 * ONE_TOKEN));
    assert_eq!(client.get_balance(&stream_id), 50 * ONE_TOKEN);

    // 3. Advance 10 seconds, withdraw 10 tokens
    env.ledger().set(LedgerInfo {
        timestamp: 1010,
        protocol_version: 22,
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
        protocol_version: 22,
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
        protocol_version: 22,
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
