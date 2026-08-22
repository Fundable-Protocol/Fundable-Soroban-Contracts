extern crate std;

use super::*;
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger as _},
    xdr, Address, Env, IntoVal, String, Symbol, TryFromVal, Val, Vec,
};

// Import the actual contract types to register them in tests
use flow::{FlowContract, FlowContractArgs};
use lockup::{LockupContract, LockupContractArgs};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient;
use stream_nft::{StreamNftContract, StreamNftContractArgs};

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

fn has_dual_id_event(
    env: &Env,
    contract: &Address,
    event_name: &str,
    token_id: i128,
    core_stream_id: u64,
) -> bool {
    let event_symbol = xdr::ScVal::try_from_val(env, &Symbol::new(env, event_name)).unwrap();
    let token_val: Val = token_id.into_val(env);
    let core_val: Val = core_stream_id.into_val(env);
    let token_value = xdr::ScVal::try_from_val(env, &token_val).unwrap();
    let core_value = xdr::ScVal::try_from_val(env, &core_val).unwrap();
    env.events()
        .all()
        .filter_by_contract(contract)
        .events()
        .iter()
        .any(|event| {
            let xdr::ContractEventBody::V0(body) = &event.body;
            body.topics.len() >= 3
                && body.topics[0] == event_symbol
                && body.topics[1] == token_value
                && body.topics[2] == core_value
        })
}

fn register_protocol(env: &Env, admin: &Address) -> (Address, Address, Address, Address) {
    let flow_id = env.register(FlowContract, FlowContractArgs::__constructor(admin));
    let lockup_id = env.register(LockupContract, LockupContractArgs::__constructor(admin));
    let router_id = env.register(RouterContract, RouterContractArgs::__constructor(admin));
    let name = String::from_str(env, "Fundable Stream NFT");
    let symbol = String::from_str(env, "FSNFT");
    let nft_id = env.register(
        StreamNftContract,
        StreamNftContractArgs::__constructor(&router_id, &name, &symbol),
    );
    RouterContractClient::new(env, &router_id).configure(&flow_id, &lockup_id, &nft_id);
    (flow_id, lockup_id, nft_id, router_id)
}

#[test]
fn test_constructor_and_configure() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let router_id = env.register(RouterContract, RouterContractArgs::__constructor(&admin));
    let client = RouterContractClient::new(&env, &router_id);
    let flow = Address::generate(&env);
    let lockup = Address::generate(&env);
    let nft = Address::generate(&env);

    client.configure(&flow, &lockup, &nft);

    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, admin);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #305)")]
fn test_configure_twice_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let router_id = env.register(RouterContract, RouterContractArgs::__constructor(&admin));
    let client = RouterContractClient::new(&env, &router_id);
    let flow = Address::generate(&env);
    let lockup = Address::generate(&env);
    let nft = Address::generate(&env);

    client.configure(&flow, &lockup, &nft);
    client.configure(&flow, &lockup, &nft);
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

    // 2. Deploy and configure core contracts
    let admin = Address::generate(&env);
    let (flow_id, _lockup_id, nft_id, router_id) = register_protocol(&env, &admin);
    let router_client = RouterContractClient::new(&env, &router_id);

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
        &(100 * decimals as i128),
    );

    assert_eq!(
        env.auths(),
        std::vec![(
            sender.clone(),
            invocation(
                &env,
                &router_id,
                "create_flow_stream",
                (
                    &sender,
                    &recipient,
                    &token_id,
                    &rate_per_second,
                    7_u32,
                    start_time,
                    100 * decimals as i128,
                )
                    .into_val(&env),
                std::vec![invocation(
                    &env,
                    &flow_id,
                    "create_and_deposit",
                    (
                        &sender,
                        &router_id,
                        &token_id,
                        &rate_per_second,
                        7_u32,
                        start_time,
                        100 * decimals as i128,
                    )
                        .into_val(&env),
                    std::vec![invocation(
                        &env,
                        &token_id,
                        "transfer",
                        (&sender, &flow_id, 100 * decimals as i128).into_val(&env),
                        std::vec![],
                    )],
                )],
            ),
        )]
    );

    assert_eq!(token_nft_id, 1);
    assert!(has_dual_id_event(
        &env,
        &router_id,
        "stream_created",
        token_nft_id,
        1,
    ));

    // Verify NFT ownership
    let local_nft_client = nft_client::Client::new(&env, &nft_id);
    assert_eq!(local_nft_client.owner_of(&token_nft_id), recipient);
    assert_eq!(local_nft_client.balance(&recipient), 1);

    // 5. Verify the public-to-core mapping and atomic initial funding.
    let (stream_type, stream_id) = local_nft_client.get_stream_data(&token_nft_id);
    assert_eq!(stream_type as u32, nft_client::StreamType::Flow as u32);
    assert_eq!(stream_id, 1);
    assert_eq!(
        flow_client::Client::new(&env, &flow_id).get_balance(&stream_id),
        100 * decimals as i128
    );
    assert_eq!(router_client.owner_of(&token_nft_id), recipient);
    assert_eq!(router_client.stream_type(&token_nft_id), StreamType::Flow);
    assert_eq!(router_client.core_stream_id(&token_nft_id), stream_id);
    assert_eq!(
        router_client.status_of(&token_nft_id),
        CanonicalStreamStatus::Active
    );
    let metadata = router_client.get_stream(&token_nft_id);
    assert_eq!(metadata.token_id, token_nft_id);
    assert_eq!(metadata.core_stream_id, stream_id);
    assert!(metadata.transferable);

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
    assert!(has_dual_id_event(
        &env,
        &router_id,
        "stream_withdrawn",
        token_nft_id,
        stream_id,
    ));

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

    // 2. Deploy and configure core contracts
    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, nft_id, router_id) = register_protocol(&env, &admin);
    let router_client = RouterContractClient::new(&env, &router_id);

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

    // 2. Deploy and configure core contracts
    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, _nft_id, router_id) = register_protocol(&env, &admin);
    let router_client = RouterContractClient::new(&env, &router_id);

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
        &(100 * decimals as i128),
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

    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, nft_id, router_id) = register_protocol(&env, &admin);
    let local_nft_client = nft_client::Client::new(&env, &nft_id);
    let router_client = RouterContractClient::new(&env, &router_id);

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
fn test_nft_owner_voids_flow_and_terminal_receipt_persists() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    let admin = Address::generate(&env);
    let (flow_id, _lockup_id, nft_id, router_id) = register_protocol(&env, &admin);
    let flow = flow_client::Client::new(&env, &flow_id);
    let nft = nft_client::Client::new(&env, &nft_id);
    let router = RouterContractClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let terminal_owner = Address::generate(&env);
    let decimals = 10u32.pow(7) as i128;
    token_admin_client.mint(&sender, &(1_000 * decimals));

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1_000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let token_nft_id = router.create_flow_stream(
        &sender,
        &recipient,
        &token_id,
        &1_000_000_000_000_000_000,
        &7,
        &1_000,
        &(100 * decimals),
    );
    let core_stream_id = router.core_stream_id(&token_nft_id);

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1_010,
        protocol_version: 25,
        sequence_number: 110,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    router.void_flow(&token_nft_id, &recipient);
    assert_eq!(
        env.auths(),
        std::vec![(
            recipient.clone(),
            invocation(
                &env,
                &router_id,
                "void_flow",
                (&token_nft_id, &recipient).into_val(&env),
                std::vec![],
            ),
        )]
    );
    assert!(has_dual_id_event(
        &env,
        &router_id,
        "stream_voided",
        token_nft_id,
        core_stream_id,
    ));
    assert_eq!(
        flow.status_of(&core_stream_id),
        flow_client::StreamStatus::Voided
    );
    assert_eq!(
        router.status_of(&token_nft_id),
        CanonicalStreamStatus::Canceled
    );

    assert_eq!(
        router.withdraw_max(&token_nft_id, &recipient, &recipient),
        10 * decimals
    );
    assert_eq!(
        env.auths(),
        std::vec![(
            recipient.clone(),
            invocation(
                &env,
                &router_id,
                "withdraw_max",
                (&token_nft_id, &recipient, &recipient).into_val(&env),
                std::vec![],
            ),
        )]
    );
    assert_eq!(flow.refund_max(&core_stream_id, &sender), 90 * decimals);
    assert_eq!(
        router.status_of(&token_nft_id),
        CanonicalStreamStatus::Completed
    );

    // Completion preserves a transferable receipt and its immutable mapping.
    assert!(nft.is_transferable(&token_nft_id));
    nft.transfer(&recipient, &terminal_owner, &token_nft_id);
    assert_eq!(router.owner_of(&token_nft_id), terminal_owner);
    assert_eq!(router.core_stream_id(&token_nft_id), core_stream_id);
    assert_eq!(
        router.status_of(&token_nft_id),
        CanonicalStreamStatus::Completed
    );
}

#[test]
fn test_lockup_transfer_partial_withdraw_cancel_and_terminal_withdraw() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    let admin = Address::generate(&env);
    let (_flow_id, lockup_id, nft_id, router_id) = register_protocol(&env, &admin);
    let lockup = lockup_client::Client::new(&env, &lockup_id);
    let nft = nft_client::Client::new(&env, &nft_id);
    let router = RouterContractClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let initial_owner = Address::generate(&env);
    let partial_owner = Address::generate(&env);
    let terminal_owner = Address::generate(&env);
    let decimals = 10u32.pow(7) as i128;
    token_admin_client.mint(&sender, &(100 * decimals));

    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp: 1_000,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });

    let token_nft_id = router.create_lockup_stream(&CreateLockupParams {
        sender: sender.clone(),
        recipient: initial_owner.clone(),
        token: token_id.clone(),
        total_amount: 100 * decimals,
        start_time: 1_000,
        end_time: 1_100,
        cliff_time: 0,
        start_unlock_amount: 0,
        cliff_unlock_amount: 0,
        granularity: 1,
        cancelable: true,
    });
    let core_stream_id = router.core_stream_id(&token_nft_id);
    assert_eq!(token.balance(&lockup_id), 100 * decimals);

    // Transfer before a partial withdrawal moves only beneficiary rights.
    nft.transfer(&initial_owner, &partial_owner, &token_nft_id);
    env.ledger().set_timestamp(1_030);
    router.withdraw(
        &token_nft_id,
        &partial_owner,
        &partial_owner,
        &(10 * decimals),
    );

    // The original sender still cancels and receives only the unvested amount.
    env.ledger().set_timestamp(1_040);
    assert_eq!(lockup.cancel(&core_stream_id, &sender), 60 * decimals);
    assert_eq!(
        env.auths(),
        std::vec![(
            sender.clone(),
            invocation(
                &env,
                &lockup_id,
                "cancel",
                (&core_stream_id, &sender).into_val(&env),
                std::vec![],
            ),
        )]
    );
    assert_eq!(token.balance(&sender), 60 * decimals);
    assert_eq!(
        router.status_of(&token_nft_id),
        CanonicalStreamStatus::Canceled
    );

    // A post-cancellation transfer moves the remaining vested withdrawal right.
    nft.transfer(&partial_owner, &terminal_owner, &token_nft_id);
    assert_eq!(
        router.withdraw_max(&token_nft_id, &terminal_owner, &terminal_owner),
        30 * decimals
    );
    assert_eq!(token.balance(&terminal_owner), 30 * decimals);
    assert_eq!(
        router.status_of(&token_nft_id),
        CanonicalStreamStatus::Completed
    );
    assert_eq!(router.owner_of(&token_nft_id), terminal_owner);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #303)")]
fn test_non_owner_cannot_void_flow() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_id = sac.address();
    let token_admin_client = StellarAssetClient::new(&env, &token_id);
    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, _nft_id, router_id) = register_protocol(&env, &admin);
    let router = RouterContractClient::new(&env, &router_id);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let attacker = Address::generate(&env);
    token_admin_client.mint(&sender, &100);

    let token_nft_id = router.create_flow_stream(
        &sender,
        &recipient,
        &token_id,
        &1,
        &7,
        &env.ledger().timestamp(),
        &100,
    );
    router.void_flow(&token_nft_id, &attacker);
}

#[test]
fn test_upgrade() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, _nft_id, router_id) = register_protocol(&env, &admin);
    let router_client = RouterContractClient::new(&env, &router_id);

    // Use a valid WASM from our imports to test upgrading
    let new_wasm_hash = env.deployer().upload_contract_wasm(flow_client::WASM);
    router_client.upgrade(&new_wasm_hash);

    // Check that admin authorization was requested
    let auths = env.auths();
    assert!(!auths.is_empty());
    assert_eq!(auths[0].0, admin);
}

#[test]
fn test_upgrade_nft() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let (_flow_id, _lockup_id, _nft_id, router_id) = register_protocol(&env, &admin);
    let router_client = RouterContractClient::new(&env, &router_id);

    // Use a valid WASM from our imports to test upgrading
    let new_wasm_hash = env.deployer().upload_contract_wasm(flow_client::WASM);
    router_client.upgrade_nft(&new_wasm_hash);

    // Check that admin authorization was requested
    let auths = env.auths();
    assert!(!auths.is_empty());
    assert_eq!(auths[0].0, admin);
}
