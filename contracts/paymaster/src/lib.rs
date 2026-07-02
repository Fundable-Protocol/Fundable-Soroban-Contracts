//! Fundable Paymaster (FeeForwarder) Contract
//!
//! Implements Soroban Gas Abstraction using the FeeForwarder pattern.
//! Users pay transaction fees in a supported token (e.g. USDC) instead of
//! native XLM. A relayer submits the transaction and is compensated
//! atomically from the user's token balance.
//!
//! ## Architecture
//!
//! The Paymaster sits in front of the existing contracts (Router, Flow,
//! Lockup) as a transparent wrapper. It:
//! 1. Collects a fee in a supported token from the user.
//! 2. Forwards the user's intended contract invocation to the target.
//! 3. Returns the result of the target invocation.
//!
//! No modifications are required to the existing contracts.

#![no_std]

use shared::errors::PaymasterError;
use shared::events::{emit_fee_collected, emit_invocation_forwarded};
use shared::storage::{DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD};
use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, Env, Symbol, Val, Vec,
};

#[contract]
pub struct PaymasterContract;

#[contractimpl]
impl PaymasterContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the Paymaster with an admin and an initial list of
    /// allowed fee tokens.
    ///
    /// # Arguments
    /// * `admin` — The admin address (can upgrade and manage token whitelist).
    /// * `allowed_fee_tokens` — Initial list of token addresses accepted
    ///   for fee payment.
    ///
    /// # Errors
    /// * `PaymasterError::AlreadyInitialized` — if called more than once.
    pub fn initialize(env: Env, admin: Address, allowed_fee_tokens: Vec<Address>) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, PaymasterError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::AllowedFeeTokens, &allowed_fee_tokens);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    // -----------------------------------------------------------------------
    // Core: Collect Fee & Invoke
    // -----------------------------------------------------------------------

    /// Atomically collect a token fee from the user and invoke the target
    /// contract function.
    ///
    /// The user must authorize both the fee transfer and the downstream
    /// contract invocation via Soroban's native auth framework.
    ///
    /// # Arguments
    /// * `user` — The end user paying the fee and authorizing the call.
    /// * `fee_token` — The SAC/SEP-41 token used for fee payment.
    /// * `max_fee` — The fee amount to transfer from user to relayer.
    /// * `relayer` — The relayer address receiving the fee.
    /// * `target_contract` — The contract to invoke (e.g. Router).
    /// * `function_name` — The function to call on the target contract.
    /// * `args` — Arguments to pass to the target function.
    ///
    /// # Returns
    /// The raw `Val` result from the target contract invocation.
    ///
    /// # Errors
    /// * `PaymasterError::NotInitialized` — contract not initialized.
    /// * `PaymasterError::TokenNotAllowed` — fee token not whitelisted.
    /// * `PaymasterError::FeeAmountZero` — max_fee is 0.
    /// * Any error from the token transfer or target contract is propagated.
    pub fn collect_fee_and_invoke(
        env: Env,
        user: Address,
        fee_token: Address,
        max_fee: i128,
        relayer: Address,
        target_contract: Address,
        function_name: Symbol,
        args: Vec<Val>,
    ) -> Val {
        // Require user authorization for the full operation
        user.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        // Validate initialization
        if !env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, PaymasterError::NotInitialized);
        }

        // Validate fee amount
        if max_fee <= 0 {
            panic_with_error!(&env, PaymasterError::FeeAmountZero);
        }

        // Validate token is allowed
        let allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedFeeTokens)
            .unwrap();

        if !Self::is_token_allowed(&env, &allowed_tokens, &fee_token) {
            panic_with_error!(&env, PaymasterError::TokenNotAllowed);
        }

        // Step 1: Collect fee from user → relayer
        let token_client = token::Client::new(&env, &fee_token);
        token_client.transfer(&user, &relayer, &max_fee);

        emit_fee_collected(&env, &user, &relayer, &fee_token, max_fee);

        // Step 2: Invoke the target contract
        let result: Val = env.invoke_contract(&target_contract, &function_name, args);

        emit_invocation_forwarded(&env, &user, &target_contract, &function_name);

        result
    }

    // -----------------------------------------------------------------------
    // Admin: Fee Token Management
    // -----------------------------------------------------------------------

    /// Add a token to the allowed fee token whitelist.
    ///
    /// # Auth
    /// Requires admin authorization.
    pub fn add_fee_token(env: Env, token: Address) {
        let admin: Address = Self::require_admin(&env);
        admin.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let mut allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedFeeTokens)
            .unwrap();

        // Don't add duplicates
        if !Self::is_token_allowed(&env, &allowed_tokens, &token) {
            allowed_tokens.push_back(token);
            env.storage()
                .instance()
                .set(&DataKey::AllowedFeeTokens, &allowed_tokens);
        }
    }

    /// Remove a token from the allowed fee token whitelist.
    ///
    /// # Auth
    /// Requires admin authorization.
    pub fn remove_fee_token(env: Env, token: Address) {
        let admin: Address = Self::require_admin(&env);
        admin.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        let allowed_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedFeeTokens)
            .unwrap();

        let mut new_tokens: Vec<Address> = Vec::new(&env);
        for t in allowed_tokens.iter() {
            if t != token {
                new_tokens.push_back(t);
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::AllowedFeeTokens, &new_tokens);
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Returns the list of currently accepted fee tokens.
    pub fn get_fee_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        env.storage()
            .instance()
            .get(&DataKey::AllowedFeeTokens)
            .unwrap_or(Vec::new(&env))
    }

    /// Returns the admin address.
    pub fn get_admin(env: Env) -> Address {
        Self::require_admin(&env)
    }

    // -----------------------------------------------------------------------
    // Admin: Upgrade
    // -----------------------------------------------------------------------

    /// Upgrade the contract WASM bytecode.
    ///
    /// # Auth
    /// Requires admin authorization.
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = Self::require_admin(&env);
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    // -----------------------------------------------------------------------
    // Internal Helpers
    // -----------------------------------------------------------------------

    /// Retrieve the admin address, panicking if not initialized.
    fn require_admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, PaymasterError::NotInitialized))
    }

    /// Check if a token is in the allowed list.
    fn is_token_allowed(env: &Env, allowed: &Vec<Address>, token: &Address) -> bool {
        for t in allowed.iter() {
            if &t == token {
                return true;
            }
        }
        let _ = env; // suppress unused warning
        false
    }
}

#[cfg(test)]
mod test;
