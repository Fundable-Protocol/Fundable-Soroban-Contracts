//! Tests for the Fundable Lockup contract.
//!
//! Covers:
//! - Full lifecycle: create → vest → withdraw → deplete
//! - Cliff behavior: no vesting before cliff
//! - Cancel: sender reclaims unvested tokens
//! - Renounce: permanently disabling cancel
//! - Granularity: discrete unlock steps
//! - Authorization failures
//! - Edge cases: settled streams, double cancel, etc.

#![cfg(test)]

extern crate std;

use super::*;
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{
        Address as _, AuthorizedFunction, AuthorizedInvocation, EnvTestConfig, Ledger, LedgerInfo,
    },
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

mod current_lockup_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/lockup.wasm");
}

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/// 1 token in 7-decimal representation.
const ONE_TOKEN: i128 = 10_000_000; // 1e7

/// Set up a test environment with a token and funded sender.
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

    // Create a test token (7 decimals)
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token = sac.address();
    let token_client = TokenClient::new(&env, &token);
    let sac_admin = StellarAssetClient::new(&env, &token);

    // Mint tokens to the sender (1,000,000 tokens)
    sac_admin.mint(&sender, &(1_000_000 * ONE_TOKEN));

    // Register the Lockup contract with atomic constructor arguments.
    let contract_id = env.register(LockupContract, LockupContractArgs::__constructor(&admin));

    (env, contract_id, sender, recipient, token, token_client)
}

fn get_client<'a>(env: &Env, contract_id: &Address) -> LockupContractClient<'a> {
    LockupContractClient::new(env, contract_id)
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

fn auth_params(sender: &Address, recipient: &Address, token: &Address) -> CreateLockupParams {
    CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1_000,
        end_time: 1_100,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    }
}

#[test]
fn test_exact_authorization_trees_for_sensitive_lockup_calls() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = auth_params(&sender, &recipient, &token);

    let withdraw_stream = client.create(&params);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "create",
        (params.clone(),).into_val(&env),
        std::vec![invocation(
            &env,
            &token,
            "transfer",
            (sender.clone(), contract_id.clone(), params.total_amount,).into_val(&env),
            std::vec![],
        )],
    );

    env.ledger().set_timestamp(1_050);
    client.withdraw(&withdraw_stream, &recipient, &recipient, &(10 * ONE_TOKEN));
    assert_exact_auth(
        &env,
        &recipient,
        &contract_id,
        "withdraw",
        (
            withdraw_stream,
            recipient.clone(),
            recipient.clone(),
            10 * ONE_TOKEN,
        )
            .into_val(&env),
        std::vec![],
    );
    client.withdraw_max(&withdraw_stream, &recipient, &recipient);
    assert_exact_auth(
        &env,
        &recipient,
        &contract_id,
        "withdraw_max",
        (withdraw_stream, recipient.clone(), recipient.clone()).into_val(&env),
        std::vec![],
    );

    let cancel_stream = client.create(&params);
    client.cancel(&cancel_stream, &sender);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "cancel",
        (cancel_stream, sender.clone()).into_val(&env),
        std::vec![],
    );

    let renounce_stream = client.create(&params);
    client.renounce(&renounce_stream, &sender);
    assert_exact_auth(
        &env,
        &sender,
        &contract_id,
        "renounce",
        (renounce_stream, sender.clone()).into_val(&env),
        std::vec![],
    );
}

#[test]
fn test_exact_authorization_trees_for_lockup_admin_calls() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let contract_id = env.register(LockupContract, LockupContractArgs::__constructor(&admin));
    let client = LockupContractClient::new(&env, &contract_id);

    let wasm_hash = env
        .deployer()
        .upload_contract_wasm(current_lockup_wasm::WASM);
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

fn assert_lockup_accounting_invariants(
    client: &LockupContractClient<'_>,
    token_client: &TokenClient<'_>,
    contract_id: &Address,
    stream_id: u64,
) {
    let deposited = client.get_deposited_amount(&stream_id);
    let withdrawn = client.get_withdrawn_amount(&stream_id);
    let refunded = client.get_refunded_amount(&stream_id);
    let streamed = client.streamed_amount_of(&stream_id);
    let withdrawable = client.withdrawable_amount_of(&stream_id);
    let stream = client.get_stream(&stream_id);
    let remaining = deposited - withdrawn - refunded;

    assert!(deposited > 0);
    assert!(withdrawn >= 0);
    assert!(refunded >= 0);
    assert!(remaining >= 0);
    assert!(streamed >= withdrawn && streamed <= deposited);
    assert_eq!(withdrawable, streamed - withdrawn);
    assert_eq!(stream.total_amount, deposited);
    assert_eq!(token_client.balance(contract_id), remaining);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn prop_lockup_conserves_assets_through_withdrawal_and_cancellation(
        total in 1_i128..=1_000_000_000_000_i128,
        duration in 2_u64..=100_000_u64,
        elapsed_percent in 0_u64..=100_u64,
        granularity_percent in 1_u64..=100_u64,
        start_unlock_percent in 0_i128..=100_i128,
        withdraw_percent in 0_i128..=100_i128,
    ) {
        let (env, contract_id, sender, recipient, token, token_client) =
            setup_test_with_snapshots(false);
        let client = get_client(&env, &contract_id);
        let initial_supply = token_client.balance(&sender);
        let start_unlock = total * start_unlock_percent / 100;
        let granularity = core::cmp::max(1, duration * granularity_percent / 100);
        let params = CreateLockupParams {
            sender: sender.clone(),
            recipient: recipient.clone(),
            token: token.clone(),
            total_amount: total,
            start_time: 1_000,
            end_time: 1_000 + duration,
            cliff_time: 0,
            start_unlock_amount: start_unlock,
            cliff_unlock_amount: 0,
            granularity,
            cancelable: true,
        };
        let stream_id = client.create(&params);
        env.ledger().set_timestamp(1_000 + duration * elapsed_percent / 100);

        assert_lockup_accounting_invariants(&client, &token_client, &contract_id, stream_id);
        let withdrawable = client.withdrawable_amount_of(&stream_id);
        let withdrawal = withdrawable * withdraw_percent / 100;
        if withdrawal > 0 {
            client.withdraw(&stream_id, &recipient, &recipient, &withdrawal);
        }
        assert_lockup_accounting_invariants(&client, &token_client, &contract_id, stream_id);

        client.cancel(&stream_id, &sender);
        let deposited = client.get_deposited_amount(&stream_id);
        let withdrawn = client.get_withdrawn_amount(&stream_id);
        let refunded = client.get_refunded_amount(&stream_id);
        let streamed = client.streamed_amount_of(&stream_id);
        let remaining = deposited - withdrawn - refunded;
        prop_assert_eq!(streamed, deposited - refunded);
        prop_assert_eq!(client.withdrawable_amount_of(&stream_id), remaining);
        prop_assert_eq!(token_client.balance(&contract_id), remaining);

        if remaining > 0 {
            client.withdraw_max(&stream_id, &recipient, &recipient);
        }
        prop_assert_eq!(
            client.get_withdrawn_amount(&stream_id)
                + client.get_refunded_amount(&stream_id),
            deposited
        );
        prop_assert_eq!(token_client.balance(&contract_id), 0);
        prop_assert_eq!(
            token_client.balance(&sender)
                + token_client.balance(&recipient)
                + token_client.balance(&contract_id),
            initial_supply
        );
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
    let contract_id = env.register(LockupContract, LockupContractArgs::__constructor(&admin));
    let client = LockupContractClient::new(&env, &contract_id);
    client.set_admin(&admin);
}

// ---------------------------------------------------------------------------
// Stream Creation Tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_stream() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    assert_eq!(stream_id, 1);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.sender, sender);
    assert_eq!(stream.recipient, recipient);
    assert_eq!(stream.total_amount, total);
    assert_eq!(stream.withdrawn_amount, 0);
    assert!(stream.cancelable);

    // Contract should hold the tokens
    assert_eq!(token_client.balance(&contract_id), total);
}

#[test]
fn test_create_with_cliff() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1500,
        start_unlock_amount: 10 * ONE_TOKEN,
        cliff_unlock_amount: 20 * ONE_TOKEN,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.cliff_time, 1500);
    assert_eq!(stream.start_unlock_amount, 10 * ONE_TOKEN);
    assert_eq!(stream.cliff_unlock_amount, 20 * ONE_TOKEN);
}

// ---------------------------------------------------------------------------
// Vesting Calculation Tests
// ---------------------------------------------------------------------------

#[test]
fn test_vesting_before_start() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // Stream starts in the future
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1100,
        end_time: 2100,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Before start: status should be Pending, nothing vested
    assert_eq!(client.status_of(&stream_id), LockupStatus::Pending);
    assert_eq!(client.streamed_amount_of(&stream_id), 0);
    assert_eq!(client.withdrawable_amount_of(&stream_id), 0);
}

#[test]
fn test_linear_vesting_no_cliff() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // 100 tokens over 1000 seconds (1 token/10sec)
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Advance to 50% (t=1500)
    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    assert_eq!(client.status_of(&stream_id), LockupStatus::Streaming);
    assert_eq!(client.streamed_amount_of(&stream_id), 50 * ONE_TOKEN);
    assert_eq!(client.withdrawable_amount_of(&stream_id), 50 * ONE_TOKEN);
}

#[test]
fn test_vesting_with_cliff() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // 100 tokens, start=1000, end=2000, cliff=1500
    // start_unlock=10, cliff_unlock=20
    // streamable = 100 - 10 - 20 = 70 tokens over 500 seconds (cliff to end)
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1500,
        start_unlock_amount: 10 * ONE_TOKEN,
        cliff_unlock_amount: 20 * ONE_TOKEN,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Before cliff (t=1200): only start_unlock_amount
    env.ledger().set(LedgerInfo {
        timestamp: 1200,
        protocol_version: 25,
        sequence_number: 120,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 10 * ONE_TOKEN);

    // At cliff (t=1500): start + cliff = 30 tokens
    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 30 * ONE_TOKEN);

    // Half way from cliff to end (t=1750): 30 + 70*250/500 = 30 + 35 = 65
    env.ledger().set(LedgerInfo {
        timestamp: 1750,
        protocol_version: 25,
        sequence_number: 175,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 65 * ONE_TOKEN);

    // At end (t=2000): all 100 tokens
    env.ledger().set(LedgerInfo {
        timestamp: 2000,
        protocol_version: 25,
        sequence_number: 200,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 100 * ONE_TOKEN);
    assert_eq!(client.status_of(&stream_id), LockupStatus::Settled);
}

#[test]
fn test_granularity() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    // 100 tokens over 1000 seconds, granularity = 100s
    // Tokens should unlock in 10% steps every 100 seconds.
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 100,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // At t=1050: only 0 tokens vested (floor(50/100) = 0 steps)
    env.ledger().set(LedgerInfo {
        timestamp: 1050,
        protocol_version: 25,
        sequence_number: 105,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 0);

    // At t=1100: 10 tokens (floor(100/100) * 100 * 100 / 1000 = 10)
    env.ledger().set(LedgerInfo {
        timestamp: 1100,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 10 * ONE_TOKEN);

    // At t=1150: still 10 tokens (floor(150/100) = 1 step)
    env.ledger().set(LedgerInfo {
        timestamp: 1150,
        protocol_version: 25,
        sequence_number: 115,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 10 * ONE_TOKEN);

    // At t=1500: 50 tokens (floor(500/100) * 100 * 100 / 1000 = 50)
    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 50 * ONE_TOKEN);
}

// ---------------------------------------------------------------------------
// Withdraw Tests
// ---------------------------------------------------------------------------

#[test]
fn test_withdraw() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Advance to 50% vested
    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Withdraw 30 tokens
    let amount = 30 * ONE_TOKEN;
    client.withdraw(&stream_id, &recipient, &recipient, &amount);

    assert_eq!(token_client.balance(&recipient), amount);
    assert_eq!(client.get_withdrawn_amount(&stream_id), amount);
    assert_eq!(client.withdrawable_amount_of(&stream_id), 20 * ONE_TOKEN);
}

#[test]
fn test_withdraw_max() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let withdrawn = client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_eq!(withdrawn, 50 * ONE_TOKEN);
    assert_eq!(token_client.balance(&recipient), 50 * ONE_TOKEN);
}

#[test]
fn test_withdraw_depletes_stream() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Advance past end
    env.ledger().set(LedgerInfo {
        timestamp: 3000,
        protocol_version: 25,
        sequence_number: 300,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Withdraw everything
    client.withdraw_max(&stream_id, &recipient, &recipient);

    assert_eq!(client.status_of(&stream_id), LockupStatus::Depleted);
    assert_eq!(client.withdrawable_amount_of(&stream_id), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #105)")] // Overdraw
fn test_withdraw_overdraw_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: (100 * ONE_TOKEN),
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Try to withdraw 60 tokens (only 50 vested)
    client.withdraw(&stream_id, &recipient, &recipient, &(60 * ONE_TOKEN));
}

#[test]
#[should_panic(expected = "Error(Contract, #102)")] // Unauthorized
fn test_withdraw_wrong_caller_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: (100 * ONE_TOKEN),
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Sender tries to withdraw (only recipient should be able to)
    client.withdraw(&stream_id, &sender, &sender, &(10 * ONE_TOKEN));
}

// ---------------------------------------------------------------------------
// Cancel Tests
// ---------------------------------------------------------------------------

#[test]
fn test_cancel() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let sender_balance_before = token_client.balance(&sender);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Advance to 30% vested
    env.ledger().set(LedgerInfo {
        timestamp: 1300,
        protocol_version: 25,
        sequence_number: 130,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let refunded = client.cancel(&stream_id, &sender);
    assert_eq!(refunded, 70 * ONE_TOKEN); // 70% unvested

    // Sender should get back 70 tokens
    assert_eq!(
        token_client.balance(&sender),
        sender_balance_before - total + refunded,
    );

    // Status should be Canceled
    assert_eq!(client.status_of(&stream_id), LockupStatus::Canceled);

    // Recipient should be able to withdraw the vested 30 tokens
    assert_eq!(client.withdrawable_amount_of(&stream_id), 30 * ONE_TOKEN);
}

#[test]
fn test_cancel_then_withdraw() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // Advance to 40% vested
    env.ledger().set(LedgerInfo {
        timestamp: 1400,
        protocol_version: 25,
        sequence_number: 140,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    client.cancel(&stream_id, &sender);

    // Recipient withdraws their 40 tokens
    let withdrawn = client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_eq!(withdrawn, 40 * ONE_TOKEN);
    assert_eq!(token_client.balance(&recipient), 40 * ONE_TOKEN);

    // Stream should now be depleted
    assert_eq!(client.status_of(&stream_id), LockupStatus::Depleted);
}

#[test]
#[should_panic(expected = "Error(Contract, #104)")] // NotCancelable
fn test_cancel_non_cancelable_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: false, // not cancelable
    };
    let stream_id = client.create(&params);

    client.cancel(&stream_id, &sender);
}

#[test]
#[should_panic(expected = "Error(Contract, #103)")] // AlreadyCancelled
fn test_cancel_twice_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: (100 * ONE_TOKEN),
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    client.cancel(&stream_id, &sender);
    client.cancel(&stream_id, &sender); // Should panic
}

// ---------------------------------------------------------------------------
// Renounce Tests
// ---------------------------------------------------------------------------

#[test]
fn test_renounce() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: (100 * ONE_TOKEN),
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    assert!(client.is_cancelable(&stream_id));

    client.renounce(&stream_id, &sender);

    assert!(!client.is_cancelable(&stream_id));
}

#[test]
#[should_panic(expected = "Error(Contract, #104)")] // NotCancelable
fn test_cancel_after_renounce_fails() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: (100 * ONE_TOKEN),
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    client.renounce(&stream_id, &sender);
    client.cancel(&stream_id, &sender); // Should panic
}

// ---------------------------------------------------------------------------
// Status Tests
// ---------------------------------------------------------------------------

#[test]
fn test_status_lifecycle() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);

    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1100,
        end_time: 2100,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: false,
    };
    let stream_id = client.create(&params);

    // Before start: Pending (warm)
    assert_eq!(client.status_of(&stream_id), LockupStatus::Pending);
    assert!(client.is_warm(&stream_id));
    assert!(!client.is_cold(&stream_id));

    // During vesting: Streaming (warm)
    env.ledger().set(LedgerInfo {
        timestamp: 1500,
        protocol_version: 25,
        sequence_number: 150,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.status_of(&stream_id), LockupStatus::Streaming);
    assert!(client.is_warm(&stream_id));

    // After end: Settled (cold)
    env.ledger().set(LedgerInfo {
        timestamp: 3000,
        protocol_version: 25,
        sequence_number: 300,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.status_of(&stream_id), LockupStatus::Settled);
    assert!(client.is_cold(&stream_id));

    // Withdraw all: Depleted (cold)
    client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_eq!(client.status_of(&stream_id), LockupStatus::Depleted);
    assert!(client.is_cold(&stream_id));
}

// ---------------------------------------------------------------------------
// Full Lifecycle Test
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle() {
    let (env, contract_id, sender, recipient, token, token_client) = setup_test();
    let client = get_client(&env, &contract_id);

    let total = 100 * ONE_TOKEN;

    // 1. Create stream with cliff
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: total,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1200,
        start_unlock_amount: 5 * ONE_TOKEN,
        cliff_unlock_amount: 15 * ONE_TOKEN,
        granularity: 1,
        cancelable: true,
    };
    let stream_id = client.create(&params);

    // 2. Before cliff — only start_unlock_amount
    env.ledger().set(LedgerInfo {
        timestamp: 1100,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.streamed_amount_of(&stream_id), 5 * ONE_TOKEN);

    // 3. Withdraw start amount
    client.withdraw(&stream_id, &recipient, &recipient, &(5 * ONE_TOKEN));
    assert_eq!(token_client.balance(&recipient), 5 * ONE_TOKEN);

    // 4. After cliff — unlock amounts + linear portion
    env.ledger().set(LedgerInfo {
        timestamp: 1600,
        protocol_version: 25,
        sequence_number: 160,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    // streamable = 100 - 5 - 15 = 80, over 800 seconds (1200 to 2000)
    // elapsed = 400, streamed_portion = 400 * 80 / 800 = 40
    // total vested = 5 + 15 + 40 = 60
    assert_eq!(client.streamed_amount_of(&stream_id), 60 * ONE_TOKEN);
    // Already withdrew 5, so withdrawable = 55
    assert_eq!(client.withdrawable_amount_of(&stream_id), 55 * ONE_TOKEN);

    // 5. Refundable = 100 - 60 = 40
    assert_eq!(client.refundable_amount_of(&stream_id), 40 * ONE_TOKEN);

    // 6. After end — fully settled
    env.ledger().set(LedgerInfo {
        timestamp: 2500,
        protocol_version: 25,
        sequence_number: 250,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    assert_eq!(client.status_of(&stream_id), LockupStatus::Settled);
    assert_eq!(client.streamed_amount_of(&stream_id), 100 * ONE_TOKEN);

    // 7. Withdraw remaining
    let withdrawn = client.withdraw_max(&stream_id, &recipient, &recipient);
    assert_eq!(withdrawn, 95 * ONE_TOKEN); // 100 - 5 already withdrawn
    assert_eq!(client.status_of(&stream_id), LockupStatus::Depleted);
}

// ---------------------------------------------------------------------------
// Edge Cases & Error Tests
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #110)")] // SenderEqualsRecipient
fn test_create_sender_equals_recipient() {
    let (env, contract_id, sender, _recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: sender.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}

#[test]
#[should_panic(expected = "Error(Contract, #107)")] // AmountZero
fn test_create_zero_amount() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 0,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}

#[test]
#[should_panic(expected = "Error(Contract, #106)")] // InvalidTimeRange
fn test_create_invalid_time_range() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: 100 * ONE_TOKEN,
        start_time: 2000, // Start after end
        end_time: 1000,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}

#[test]
#[should_panic(expected = "Error(Contract, #111)")] // InvalidUnlockAmount
fn test_create_rejects_negative_start_unlock_amount() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender,
        recipient,
        token,
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1500,
        start_unlock_amount: -ONE_TOKEN,
        cliff_unlock_amount: 10 * ONE_TOKEN,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}

#[test]
#[should_panic(expected = "Error(Contract, #111)")] // InvalidUnlockAmount
fn test_create_rejects_negative_cliff_offset_attack() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender,
        recipient,
        token,
        total_amount: 100 * ONE_TOKEN,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1500,
        start_unlock_amount: 150 * ONE_TOKEN,
        cliff_unlock_amount: -100 * ONE_TOKEN,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}

#[test]
#[should_panic(expected = "Error(Contract, #111)")] // InvalidUnlockAmount
fn test_create_rejects_unlock_sum_overflow() {
    let (env, contract_id, sender, recipient, token, _) = setup_test();
    let client = get_client(&env, &contract_id);
    let params = CreateLockupParams {
        sender,
        recipient,
        token,
        total_amount: i128::MAX,
        start_time: 1000,
        end_time: 2000,
        cliff_time: 1500,
        start_unlock_amount: i128::MAX,
        cliff_unlock_amount: 1,
        granularity: 1,
        cancelable: true,
    };
    client.create(&params);
}
