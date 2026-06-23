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

#![no_std]
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, BytesN, Env};

use shared::errors::FlowError;
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
/// 1. **Admin** — initialize, upgrade, set_admin
/// 2. **Create** — create, create_and_deposit
/// 3. **Mutate** — deposit, withdraw, pause, restart, adjust_rate, refund, void
/// 4. **Query** — get_stream, status_of, covered_debt_of, etc.
#[contractimpl]
impl FlowContract {
    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Must be called exactly once before any other function.
    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic_with_error!(&env, FlowError::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        storage::extend_instance_ttl(&env);
    }

    /// Upgrade the contract WASM bytecode.
    ///
    /// Admin-only. Per SKILL.md §7: admin-gated upgrades with event emission.
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
        if stream.rate_per_second == 0 {
            panic_with_error!(&env, FlowError::StreamPaused);
        }
        if stream.is_voided {
            panic_with_error!(&env, FlowError::StreamVoided);
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
