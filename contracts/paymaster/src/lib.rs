//! Fundable Paymaster (FeeForwarder) Contract
//!
//! Implements Soroban Gas Abstraction using the OpenZeppelin
//! `stellar-fee-abstraction` crate. Users pay transaction fees in a
//! supported token (e.g. USDC) instead of native XLM. A relayer submits
//! the transaction and is compensated atomically from the user's token
//! balance.
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
//!
//! ## OZ Integration
//!
//! This contract uses the official `stellar-fee-abstraction` crate from
//! OpenZeppelin which provides battle-tested primitives for:
//! - `collect_fee_and_invoke` — atomic fee collection + target invocation
//! - `set_allowed_fee_token` / `is_allowed_fee_token` — token allowlist
//! - `sweep_token` — admin fee sweeping
//! - `validate_fee_bounds` / `validate_expiration_ledger` — input validation

#![no_std]

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PaymasterError {
    AlreadyInitialized = 401,
    NotInitialized = 402,
    TokenNotAllowed = 404,
}
use shared::storage::{DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD};
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Env, Symbol, Val, Vec};
use stellar_fee_abstraction::{
    collect_fee_and_invoke, is_allowed_fee_token, set_allowed_fee_token, sweep_token,
    FeeAbstractionApproval,
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

        // Register each token in the OZ fee-abstraction allowlist
        for token in allowed_fee_tokens.iter() {
            set_allowed_fee_token(&env, &token, true);
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }

    // -----------------------------------------------------------------------
    // Core: Forward (collect fee and invoke target)
    // -----------------------------------------------------------------------

    /// Atomically collect a token fee from the user and invoke the target
    /// contract function, using the OpenZeppelin fee abstraction helpers.
    ///
    /// The user must authorize both the fee transfer and the downstream
    /// contract invocation via Soroban's native auth framework.
    ///
    /// # Arguments
    /// * `user` — The end user paying the fee and authorizing the call.
    /// * `fee_token` — The SAC/SEP-41 token used for fee payment.
    /// * `fee_amount` — The actual fee amount to transfer from user.
    /// * `max_fee_amount` — The maximum fee the user authorized.
    /// * `expiration_ledger` — The ledger at which the approval expires.
    /// * `fee_recipient` — The address receiving the fee (typically the relayer).
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
    /// * Any error from the OZ fee-abstraction helpers is propagated.
    pub fn forward(
        env: Env,
        user: Address,
        fee_token: Address,
        fee_amount: i128,
        max_fee_amount: i128,
        expiration_ledger: u32,
        fee_recipient: Address,
        target_contract: Address,
        function_name: Symbol,
        args: Vec<Val>,
    ) -> Val {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        // Validate initialization
        if !env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(&env, PaymasterError::NotInitialized);
        }

        // Validate token is on the allowlist
        if !is_allowed_fee_token(&env, &fee_token) {
            panic_with_error!(&env, PaymasterError::TokenNotAllowed);
        }

        // Delegate to OZ's atomic collect-fee-and-invoke
        collect_fee_and_invoke(
            &env,
            &fee_token,
            fee_amount,
            max_fee_amount,
            expiration_ledger,
            &target_contract,
            &function_name,
            &args,
            &user,
            &fee_recipient,
            FeeAbstractionApproval::Eager,
        )
    }

    // -----------------------------------------------------------------------
    // Backwards-compatible wrapper (matches original API)
    // -----------------------------------------------------------------------

    /// Backwards-compatible entry point matching the original Paymaster API.
    ///
    /// This wraps `forward()` with a simplified interface where the relayer
    /// address is the fee recipient and fee_amount == max_fee.
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
        // Use max ledger for expiration (backwards compat - no expiration)
        let expiration_ledger = env.ledger().sequence() + 1000;

        Self::forward(
            env,
            user,
            fee_token,
            max_fee, // fee_amount == max_fee
            max_fee, // max_fee_amount
            expiration_ledger,
            relayer, // fee_recipient = relayer
            target_contract,
            function_name,
            args,
        )
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

        set_allowed_fee_token(&env, &token, true);
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

        set_allowed_fee_token(&env, &token, false);
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Check if a specific token is in the allowed fee token list.
    pub fn is_fee_token_allowed(env: Env, token: Address) -> bool {
        is_allowed_fee_token(&env, &token)
    }

    /// Returns the admin address.
    pub fn get_admin(env: Env) -> Address {
        Self::require_admin(&env)
    }

    // -----------------------------------------------------------------------
    // Admin: Sweep & Upgrade
    // -----------------------------------------------------------------------

    /// Sweep accumulated fee tokens to a recipient address.
    ///
    /// # Auth
    /// Requires admin authorization.
    pub fn sweep(env: Env, token: Address, to: Address) {
        let admin: Address = Self::require_admin(&env);
        admin.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);

        sweep_token(&env, &token, &to);
    }

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
}

#[cfg(test)]
mod test;
