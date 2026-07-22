#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String};

fn create_contract(env: &Env) -> (Address, StreamNftContractClient) {
    let contract_id = env.register(StreamNftContract, ());
    let client = StreamNftContractClient::new(env, &contract_id);
    (contract_id, client)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);

    assert_eq!(client.name(), name);
    assert_eq!(client.symbol(), symbol);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #201)")]
fn test_initialize_twice_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);
    client.initialize(&admin, &name, &symbol);
}

#[test]
fn test_mint_and_burn() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);

    let user = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    assert_eq!(client.balance(&user), 0);

    // Mint
    client.mint(&user, &StreamType::Flow, &stream_id, &token_id);

    assert_eq!(client.balance(&user), 1);
    assert_eq!(client.owner_of(&token_id), user);
    assert_eq!(
        client.get_stream_data(&token_id),
        (StreamType::Flow, stream_id)
    );

    // Burn
    client.burn(&token_id);

    assert_eq!(client.balance(&user), 0);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #203)")]
fn test_burn_nonexistent_fails() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);
    client.burn(&1);
}

#[test]
fn test_transfer() {
    let env = Env::default();
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();

    let (_, client) = create_contract(&env);
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    client.mint(&user1, &StreamType::Flow, &stream_id, &token_id);
    assert_eq!(client.balance(&user1), 1);
    assert_eq!(client.balance(&user2), 0);
    assert_eq!(client.owner_of(&token_id), user1);

    client.transfer(&user1, &user2, &token_id);

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
    let admin = Address::generate(&env);

    let name = String::from_str(&env, "Fundable Stream NFT");
    let symbol = String::from_str(&env, "FSTRM");

    client.initialize(&admin, &name, &symbol);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let token_id = 1;
    let stream_id = 42;

    client.mint(&user1, &StreamType::Flow, &stream_id, &token_id);

    // user2 tries to transfer user1's token. (In mock_all_auths, the auth check passes,
    // but our logic enforces `owner == from` which fails if we pass user2 as from)
    client.transfer(&user2, &user2, &token_id);
}
