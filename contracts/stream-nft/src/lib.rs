#![no_std]

use shared::errors::NftError;
use shared::events::emit_nft_transfer;
use shared::storage::{
    DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS,
    PERSISTENT_TTL_THRESHOLD,
};
use shared::types::StreamType;
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env, String, Symbol};

#[contract]
pub struct StreamNftContract;

#[contractimpl]
impl StreamNftContract {
    /// Initialize the NFT contract.
    pub fn initialize(env: Env, admin: Address, name: String, symbol: String) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, NftError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::TokenMetadata(Symbol::new(&env, "name")), &name);
        env.storage().instance().set(
            &DataKey::TokenMetadata(Symbol::new(&env, "symbol")),
            &symbol,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Admin can upgrade the contract logic.
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    /// Mint a new NFT representing a stream.
    /// Only the admin (Router) can mint.
    pub fn mint(env: Env, to: Address, stream_type: StreamType, stream_id: u64, token_id: i128) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, shared::errors::FlowError::NotInitialized)); // Reuse error or create general one

        admin.require_auth();

        let owner_key = DataKey::TokenOwner(token_id);
        if env.storage().persistent().has(&owner_key) {
            panic_with_error!(&env, NftError::AlreadyMinted);
        }

        // Set owner
        env.storage().persistent().set(&owner_key, &to);
        env.storage().persistent().extend_ttl(
            &owner_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        // Set stream data
        let data_key = DataKey::TokenStreamData(token_id);
        env.storage()
            .persistent()
            .set(&data_key, &(stream_type, stream_id));
        env.storage().persistent().extend_ttl(
            &data_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        // Update balance
        let balance_key = DataKey::NftBalance(to.clone());
        let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&balance_key, &(current_balance + 1));
        env.storage().persistent().extend_ttl(
            &balance_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        // Event (mint = from zero address)
        // Since we can't easily construct a zero address in Soroban without a byte array,
        // we'll just emit transfer from the admin/contract itself or skip the 'from' and emit special mint.
        // For simplicity, we just emit nft_transfer where from == admin.
        emit_nft_transfer(&env, &admin, &to, token_id);
    }

    /// Burn an NFT (e.g. when stream is depleted or voided).
    /// Only admin (Router) can burn.
    pub fn burn(env: Env, token_id: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let owner_key = DataKey::TokenOwner(token_id);
        let owner: Address = env
            .storage()
            .persistent()
            .get(&owner_key)
            .unwrap_or_else(|| panic_with_error!(&env, NftError::TokenNotFound));

        // Remove owner
        env.storage().persistent().remove(&owner_key);

        // Remove stream data
        let data_key = DataKey::TokenStreamData(token_id);
        env.storage().persistent().remove(&data_key);

        // Update balance
        let balance_key = DataKey::NftBalance(owner.clone());
        let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if current_balance > 0 {
            env.storage()
                .persistent()
                .set(&balance_key, &(current_balance - 1));
            env.storage().persistent().extend_ttl(
                &balance_key,
                PERSISTENT_TTL_THRESHOLD,
                PERSISTENT_TTL_LEDGERS,
            );
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        // Event
        emit_nft_transfer(&env, &owner, &admin, token_id);
    }

    /// Transfer an NFT to a new owner.
    pub fn transfer(env: Env, from: Address, to: Address, token_id: i128) {
        from.require_auth();

        let owner_key = DataKey::TokenOwner(token_id);
        let owner: Address = env
            .storage()
            .persistent()
            .get(&owner_key)
            .unwrap_or_else(|| panic_with_error!(&env, NftError::TokenNotFound));

        if owner != from {
            panic_with_error!(&env, NftError::NotAuthorized);
        }

        // Set new owner
        env.storage().persistent().set(&owner_key, &to);
        env.storage().persistent().extend_ttl(
            &owner_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        // Update balances
        let from_balance_key = DataKey::NftBalance(from.clone());
        let from_balance: i128 = env
            .storage()
            .persistent()
            .get(&from_balance_key)
            .unwrap_or(0);
        if from_balance > 0 {
            env.storage()
                .persistent()
                .set(&from_balance_key, &(from_balance - 1));
            env.storage().persistent().extend_ttl(
                &from_balance_key,
                PERSISTENT_TTL_THRESHOLD,
                PERSISTENT_TTL_LEDGERS,
            );
        }

        let to_balance_key = DataKey::NftBalance(to.clone());
        let to_balance: i128 = env.storage().persistent().get(&to_balance_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&to_balance_key, &(to_balance + 1));
        env.storage().persistent().extend_ttl(
            &to_balance_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        // Bump stream data TTL
        let data_key = DataKey::TokenStreamData(token_id);
        env.storage().persistent().extend_ttl(
            &data_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );

        emit_nft_transfer(&env, &from, &to, token_id);
    }

    /// Get the owner of an NFT.
    pub fn owner_of(env: Env, token_id: i128) -> Address {
        let owner_key = DataKey::TokenOwner(token_id);
        let owner = env
            .storage()
            .persistent()
            .get(&owner_key)
            .unwrap_or_else(|| panic_with_error!(&env, NftError::TokenNotFound));

        env.storage().persistent().extend_ttl(
            &owner_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );
        owner
    }

    /// Get the number of NFTs owned by an address.
    pub fn balance(env: Env, owner: Address) -> i128 {
        let balance_key = DataKey::NftBalance(owner);
        if let Some(balance) = env.storage().persistent().get::<_, i128>(&balance_key) {
            env.storage().persistent().extend_ttl(
                &balance_key,
                PERSISTENT_TTL_THRESHOLD,
                PERSISTENT_TTL_LEDGERS,
            );
            balance
        } else {
            0
        }
    }

    /// Get the stream details associated with an NFT.
    pub fn get_stream_data(env: Env, token_id: i128) -> (StreamType, u64) {
        let data_key = DataKey::TokenStreamData(token_id);
        let data = env
            .storage()
            .persistent()
            .get(&data_key)
            .unwrap_or_else(|| panic_with_error!(&env, NftError::TokenNotFound));

        env.storage().persistent().extend_ttl(
            &data_key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );
        data
    }

    /// Get token name
    pub fn name(env: Env) -> String {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
        env.storage()
            .instance()
            .get(&DataKey::TokenMetadata(Symbol::new(&env, "name")))
            .unwrap()
    }

    /// Get token symbol
    pub fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
        env.storage()
            .instance()
            .get(&DataKey::TokenMetadata(Symbol::new(&env, "symbol")))
            .unwrap()
    }
}

#[cfg(test)]
mod test;
