#![no_std]

use shared::errors::RouterError;
use shared::events;
use shared::storage::{DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD};
use shared::types::{
    CanonicalStreamStatus, CreateLockupParams, StreamMetadata, StreamType,
};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env};

mod flow_client {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/flow.wasm");
}

mod lockup_client {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/lockup.wasm");
}

mod nft_client {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/stream_nft.wasm");
}

#[contract]
pub struct RouterContract;

fn configured_address(env: &Env, key: &DataKey) -> Address {
    env.storage()
        .instance()
        .get(key)
        .unwrap_or_else(|| panic_with_error!(env, RouterError::NotInitialized))
}

fn shared_stream_type(stream_type: &nft_client::StreamType) -> StreamType {
    match stream_type {
        nft_client::StreamType::Flow => StreamType::Flow,
        nft_client::StreamType::Lockup => StreamType::Lockup,
    }
}

fn require_nft_owner(
    env: &Env,
    token_id: i128,
    caller: &Address,
) -> (nft_client::StreamType, u64) {
    let nft_addr = configured_address(env, &DataKey::NftContract);
    let nft = nft_client::Client::new(env, &nft_addr);
    if nft.owner_of(&token_id) != *caller {
        panic_with_error!(env, RouterError::NotAuthorized);
    }
    nft.get_stream_data(&token_id)
}

fn canonical_status(
    env: &Env,
    stream_type: &nft_client::StreamType,
    core_stream_id: u64,
) -> CanonicalStreamStatus {
    match stream_type {
        nft_client::StreamType::Flow => {
            let flow_addr = configured_address(env, &DataKey::FlowContract);
            let flow = flow_client::Client::new(env, &flow_addr);
            match flow.status_of(&core_stream_id) {
                flow_client::StreamStatus::Pending => CanonicalStreamStatus::Pending,
                flow_client::StreamStatus::StreamingSolvent
                | flow_client::StreamStatus::StreamingInsolvent => {
                    CanonicalStreamStatus::Active
                }
                flow_client::StreamStatus::PausedSolvent
                | flow_client::StreamStatus::PausedInsolvent => CanonicalStreamStatus::Paused,
                flow_client::StreamStatus::Voided => {
                    if flow.withdrawable_amount_of(&core_stream_id) == 0
                        && flow.refundable_amount_of(&core_stream_id) == 0
                    {
                        CanonicalStreamStatus::Completed
                    } else {
                        CanonicalStreamStatus::Canceled
                    }
                }
            }
        }
        nft_client::StreamType::Lockup => {
            let lockup_addr = configured_address(env, &DataKey::LockupContract);
            let lockup = lockup_client::Client::new(env, &lockup_addr);
            match lockup.status_of(&core_stream_id) {
                lockup_client::LockupStatus::Pending => CanonicalStreamStatus::Pending,
                lockup_client::LockupStatus::Streaming
                | lockup_client::LockupStatus::Settled => CanonicalStreamStatus::Active,
                lockup_client::LockupStatus::Canceled => CanonicalStreamStatus::Canceled,
                lockup_client::LockupStatus::Depleted => CanonicalStreamStatus::Completed,
            }
        }
    }
}

#[contractimpl]
impl RouterContract {
    /// Atomically initialize the Router admin during deployment.
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::NextStreamId, &1_i128);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Configure core contract addresses once after all contracts are deployed.
    pub fn configure(
        env: Env,
        flow_contract: Address,
        lockup_contract: Address,
        nft_contract: Address,
    ) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        if env.storage().instance().has(&DataKey::FlowContract)
            || env.storage().instance().has(&DataKey::LockupContract)
            || env.storage().instance().has(&DataKey::NftContract)
        {
            panic_with_error!(&env, RouterError::AlreadyConfigured);
        }

        env.storage()
            .instance()
            .set(&DataKey::FlowContract, &flow_contract);
        env.storage()
            .instance()
            .set(&DataKey::LockupContract, &lockup_contract);
        env.storage()
            .instance()
            .set(&DataKey::NftContract, &nft_contract);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Admin can upgrade the router logic.
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Admin can upgrade the NFT contract logic.
    pub fn upgrade_nft(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let nft_client = nft_client::Client::new(&env, &nft_addr);
        nft_client.upgrade(&new_wasm_hash);
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
        initial_amount: i128,
    ) -> i128 {
        sender.require_auth();
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::FlowContract)
            .unwrap();
        let nft_addr: Address = env.storage().instance().get(&DataKey::NftContract).unwrap();
        let router_addr = env.current_contract_address();

        let flow_client = flow_client::Client::new(&env, &flow_addr);
        let nft_client = nft_client::Client::new(&env, &nft_addr);

        // 1. Create stream on Flow contract (Router is the recipient)
        let stream_id = flow_client.create_and_deposit(
            &sender,
            &router_addr, // Router is recipient
            &token,
            &rate_per_second,
            &token_decimals,
            &start_time,
            &initial_amount,
        );

        // 2. Generate token ID
        let token_id: i128 = env
            .storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap();
        env.storage()
            .instance()
            .set(&DataKey::NextStreamId, &(token_id + 1));

        // 3. Mint NFT to actual recipient
        nft_client.mint(
            &recipient,
            &nft_client::StreamType::Flow,
            &stream_id,
            &token_id,
        );

        events::emit_stream_created(
            &env,
            token_id,
            stream_id,
            &StreamType::Flow,
            &sender,
            &recipient,
            &token,
        );

        token_id
    }

    /// Create a Lockup stream and mint an NFT.
    pub fn create_lockup_stream(env: Env, params: CreateLockupParams) -> i128 {
        params.sender.require_auth();
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let lockup_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::LockupContract)
            .unwrap();
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
        let token_id: i128 = env
            .storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap();
        env.storage()
            .instance()
            .set(&DataKey::NextStreamId, &(token_id + 1));

        // 3. Mint NFT to actual recipient
        nft_client.mint(
            &original_recipient,
            &nft_client::StreamType::Lockup,
            &stream_id,
            &token_id,
        );

        events::emit_stream_created(
            &env,
            token_id,
            stream_id,
            &StreamType::Lockup,
            &params.sender,
            &original_recipient,
            &params.token,
        );

        token_id
    }

    // --- Withdraw ---

    /// Withdraw tokens from a stream using the NFT.
    pub fn withdraw(env: Env, token_id: i128, caller: Address, to: Address, amount: i128) {
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr = configured_address(&env, &DataKey::FlowContract);
        let lockup_addr = configured_address(&env, &DataKey::LockupContract);
        let router_addr = env.current_contract_address();

        let (stream_type, stream_id) = require_nft_owner(&env, token_id, &caller);
        let public_stream_type = shared_stream_type(&stream_type);

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

        events::emit_stream_withdrawn(
            &env,
            token_id,
            stream_id,
            &public_stream_type,
            &caller,
            &to,
            amount,
        );
    }

    /// Withdraw max tokens from a stream using the NFT.
    pub fn withdraw_max(env: Env, token_id: i128, caller: Address, to: Address) -> i128 {
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let flow_addr = configured_address(&env, &DataKey::FlowContract);
        let lockup_addr = configured_address(&env, &DataKey::LockupContract);
        let router_addr = env.current_contract_address();

        let (stream_type, stream_id) = require_nft_owner(&env, token_id, &caller);
        let public_stream_type = shared_stream_type(&stream_type);

        // 3. Route withdrawal to proper contract
        let amount = if stream_type == nft_client::StreamType::Flow {
            let flow_client = flow_client::Client::new(&env, &flow_addr);
            flow_client.withdraw_max(&stream_id, &router_addr, &to)
        } else if stream_type == nft_client::StreamType::Lockup {
            let lockup_client = lockup_client::Client::new(&env, &lockup_addr);
            lockup_client.withdraw_max(&stream_id, &router_addr, &to)
        } else {
            panic_with_error!(&env, RouterError::InvalidStreamType);
        };

        if amount > 0 {
            events::emit_stream_withdrawn(
                &env,
                token_id,
                stream_id,
                &public_stream_type,
                &caller,
                &to,
                amount,
            );
        }
        amount
    }

    /// Permanently void a Flow stream as its current NFT owner.
    pub fn void_flow(env: Env, token_id: i128, caller: Address) {
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let (stream_type, stream_id) = require_nft_owner(&env, token_id, &caller);
        if stream_type != nft_client::StreamType::Flow {
            panic_with_error!(&env, RouterError::InvalidStreamType);
        }

        let flow_addr = configured_address(&env, &DataKey::FlowContract);
        let router_addr = env.current_contract_address();
        flow_client::Client::new(&env, &flow_addr).void_stream(&stream_id, &router_addr);
        events::emit_stream_voided(&env, token_id, stream_id, &caller);
    }

    // --- Stable Public Queries ---

    /// Return the current NFT owner for a public stream ID.
    pub fn owner_of(env: Env, token_id: i128) -> Address {
        let nft_addr = configured_address(&env, &DataKey::NftContract);
        nft_client::Client::new(&env, &nft_addr).owner_of(&token_id)
    }

    /// Return the core engine kind for a public stream ID.
    pub fn stream_type(env: Env, token_id: i128) -> StreamType {
        let nft_addr = configured_address(&env, &DataKey::NftContract);
        let (stream_type, _) =
            nft_client::Client::new(&env, &nft_addr).get_stream_data(&token_id);
        shared_stream_type(&stream_type)
    }

    /// Return the internal core engine ID for a public stream ID.
    pub fn core_stream_id(env: Env, token_id: i128) -> u64 {
        let nft_addr = configured_address(&env, &DataKey::NftContract);
        let (_, core_stream_id) =
            nft_client::Client::new(&env, &nft_addr).get_stream_data(&token_id);
        core_stream_id
    }

    /// Return the canonical cross-engine lifecycle for a public stream ID.
    pub fn status_of(env: Env, token_id: i128) -> CanonicalStreamStatus {
        let nft_addr = configured_address(&env, &DataKey::NftContract);
        let (stream_type, core_stream_id) =
            nft_client::Client::new(&env, &nft_addr).get_stream_data(&token_id);
        canonical_status(&env, &stream_type, core_stream_id)
    }

    /// Return stable public metadata and the canonical lifecycle in one query.
    pub fn get_stream(env: Env, token_id: i128) -> StreamMetadata {
        let nft_addr = configured_address(&env, &DataKey::NftContract);
        let nft = nft_client::Client::new(&env, &nft_addr);
        let owner = nft.owner_of(&token_id);
        let (stream_type, core_stream_id) = nft.get_stream_data(&token_id);
        StreamMetadata {
            token_id,
            owner,
            stream_type: shared_stream_type(&stream_type),
            core_stream_id,
            status: canonical_status(&env, &stream_type, core_stream_id),
            transferable: true,
        }
    }
}

#[cfg(test)]
mod test;
