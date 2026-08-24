use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, String, Symbol,
};

fn create_contract(env: &Env) -> (Address, StreamNftContractClient<'_>) {
    let admin = Address::generate(env);
    let name = String::from_str(env, "Fundable Stream NFT");
    let symbol = String::from_str(env, "FSTRM");
    let contract_id = env.register(
        StreamNftContract,
        StreamNftContractArgs::__constructor(&admin, &name, &symbol),
    );
    let client = StreamNftContractClient::new(env, &contract_id);
    (contract_id, client)
}

#[test]
fn test_constructor_sets_metadata() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    assert_eq!(client.name(), name);
    assert_eq!(client.symbol(), symbol);
}

#[test]
fn test_mint_persists_receipt() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let user = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    assert_eq!(client.balance(&user), 0);

    // Mint
    client.mint(&user, &StreamType::Flow, &stream_id, &token_id, &true);

    assert_eq!(client.balance(&user), 1);
    assert_eq!(client.owner_of(&token_id), user);
    assert_eq!(
        client.get_stream_data(&token_id),
        (StreamType::Flow, stream_id)
    );

    assert!(client.is_transferable(&token_id));
    assert_eq!(client.balance(&user), 1);
}

#[test]
fn test_transfer() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (contract_id, client) = create_contract(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    client.mint(&user1, &StreamType::Flow, &stream_id, &token_id, &true);
    assert_eq!(client.balance(&user1), 1);
    assert_eq!(client.balance(&user2), 0);
    assert_eq!(client.owner_of(&token_id), user1);

    client.transfer(&user1, &user2, &token_id);

    assert_eq!(
        env.events().all(),
        soroban_sdk::vec![
            &env,
            (
                contract_id,
                (Symbol::new(&env, "transfer"), token_id).into_val(&env),
                (
                    user1.clone(),
                    user2.clone(),
                    StreamType::Flow,
                    stream_id,
                    true,
                )
                    .into_val(&env),
            )
        ]
    );

    assert_eq!(client.balance(&user1), 0);
    assert_eq!(client.balance(&user2), 1);
    assert_eq!(client.owner_of(&token_id), user2);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #202)")]
fn test_transfer_unauthorized_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    client.mint(&user1, &StreamType::Flow, &stream_id, &token_id, &true);

    // user2 tries to transfer user1's token. (In mock_all_auths, the auth check passes,
    // but our logic enforces `owner == from` which fails if we pass user2 as from)
    client.transfer(&user2, &user2, &token_id);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #204)")]
fn test_non_transferable_stream_rejects_transfer() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token_id = 1;

    client.mint(&owner, &StreamType::Lockup, &42, &token_id, &false);
    assert!(!client.is_transferable(&token_id));
    client.transfer(&owner, &new_owner, &token_id);
}
