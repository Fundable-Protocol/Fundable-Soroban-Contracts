#![no_std]

use shared::errors::RouterError;
use shared::storage::{
    DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD
};
use shared::types::{CreateLockupParams};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env};

mod flow_client {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/flow.wasm"
    );
}

mod lockup_client {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/lockup.wasm"
    );
}

mod nft_client {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/stream_nft.wasm"
    );
}

#[contract]
pub struct RouterContract;

#[contractimpl]
impl RouterContract {
    /// Initialize the Router with the addresses of the core contracts.
    pub fn initialize(
        env: Env,
        admin: Address,
        flow_contract: Address,
        lockup_contract: Address,
        nft_contract: Address,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, RouterError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FlowContract, &flow_contract);
        env.storage().instance().set(&DataKey::LockupContract, &lockup_contract);
        env.storage().instance().set(&DataKey::NftContract, &nft_contract);
        env.storage().instance().set(&DataKey::NextStreamId, &1_i128);

        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Admin can upgrade the router logic.
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    // --- Create Streams ---

    /// Create a Flow stream and mint an NFT.
    pub fn create_flow_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        rate_per_second: i128,
        token_decimals: u32,
        start_time: u64,
    ) -> i128 {
        sender.require_auth();
        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr: Address = env.storage().instance().get(&DataKey::FlowContract).unwrap();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let router_addr = env.current_contract_address();

        let flow_client = flow_client::Client::new(&env, &flow_addr);
        let nft_client = nft_client::Client::new(&env, &nft_addr);

        // 1. Create stream on Flow contract (Router is the recipient)
        let stream_id = flow_client.create(
            &sender,
            &router_addr, // Router is recipient
            &token,
            &rate_per_second,
            &token_decimals,
            &start_time,
        );

        // 2. Generate token ID
        let token_id: i128 = env.storage().instance().get(&DataKey::NextStreamId).unwrap();
        env.storage().instance().set(&DataKey::NextStreamId, &(token_id + 1));

        // 3. Mint NFT to actual recipient
        nft_client.mint(&recipient, &nft_client::StreamType::Flow, &stream_id, &token_id);

        token_id
    }

    /// Create a Lockup stream and mint an NFT.
    pub fn create_lockup_stream(
        env: Env,
        params: CreateLockupParams,
    ) -> i128 {
        params.sender.require_auth();
        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let lockup_addr: Address = env.storage().instance().get(&DataKey::LockupContract).unwrap();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let router_addr = env.current_contract_address();
        let original_recipient = params.recipient.clone();

        let lockup_client = lockup_client::Client::new(&env, &lockup_addr);
        let nft_client = nft_client::Client::new(&env, &nft_addr);

        // Map to lockup_client's type
        let lockup_params = lockup_client::CreateLockupParams {
            sender: params.sender.clone(),
            recipient: router_addr,
            token: params.token.clone(),
            total_amount: params.total_amount,
            start_time: params.start_time,
            end_time: params.end_time,
            cliff_time: params.cliff_time,
            start_unlock_amount: params.start_unlock_amount,
            cliff_unlock_amount: params.cliff_unlock_amount,
            granularity: params.granularity,
            cancelable: params.cancelable,
        };

        // 1. Create stream on Lockup contract
        let stream_id = lockup_client.create(&lockup_params);

        // 2. Generate token ID
        let token_id: i128 = env.storage().instance().get(&DataKey::NextStreamId).unwrap();
        env.storage().instance().set(&DataKey::NextStreamId, &(token_id + 1));

        // 3. Mint NFT to actual recipient
        nft_client.mint(&original_recipient, &nft_client::StreamType::Lockup, &stream_id, &token_id);

        token_id
    }

    // --- Withdraw ---

    /// Withdraw tokens from a stream using the NFT.
    pub fn withdraw(env: Env, token_id: i128, caller: Address, to: Address, amount: i128) {
        caller.require_auth();
        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr: Address = env.storage().instance().get(&DataKey::FlowContract).unwrap();
        let lockup_addr: Address = env.storage().instance().get(&DataKey::LockupContract).unwrap();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let router_addr = env.current_contract_address();

        let nft_client = nft_client::Client::new(&env, &nft_addr);

        // 1. Verify caller owns the NFT
        let owner = nft_client.owner_of(&token_id);
        if owner != caller {
            panic_with_error!(&env, RouterError::NotAuthorized);
        }

        // 2. Get stream data
        let (stream_type, stream_id) = nft_client.get_stream_data(&token_id);

        // 3. Route withdrawal to proper contract
        if stream_type == nft_client::StreamType::Flow {
            let flow_client = flow_client::Client::new(&env, &flow_addr);
            // Router is the recipient on the flow contract, so Router must be the caller parameter
            flow_client.withdraw(&stream_id, &router_addr, &to, &amount);
        } else if stream_type == nft_client::StreamType::Lockup {
            let lockup_client = lockup_client::Client::new(&env, &lockup_addr);
            lockup_client.withdraw(&stream_id, &router_addr, &to, &amount);
        } else {
            panic_with_error!(&env, RouterError::InvalidStreamType);
        }
    }

    /// Withdraw max tokens from a stream using the NFT.
    pub fn withdraw_max(env: Env, token_id: i128, caller: Address, to: Address) -> i128 {
        caller.require_auth();
        env.storage().instance().extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr: Address = env.storage().instance().get(&DataKey::FlowContract).unwrap();
        let lockup_addr: Address = env.storage().instance().get(&DataKey::LockupContract).unwrap();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let router_addr = env.current_contract_address();

        let nft_client = nft_client::Client::new(&env, &nft_addr);

        // 1. Verify caller owns the NFT
        let owner = nft_client.owner_of(&token_id);
        if owner != caller {
            panic_with_error!(&env, RouterError::NotAuthorized);
        }

        // 2. Get stream data
        let (stream_type, stream_id) = nft_client.get_stream_data(&token_id);

        // 3. Route withdrawal to proper contract
        if stream_type == nft_client::StreamType::Flow {
            let flow_client = flow_client::Client::new(&env, &flow_addr);
            flow_client.withdraw_max(&stream_id, &router_addr, &to)
        } else if stream_type == nft_client::StreamType::Lockup {
            let lockup_client = lockup_client::Client::new(&env, &lockup_addr);
            lockup_client.withdraw_max(&stream_id, &router_addr, &to)
        } else {
            panic_with_error!(&env, RouterError::InvalidStreamType);
        }
    }
}

#[cfg(test)]
mod test;
