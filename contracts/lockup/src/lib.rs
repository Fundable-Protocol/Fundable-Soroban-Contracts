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
//! - Negative unlock amounts are rejected to prevent pooled-escrow drain.
//! - Two-step admin transfer prevents accidental transfer to unusable address.
//! - Timelocked upgrades provide mainnet safety.

#![no_std]
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, BytesN, Env};

use shared::errors::LockupError;
use shared::events;
use shared::storage::{DataKey, UPGRADE_TIMELOCK_LEDGERS};
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
/// 1. **Admin** — initialize, upgrade, set_admin, propose_admin, accept_admin
/// 2. **Create** — create (with timestamps and optional cliff)
/// 3. **Mutate** — withdraw, cancel, renounce
/// 4. **Query** — get_stream, status_of, withdrawable_amount_of, etc.
/// 5. **Keepalive** — extend_stream_ttl
#[contractimpl]
impl LockupContract {
    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Must be called exactly once before any other function.
    /// The intended admin must authorize the initialization.
    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic_with_error!(&env, LockupError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_initialized(&env, &admin);
    }

    /// Propose a timelocked upgrade.
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let unlock_ledger = env
            .ledger()
            .sequence()
            .checked_add(UPGRADE_TIMELOCK_LEDGERS)
            .unwrap_or_else(|| panic_with_error!(&env, LockupError::ArithmeticError));

        env.storage()
            .instance()
            .set(&DataKey::UpgradeUnlockLedger, &unlock_ledger);
        env.storage()
            .instance()
            .set(&DataKey::ProposedUpgrade(new_wasm_hash.clone()), &true);
        storage::extend_instance_ttl(&env);

        events::emit_upgrade_proposed(&env, &admin, &new_wasm_hash, unlock_ledger);
    }

    /// Execute a previously proposed upgrade after the timelock has expired.
    pub fn execute_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let proposed: bool = env
            .storage()
            .instance()
            .get(&DataKey::ProposedUpgrade(new_wasm_hash.clone()))
            .unwrap_or(false);
        if !proposed {
            panic_with_error!(&env, LockupError::NoUpgradeProposed);
        }

        let unlock_ledger: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeUnlockLedger)
            .unwrap_or_else(|| panic_with_error!(&env, LockupError::NoUpgradeProposed));

        if env.ledger().sequence() < unlock_ledger {
            panic_with_error!(&env, LockupError::UpgradeTimelocked);
        }

        env.storage()
            .instance()
            .remove(&DataKey::ProposedUpgrade(new_wasm_hash.clone()));
        env.storage()
            .instance()
            .remove(&DataKey::UpgradeUnlockLedger);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::extend_instance_ttl(&env);

        events::emit_upgrade_executed(&env, &admin, &new_wasm_hash);
    }

    /// Emergency upgrade — bypasses timelock.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::extend_instance_ttl(&env);
        events::emit_upgrade_executed(&env, &admin, &new_wasm_hash);
    }

    /// Propose a new admin (two-step transfer, step 1).
    pub fn propose_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_proposed(&env, &admin, &new_admin);
    }

    /// Accept an admin transfer (two-step transfer, step 2).
    pub fn accept_admin(env: Env) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, LockupError::NoAdminTransferPending));
        pending.require_auth();

        let old_admin = storage::get_admin(&env);
        storage::set_admin(&env, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_accepted(&env, &old_admin, &pending);
    }

    /// Direct admin transfer (legacy, kept for backwards compatibility).
    pub fn set_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_admin(&env, &new_admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_transferred(&env, &admin, &new_admin);
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
    // Keepalive
    // -----------------------------------------------------------------------

    /// Extend the TTL of a specific stream record.
    ///
    /// Anyone can call this to keep a long-duration stream alive.
    pub fn extend_stream_ttl(env: Env, stream_id: u64) {
        let _stream = queries::require_stream(&env, stream_id);
        storage::extend_instance_ttl(&env);
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
