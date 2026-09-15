//! Fundable Flow Contract — Open-ended rate-per-second token streaming.
//!
//! # Overview
//!
//! Flow streams allow a sender to continuously stream tokens to a recipient
//! at a configurable rate per second. The stream can be funded, paused,
//! restarted, and rate-adjusted at any time. The sender can refund excess
//! balance, and either party can permanently void the stream.
//!
//! - No embedded ERC-721 NFT — ownership tracked via `recipient` field.
//!   NFT receipts are handled by the separate `stream-nft` contract.
//! - No Comptroller/fee system — omitted for v1 simplicity.
//! - `i128` for all amounts (Soroban SDK convention).
//! - `u64` for timestamps (Soroban ledger timestamp type).
//! - Admin + upgrade support from day one (SKILL.md §7).
//!
//! # Security
//!
//! - All privileged functions use `require_auth()` (SKILL.md §1).
//! - Checked arithmetic via workspace `overflow-checks = true` (SKILL.md §2).
//! - 18-decimal internal precision for debt math (SKILL.md §2).
//! - Events emitted on every state change (SKILL.md §8).
//! - TTL extended on every storage access (SKILL.md §3).
//! - Two-step admin transfer prevents accidental transfer to unusable address.
//! - Timelocked upgrades provide mainnet safety with emergency override.

#![no_std]
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, BytesN, Env};

use shared::errors::FlowError;
use shared::events;
use shared::storage::{DataKey, UPGRADE_TIMELOCK_LEDGERS};
use shared::types::{FlowStream, StreamStatus};

mod internal;
mod queries;
mod storage;
mod test;

#[contract]
pub struct FlowContract;

/// Public API for the Fundable Flow streaming contract.
///
/// Functions are organized into:
/// 1. **Admin** — initialize, upgrade, set_admin, propose_admin, accept_admin
/// 2. **Create** — create, create_and_deposit
/// 3. **Mutate** — deposit, withdraw, pause, restart, adjust_rate, refund, void
/// 4. **Query** — get_stream, status_of, covered_debt_of, etc.
/// 5. **Keepalive** — extend_stream_ttl
#[contractimpl]
impl FlowContract {
    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Must be called exactly once before any other function.
    /// The intended admin must authorize the initialization to prevent
    /// first-caller takeover attacks.
    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic_with_error!(&env, FlowError::AlreadyInitialized);
        }
        // Require authorization from the intended admin BEFORE writing state.
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_initialized(&env, &admin);
    }

    /// Propose a timelocked upgrade. The upgrade can be executed after
    /// UPGRADE_TIMELOCK_LEDGERS have passed.
    ///
    /// Admin-only.
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let unlock_ledger = env
            .ledger()
            .sequence()
            .checked_add(UPGRADE_TIMELOCK_LEDGERS)
            .unwrap_or_else(|| panic_with_error!(&env, FlowError::ArithmeticError));

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
    ///
    /// Admin-only.
    pub fn execute_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        // Verify this hash was proposed
        let proposed: bool = env
            .storage()
            .instance()
            .get(&DataKey::ProposedUpgrade(new_wasm_hash.clone()))
            .unwrap_or(false);
        if !proposed {
            panic_with_error!(&env, FlowError::NoUpgradeProposed);
        }

        // Verify timelock has expired
        let unlock_ledger: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeUnlockLedger)
            .unwrap_or_else(|| panic_with_error!(&env, FlowError::NoUpgradeProposed));

        if env.ledger().sequence() < unlock_ledger {
            panic_with_error!(&env, FlowError::UpgradeTimelocked);
        }

        // Clean up proposal state
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

    /// Emergency upgrade — bypasses timelock. Use only in critical situations.
    /// Emits an upgrade event for auditing.
    ///
    /// Admin-only. Consider removing this function after initial mainnet
    /// stabilization, or requiring a separate emergency key.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::extend_instance_ttl(&env);
        events::emit_upgrade_executed(&env, &admin, &new_wasm_hash);
    }

    /// Propose a new admin (two-step transfer, step 1).
    ///
    /// The proposed admin must call `accept_admin()` to complete the transfer.
    /// Admin-only.
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
    ///
    /// Must be called by the address that was proposed via `propose_admin()`.
    pub fn accept_admin(env: Env) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, FlowError::NoAdminTransferPending));
        pending.require_auth();

        let old_admin = storage::get_admin(&env);
        storage::set_admin(&env, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_accepted(&env, &old_admin, &pending);
    }

    /// Direct admin transfer (legacy, kept for backwards compatibility).
    ///
    /// Admin-only. Prefer `propose_admin` + `accept_admin` for safety.
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

    /// Create a new Flow stream.
    ///
    /// The stream starts with zero balance. Use `deposit()` or
    /// `create_and_deposit()` to fund it.
    ///
    /// # Arguments
    /// * `sender` — Address streaming the tokens (can pause/adjust/refund).
    /// * `recipient` — Address receiving the tokens (can withdraw).
    /// * `token` — Soroban token contract address (SAC or SEP-41).
    /// * `rate_per_second` — Debt accrual rate in 18-decimal fixed-point.
    /// * `token_decimals` — Token's decimal count (≤ 18).
    /// * `start_time` — Unix timestamp to start. 0 = start now.
    ///
    /// # Returns
    /// The newly assigned stream ID.
    pub fn create(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        rate_per_second: i128,
        token_decimals: u32,
        start_time: u64,
    ) -> u64 {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        internal::create(
            &env,
            &sender,
            &recipient,
            &token,
            rate_per_second,
            token_decimals,
            start_time,
        )
    }

    /// Create a new Flow stream and immediately deposit tokens.
    ///
    /// Convenience function combining `create()` + `deposit()`.
    pub fn create_and_deposit(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        rate_per_second: i128,
        token_decimals: u32,
        start_time: u64,
        amount: i128,
    ) -> u64 {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream_id = internal::create(
            &env,
            &sender,
            &recipient,
            &token,
            rate_per_second,
            token_decimals,
            start_time,
        );

        internal::deposit(&env, stream_id, &sender, amount);

        stream_id
    }

    // -----------------------------------------------------------------------
    // Stream Mutations
    // -----------------------------------------------------------------------

    /// Deposit tokens into an existing stream.
    ///
    /// Anyone can fund a stream, but `funder.require_auth()` is needed
    /// for the token transfer authorization.
    pub fn deposit(env: Env, stream_id: u64, funder: Address, amount: i128) {
        funder.require_auth();
        storage::extend_instance_ttl(&env);
        internal::deposit(&env, stream_id, &funder, amount);
    }

    /// Withdraw accrued tokens from a stream.
    ///
    /// Only the stream recipient can withdraw. The withdrawn amount is
    /// capped at the covered debt (balance-backed portion of total debt).
    pub fn withdraw(env: Env, stream_id: u64, caller: Address, to: Address, amount: i128) {
        caller.require_auth();
        storage::extend_instance_ttl(&env);
        internal::withdraw(&env, stream_id, &caller, &to, amount);
    }

    /// Withdraw the maximum available amount from a stream.
    ///
    /// Convenience function — withdraws the entire covered debt.
    pub fn withdraw_max(env: Env, stream_id: u64, caller: Address, to: Address) -> i128 {
        caller.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        let amount = internal::covered_debt_of(&env, &stream);
        if amount > 0 {
            internal::withdraw(&env, stream_id, &caller, &to, amount);
        }
        amount
    }

    /// Pause an active stream.
    ///
    /// Sender-only. Snapshots ongoing debt and sets rate to 0.
    pub fn pause(env: Env, stream_id: u64, sender: Address) {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, FlowError::Unauthorized);
        }
        if stream.is_voided {
            panic_with_error!(&env, FlowError::StreamVoided);
        }
        if stream.rate_per_second == 0 {
            panic_with_error!(&env, FlowError::StreamPaused);
        }

        internal::pause(&env, stream_id);
    }

    /// Restart a paused stream with a new rate.
    ///
    /// Sender-only. The stream must be paused and not voided.
    pub fn restart(env: Env, stream_id: u64, sender: Address, rate_per_second: i128) {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, FlowError::Unauthorized);
        }

        internal::restart(&env, stream_id, &sender, rate_per_second);
    }

    /// Adjust the rate per second of an active stream.
    ///
    /// Sender-only. The stream must be actively streaming (not paused/voided).
    pub fn adjust_rate(env: Env, stream_id: u64, sender: Address, new_rate: i128) {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, FlowError::Unauthorized);
        }
        if stream.is_voided {
            panic_with_error!(&env, FlowError::StreamVoided);
        }
        if stream.rate_per_second == 0 {
            panic_with_error!(&env, FlowError::StreamPaused);
        }
        if new_rate <= 0 {
            panic_with_error!(&env, FlowError::RatePerSecondZero);
        }

        internal::adjust_rate(&env, stream_id, new_rate);
    }

    /// Refund excess balance from a stream back to the sender.
    ///
    /// Sender-only. Only unowed tokens can be refunded.
    pub fn refund(env: Env, stream_id: u64, sender: Address, amount: i128) {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, FlowError::Unauthorized);
        }

        internal::refund(&env, stream_id, amount);
    }

    /// Refund the maximum refundable amount.
    ///
    /// Sender-only. Returns the amount refunded.
    pub fn refund_max(env: Env, stream_id: u64, sender: Address) -> i128 {
        sender.require_auth();
        storage::extend_instance_ttl(&env);

        let stream = queries::require_stream(&env, stream_id);
        if sender != stream.sender {
            panic_with_error!(&env, FlowError::Unauthorized);
        }

        let amount = internal::refundable_amount_of(&env, &stream);
        if amount > 0 {
            internal::refund(&env, stream_id, amount);
        }
        amount
    }

    /// Permanently void a stream.
    ///
    /// Callable by sender OR recipient. Writes off uncovered debt and
    /// prevents the stream from being restarted.
    pub fn void_stream(env: Env, stream_id: u64, caller: Address) {
        caller.require_auth();
        storage::extend_instance_ttl(&env);
        internal::void_stream(&env, stream_id, &caller);
    }

    // -----------------------------------------------------------------------
    // Keepalive
    // -----------------------------------------------------------------------

    /// Extend the TTL of a specific stream record.
    ///
    /// Anyone can call this to keep a long-duration stream alive.
    /// Does not modify stream state.
    pub fn extend_stream_ttl(env: Env, stream_id: u64) {
        // Reading the stream via get_stream already extends its TTL.
        let _stream = queries::require_stream(&env, stream_id);
        storage::extend_instance_ttl(&env);
    }

    // -----------------------------------------------------------------------
    // Read-Only Queries
    // -----------------------------------------------------------------------

    /// Get the full stream record.
    pub fn get_stream(env: Env, stream_id: u64) -> FlowStream {
        storage::extend_instance_ttl(&env);
        queries::require_stream(&env, stream_id)
    }

    /// Get the stream's current balance.
    pub fn get_balance(env: Env, stream_id: u64) -> i128 {
        let stream = queries::require_stream(&env, stream_id);
        stream.balance
    }

    /// Get the stream's rate per second.
    pub fn get_rate_per_second(env: Env, stream_id: u64) -> i128 {
        let stream = queries::require_stream(&env, stream_id);
        stream.rate_per_second
    }

    /// Get the stream's current status.
    pub fn status_of(env: Env, stream_id: u64) -> StreamStatus {
        storage::extend_instance_ttl(&env);
        queries::status_of(&env, stream_id)
    }

    /// Get the amount withdrawable by the recipient.
    pub fn withdrawable_amount_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::withdrawable_amount_of(&env, stream_id)
    }

    /// Get the total debt owed (may exceed balance).
    pub fn total_debt_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::total_debt_of(&env, stream_id)
    }

    /// Get the covered debt (debt backed by balance).
    pub fn covered_debt_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::covered_debt_of(&env, stream_id)
    }

    /// Get the uncovered debt (debt exceeding balance).
    pub fn uncovered_debt_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::uncovered_debt_of(&env, stream_id)
    }

    /// Get the refundable amount (excess balance not owed).
    pub fn refundable_amount_of(env: Env, stream_id: u64) -> i128 {
        storage::extend_instance_ttl(&env);
        queries::refundable_amount_of(&env, stream_id)
    }

    /// Get the time at which the stream's balance will be depleted.
    pub fn depletion_time_of(env: Env, stream_id: u64) -> u64 {
        storage::extend_instance_ttl(&env);
        queries::depletion_time_of(&env, stream_id)
    }
}
