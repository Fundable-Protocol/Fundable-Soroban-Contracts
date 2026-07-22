#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String};

// Import the actual contract types to register them in tests
use flow::FlowContract;
use lockup::LockupContract;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient;
use stream_nft::StreamNftContract;

#[test]
fn test_initialize() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let router_id = env.register(RouterContract, ());
    let client = RouterContractClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    let flow = Address::generate(&env);
    let lockup = Address::generate(&env);
    let nft = Address::generate(&env);

    client.initialize(&admin, &flow, &lockup, &nft);

    // Initialized correctly without panic
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #301)")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let router_id = env.register(RouterContract, ());
    let client = RouterContractClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    let flow = Address::generate(&env);
    let lockup = Address::generate(&env);
    let nft = Address::generate(&env);

    client.initialize(&admin, &flow, &lockup, &nft);
    client.initialize(&admin, &flow, &lockup, &nft);
}

#[test]
fn test_end_to_end_flow_stream() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    // 1. Deploy token
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_client = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    // 2. Deploy core contracts
    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    // Initialize core contracts
    let admin = Address::generate(&env);
    flow_client::Client::new(&env, &flow_id).initialize(&admin);
    lockup_client::Client::new(&env, &lockup_id).initialize(&admin);
    nft_client::Client::new(&env, &nft_id).initialize(
        &router_id,
        &String::from_str(&env, "Fundable Stream NFT"),
        &String::from_str(&env, "FSNFT"),
    );

    // Initialize router
    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    // 3. Setup users
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint 1000 tokens to sender
    let decimals = 10u32.pow(7);
    token_admin_client.mint(&sender, &(1000 * decimals as i128));

    // 4. Create Flow Stream via Router
    let rate_per_second = 1_000_000_000_000_000_000; // 1 token per second in 1e18 fixed point

    // Set up ledger with a known timestamp
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let start_time = env.ledger().timestamp();

    let token_nft_id = router_client.create_flow_stream(
        &sender,
        &recipient,
        &token_id,
        &rate_per_second,
        &7, // decimals
        &start_time,
    );

    assert_eq!(token_nft_id, 1);

    // Verify NFT ownership
    let local_nft_client = nft_client::Client::new(&env, &nft_id);
    assert_eq!(local_nft_client.owner_of(&token_nft_id), recipient);
    assert_eq!(local_nft_client.balance(&recipient), 1);

    // 5. Deposit tokens into the flow stream
    // Sender must deposit into the flow stream directly, as creation does not fund it.
    let (stream_type, stream_id) = local_nft_client.get_stream_data(&token_nft_id);
    assert_eq!(stream_type as u32, nft_client::StreamType::Flow as u32);
    assert_eq!(stream_id, 1);

    flow_client::Client::new(&env, &flow_id).deposit(
        &stream_id,
        &sender,
        &(100 * decimals as i128),
    );

    // 6. Fast forward time and withdraw
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: start_time + 10, // 10 seconds pass = 10 tokens vested
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Recipient calls withdraw via router
    router_client.withdraw(
        &token_nft_id,
        &recipient,
        &recipient,
        &(10 * decimals as i128),
    );

    // Verify token balances
    assert_eq!(token_client.balance(&recipient), 10 * decimals as i128);
}

#[test]
fn test_end_to_end_lockup_stream() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    // 1. Deploy token
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_client = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    // 2. Deploy core contracts
    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    // Initialize core contracts
    let admin = Address::generate(&env);
    flow_client::Client::new(&env, &flow_id).initialize(&admin);
    lockup_client::Client::new(&env, &lockup_id).initialize(&admin);
    nft_client::Client::new(&env, &nft_id).initialize(
        &router_id,
        &String::from_str(&env, "Fundable Stream NFT"),
        &String::from_str(&env, "FSNFT"),
    );

    // Initialize router
    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    // 3. Setup users
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    // Mint tokens to sender
    let decimals = 10u32.pow(7);
    token_admin_client.mint(&sender, &(1000 * decimals as i128));

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let start_time = env.ledger().timestamp();
    let cliff_time = start_time + 10;
    let end_time = start_time + 100;

    let params = shared::types::CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token_id.clone(),
        total_amount: 100 * decimals as i128,
        start_time,
        end_time,
        cliff_time,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: false,
    };

    let token_nft_id = router_client.create_lockup_stream(&params);
    assert_eq!(token_nft_id, 1);

    let local_nft_client = nft_client::Client::new(&env, &nft_id);
    let (stream_type, stream_id) = local_nft_client.get_stream_data(&token_nft_id);
    assert_eq!(stream_type as u32, nft_client::StreamType::Lockup as u32);
    assert_eq!(stream_id, 1);

    // 4. Advance time past cliff
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: start_time + 55, // halfway done past cliff (45 / 90)
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // 5. Withdraw via Router
    router_client.withdraw(
        &token_nft_id,
        &recipient,
        &recipient,
        &(50 * decimals as i128),
    );
    assert_eq!(token_client.balance(&recipient), 50 * decimals as i128);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #303)")]
fn test_withdraw_fails_if_not_nft_owner() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    // 1. Deploy token
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    // 2. Deploy core contracts
    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    // Initialize core contracts
    let admin = Address::generate(&env);
    flow_client::Client::new(&env, &flow_id).initialize(&admin);
    lockup_client::Client::new(&env, &lockup_id).initialize(&admin);
    nft_client::Client::new(&env, &nft_id).initialize(
        &router_id,
        &String::from_str(&env, "Fundable Stream NFT"),
        &String::from_str(&env, "FSNFT"),
    );

    // Initialize router
    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    // 3. Setup users
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let malicious_actor = Address::generate(&env);

    let decimals = 10u32.pow(7);
    token_admin_client.mint(&sender, &(1000 * decimals as i128));

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let token_nft_id = router_client.create_flow_stream(
        &sender,
        &recipient,
        &token_id,
        &(1_000_000_000_000_000_000), // 1e18 rate
        &7,
        &env.ledger().timestamp(),
    );

    // Fast forward
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // Malicious actor tries to withdraw
    // Error 102 is RouterError::Unauthorized
    router_client.withdraw(
        &token_nft_id,
        &malicious_actor,
        &malicious_actor,
        &(10 * decimals as i128),
    );
}

#[test]
fn test_withdraw_after_nft_transfer() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_client = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    let admin = Address::generate(&env);
    flow_client::Client::new(&env, &flow_id).initialize(&admin);
    lockup_client::Client::new(&env, &lockup_id).initialize(&admin);
    let local_nft_client = nft_client::Client::new(&env, &nft_id);
    local_nft_client.initialize(
        &router_id,
        &String::from_str(&env, "Fundable Stream NFT"),
        &String::from_str(&env, "FSNFT"),
    );

    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let new_owner = Address::generate(&env);

    let decimals = 10u32.pow(7);
    token_admin_client.mint(&sender, &(1000 * decimals as i128));

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let token_nft_id = router_client.create_flow_stream(
        &sender,
        &recipient,
        &token_id,
        &(1_000_000_000_000_000_000), // 1e18 rate
        &7,
        &env.ledger().timestamp(),
    );

    let (_stream_type, stream_id) = local_nft_client.get_stream_data(&token_nft_id);
    flow_client::Client::new(&env, &flow_id).deposit(
        &stream_id,
        &sender,
        &(100 * decimals as i128),
    );

    // Transfer NFT to new owner
    local_nft_client.transfer(&recipient, &new_owner, &token_nft_id);
    assert_eq!(local_nft_client.owner_of(&token_nft_id), new_owner);

    // Fast forward
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    // New owner calls withdraw via router
    router_client.withdraw(
        &token_nft_id,
        &new_owner,
        &new_owner,
        &(10 * decimals as i128),
    );

    // Verify new owner got the tokens
    assert_eq!(token_client.balance(&new_owner), 10 * decimals as i128);
}

#[test]
fn test_upgrade() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    let admin = Address::generate(&env);
    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    // Use a valid WASM from our imports to test upgrading
    let new_wasm_hash = env.deployer().upload_contract_wasm(flow_client::WASM);
    router_client.upgrade(&new_wasm_hash);

    // Check that admin authorization was requested
    let auths = env.auths();
    assert!(auths.len() > 0);
    assert_eq!(auths[0].0, admin);
}

#[test]
fn test_upgrade_nft() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let flow_id = env.register(FlowContract, ());
    let lockup_id = env.register(LockupContract, ());
    let nft_id = env.register(StreamNftContract, ());
    let router_id = env.register(RouterContract, ());

    let admin = Address::generate(&env);

    // Initialize NFT with Router as admin
    nft_client::Client::new(&env, &nft_id).initialize(
        &router_id,
        &String::from_str(&env, "Fundable Stream NFT"),
        &String::from_str(&env, "FSNFT"),
    );

    let router_client = RouterContractClient::new(&env, &router_id);
    router_client.initialize(&admin, &flow_id, &lockup_id, &nft_id);

    // Use a valid WASM from our imports to test upgrading
    let new_wasm_hash = env.deployer().upload_contract_wasm(flow_client::WASM);
    router_client.upgrade_nft(&new_wasm_hash);

    // Check that admin authorization was requested
    let auths = env.auths();
    assert!(auths.len() > 0);
    assert_eq!(auths[0].0, admin);
}
