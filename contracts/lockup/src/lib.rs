//! Fundable Lockup Contract — Fixed-term linear vesting with optional cliff.
//!
//! # Overview
//!
//! Lockup streams allow a sender to vest tokens to a recipient over a fixed
//! time period. The sender fully funds the stream at creation. Tokens unlock
//! linearly between cliff_time (or start_time if no cliff) and end_time,
//! with optional discrete unlock steps via the `granularity` parameter.
//!
//! # Key Properties
//!
//! - Pre-funded: all tokens are transferred to the contract at creation.
//! - Linear unlock with optional start and cliff unlock amounts.
//! - Granularity-based discrete steps (e.g. unlock every hour instead of per-second).
//! - Cancelable streams: sender can reclaim unvested tokens (if enabled).
//! - Renounce: sender can permanently make a stream non-cancelable.
//! - Admin + upgrade support from day one (SKILL.md §7).
//!
//! # Security
//!
//! - All privileged functions use `require_auth()` (SKILL.md §1).
//! - Checked arithmetic via workspace `overflow-checks = true` (SKILL.md §2).
//! - Events emitted on every state change (SKILL.md §8).
//! - TTL extended on every storage access (SKILL.md §3).

#![no_std]
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, BytesN, Env};

use shared::errors::LockupError;
use shared::types::{CreateLockupParams, LockupStatus, LockupStream};

mod internal;
mod queries;
mod storage;
mod test;

#[contract]
pub struct LockupContract;

/// Public API for the Fundable Lockup vesting contract.
///
/// Functions are organized into:
/// 1. **Admin** — initialize, upgrade, set_admin
/// 2. **Create** — create (with timestamps and optional cliff)
/// 3. **Mutate** — withdraw, cancel, renounce
/// 4. **Query** — get_stream, status_of, withdrawable_amount_of, etc.
#[contractimpl]
impl LockupContract {
    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Must be called exactly once before any other function.
    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic_with_error!(&env, LockupError::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        storage::extend_instance_ttl(&env);
    }

    /// Upgrade the contract WASM bytecode.
    ///
    /// Admin-only.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        storage::extend_instance_ttl(&env);
    }

    /// Transfer admin rights to a new address.
    ///
    /// Admin-only.
    pub fn set_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_admin(&env, &new_admin);
        storage::extend_instance_ttl(&env);
    }

    // -----------------------------------------------------------------------
    // Stream Creation
    // -----------------------------------------------------------------------

    /// Create a new Lockup stream.
    ///
    /// The sender must have approved the token transfer for `params.total_amount`.
    /// All tokens are transferred to the contract immediately.
    ///
    /// # Arguments
    /// * `params` — All creation parameters bundled into a `CreateLockupParams` struct.
    ///
    /// # Returns
    /// The newly assigned stream ID.
    pub fn create(env: Env, params: CreateLockupParams) -> u64 {
        params.sender.require_auth();
        storage::extend_instance_ttl(&env);

        internal::create(&env, &params)
    }

    // -----------------------------------------------------------------------
    // Stream Mutations
    // -----------------------------------------------------------------------

    /// Withdraw vested tokens from a stream.
    ///
    /// Only the stream recipient can withdraw.
    pub fn withdraw(env: Env, stream_id: u64, caller: Address, to: Address, amount: i128) {
        caller.require_auth();
        storage::extend_instance_ttl(&env);
        internal::withdraw(&env, stream_id, &caller, &to, amount);
    }

    /// Withdraw the maximum available amount from a stream.
    ///
    /// Convenience function — withdraws the entire withdrawable amount.
    pub fn withdraw_max(env: Env, stream_id: u64, caller: Address, to: Address) -> i128 {
        caller.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        let amount = internal::withdrawable_amount_of(&env, &stream);
        if amount > 0 {
            internal::withdraw(&env, stream_id, &caller, &to, amount);
        }
        amount
    }

    /// Cancel a stream and reclaim unvested tokens.
    ///
    /// Sender-only. The stream must be cancelable and not already
    /// depleted or canceled. Returns the refunded amount.
    pub fn cancel(env: Env, stream_id: u64, sender: Address) -> i128 {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, LockupError::Unauthorized);
        }

        internal::cancel(&env, stream_id)
    }

    /// Permanently renounce the ability to cancel a stream.
    ///
    /// Sender-only. Once renounced, the stream cannot be canceled.
    pub fn renounce(env: Env, stream_id: u64, sender: Address) {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, LockupError::Unauthorized);
        }

        internal::renounce(&env, stream_id);
    }

    // -----------------------------------------------------------------------
    // Read-Only Queries
    // -----------------------------------------------------------------------

    /// Get the full stream record.
    pub fn get_stream(env: Env, stream_id: u64) -> LockupStream {
        storage::extend_instance_ttl(&env);
        queries::require_stream(&env, stream_id)
    }

    /// Get the stream's current status.
    pub fn status_of(env: Env, stream_id: u64) -> LockupStatus {
        storage::extend_instance_ttl(&env);
        queries::status_of(&env, stream_id)
    }

    /// Get the amount withdrawable by the recipient.
    pub fn withdrawable_amount_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::withdrawable_amount_of(&env, stream_id)
    }

    /// Get the total vested ("streamed") amount at the current time.
    pub fn streamed_amount_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::streamed_amount_of(&env, stream_id)
    }

    /// Get the refundable amount if the stream were canceled now.
    pub fn refundable_amount_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::refundable_amount_of(&env, stream_id)
    }

    /// Check if the stream is cancelable.
    pub fn is_cancelable(env: Env, stream_id: u64) -> bool {
        let stream = queries::require_stream(&env, stream_id);
        stream.cancelable && !stream.is_depleted && !stream.was_canceled
    }

    /// Check if the stream is in a "cold" state.
    pub fn is_cold(env: Env, stream_id: u64) -> bool {
        storage::extend_instance_ttl(&env);
        queries::is_cold(&env, stream_id)
    }

    /// Check if the stream is in a "warm" state.
    pub fn is_warm(env: Env, stream_id: u64) -> bool {
        storage::extend_instance_ttl(&env);
        queries::is_warm(&env, stream_id)
    }

    /// Get the deposited amount.
    pub fn get_deposited_amount(env: Env, stream_id: u64) -> i128 {
        let stream = queries::require_stream(&env, stream_id);
        stream.total_amount
    }

    /// Get the withdrawn amount.
    pub fn get_withdrawn_amount(env: Env, stream_id: u64) -> i128 {
        let stream = queries::require_stream(&env, stream_id);
        stream.withdrawn_amount
    }

    /// Get the refunded amount.
    pub fn get_refunded_amount(env: Env, stream_id: u64) -> i128 {
        let stream = queries::require_stream(&env, stream_id);
        stream.refunded_amount
    }
}
