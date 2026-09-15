//! Internal (private) implementation functions for the Lockup contract.
//!
//! These functions contain the core business logic for fixed-term vesting
//! streams with linear unlock. They are called by the public API in `lib.rs`
//! after authorization checks.
//!
//! # Architecture Notes
//!
//! - **Pre-funded**: Unlike Flow, Lockup streams are fully funded at creation.
//!   The total amount is transferred from the sender when the stream is created.
//!
//! - **Linear unlock with cliff**: Tokens vest linearly between cliff_time
//!   (or start_time if no cliff) and end_time, in discrete `granularity`
//!   steps.
//!
//! - **Cancel**: The sender can cancel if `cancelable` is true. Unvested
//!   tokens return to sender; vested tokens remain for the recipient.
//!
//! - **Renounce**: The sender can permanently make a stream non-cancelable.
//!
//! # Security Invariants
//!
//! - `streamed_amount_of` always returns a value in `[0, total_amount]`.
//! - Cancellation `sender_amount` is always in `[0, total_amount - withdrawn_amount]`.
//! - Cancellation `recipient_amount` is always `>= 0`.
//! - Aggregate accounting is never reduced by more than the stream's remaining balance.
//! - All arithmetic on user-influenced values uses checked operations.

use crate::storage;
use shared::errors::LockupError;
use shared::events;
use shared::types::{CreateLockupParams, LockupStream};
use soroban_sdk::{panic_with_error, token, Address, Env};

// ---------------------------------------------------------------------------
// Read-only vesting calculations
// ---------------------------------------------------------------------------

/// Calculate the total vested ("streamed") amount at the current time.
///
/// Uses the linear formula with discrete unlock steps:
///
/// ```text
/// if now < start_time:       vested = 0
/// if now < cliff_time:       vested = start_unlock_amount
/// if now >= end_time:         vested = total_amount
/// else:
///   elapsed = floor((now - cliff_time) / granularity) * granularity
///   streamable_duration = end_time - cliff_time
///   streamable_amount = total_amount - start_unlock_amount - cliff_unlock_amount
///   vested = start_unlock_amount + cliff_unlock_amount + (elapsed * streamable_amount / streamable_duration)
/// ```
///
/// # Security
///
/// The result is always clamped to `[0, total_amount]` regardless of stored
/// field values. This prevents negative streamed amounts from corrupting
/// cancellation calculations even if a stream was stored with invalid data
/// before the creation validation was hardened.
pub fn streamed_amount_of(env: &Env, stream: &LockupStream) -> i128 {
    // If depleted, the streamed amount is the withdrawn amount (no more to stream).
    if stream.is_depleted {
        // Clamp: withdrawn_amount should be <= total_amount by invariant,
        // but defend in depth.
        return stream.withdrawn_amount.max(0).min(stream.total_amount);
    }

    // If canceled, the streamed amount is total minus refunded.
    if stream.was_canceled {
        let val = stream
            .total_amount
            .checked_sub(stream.refunded_amount)
            .unwrap_or(0);
        return val.max(0).min(stream.total_amount);
    }

    let now = env.ledger().timestamp();

    // Before start: nothing vested.
    if now < stream.start_time {
        return 0;
    }

    // After end: everything vested.
    if now >= stream.end_time {
        return stream.total_amount;
    }

    // Before cliff (if cliff is set): only start_unlock_amount.
    if stream.cliff_time > 0 && now < stream.cliff_time {
        // Clamp: start_unlock_amount was validated >= 0 at creation,
        // but defend in depth.
        return stream.start_unlock_amount.max(0).min(stream.total_amount);
    }

    // Between cliff and end: linear interpolation with discrete steps.
    let unlock_amounts_sum = stream
        .start_unlock_amount
        .checked_add(stream.cliff_unlock_amount)
        .unwrap_or(stream.total_amount);

    // Safety: if unlock amounts >= total, everything is unlocked.
    if unlock_amounts_sum >= stream.total_amount {
        return stream.total_amount;
    }

    // Clamp unlock_amounts_sum to [0, total_amount] for safety.
    let safe_unlock_sum = unlock_amounts_sum.max(0).min(stream.total_amount);

    // Determine the reference point for elapsed time calculation.
    let reference_time = if stream.cliff_time > 0 {
        stream.cliff_time
    } else {
        stream.start_time
    };

    let streamable_duration = (stream.end_time - reference_time) as i128;
    if streamable_duration <= 0 {
        return stream.total_amount;
    }

    let streamable_amount = stream.total_amount - safe_unlock_sum;
    if streamable_amount <= 0 {
        return stream.total_amount;
    }

    // Calculate elapsed time in granularity units (discrete steps).
    let raw_elapsed = (now - reference_time) as i128;
    let granularity = stream.granularity as i128;
    let elapsed_in_granularity_units = raw_elapsed / granularity;
    let discrete_elapsed = elapsed_in_granularity_units
        .checked_mul(granularity)
        .unwrap_or(raw_elapsed);

    // streamed_portion = discrete_elapsed * streamable_amount / streamable_duration
    let streamed_portion = match discrete_elapsed.checked_mul(streamable_amount) {
        Some(product) => product / streamable_duration,
        None => {
            // On overflow, treat as fully vested (conservative for recipient).
            return stream.total_amount;
        }
    };

    let vested = safe_unlock_sum
        .checked_add(streamed_portion)
        .unwrap_or(stream.total_amount);

    // Final clamp to [0, total_amount].
    vested.max(0).min(stream.total_amount)
}

/// Calculate the withdrawable amount (vested - already withdrawn).
pub fn withdrawable_amount_of(env: &Env, stream: &LockupStream) -> i128 {
    let streamed = streamed_amount_of(env, stream);
    if streamed > stream.withdrawn_amount {
        streamed - stream.withdrawn_amount
    } else {
        0
    }
}

/// Calculate the refundable amount (total - vested).
///
/// Returns 0 if the stream is not cancelable or is already canceled/depleted.
pub fn refundable_amount_of(env: &Env, stream: &LockupStream) -> i128 {
    if !stream.cancelable || stream.is_depleted || stream.was_canceled {
        return 0;
    }
    let streamed = streamed_amount_of(env, stream);
    // streamed is clamped to [0, total_amount], so this is always >= 0.
    stream.total_amount - streamed
}

// ---------------------------------------------------------------------------
// State-changing internal functions
// ---------------------------------------------------------------------------

/// Create a new Lockup stream.
///
/// Validates inputs, transfers tokens from sender to the contract, stores the
/// stream record, and emits the creation event.
///
/// # Security Validations
///
/// - `total_amount > 0`
/// - `start_unlock_amount >= 0`
/// - `cliff_unlock_amount >= 0`
/// - `start_unlock_amount + cliff_unlock_amount` uses checked_add
/// - `0 <= unlock_sum <= total_amount`
pub fn create(env: &Env, params: &CreateLockupParams) -> u64 {
    // Validate: sender and recipient must differ (H-1)
    if params.sender == params.recipient {
        panic_with_error!(env, LockupError::SenderEqualsRecipient);
    }

    // Validate: total amount > 0
    if params.total_amount <= 0 {
        panic_with_error!(env, LockupError::AmountZero);
    }

    // Validate: end_time > start_time
    if params.end_time <= params.start_time {
        panic_with_error!(env, LockupError::InvalidTimeRange);
    }

    // Validate: if cliff is set, it must be between start and end
    if params.cliff_time > 0
        && (params.cliff_time <= params.start_time || params.cliff_time >= params.end_time)
    {
        panic_with_error!(env, LockupError::InvalidTimeRange);
    }

    // CRITICAL: Validate unlock amounts are non-negative.
    // Without this, negative values produce negative streamed amounts during
    // cancellation, allowing sender_amount to exceed the stream's deposit
    // and drain funds from other streams sharing the same token.
    if params.start_unlock_amount < 0 {
        panic_with_error!(env, LockupError::NegativeUnlockAmount);
    }
    if params.cliff_unlock_amount < 0 {
        panic_with_error!(env, LockupError::NegativeUnlockAmount);
    }

    // Use checked addition to prevent signed-addition overflow.
    let unlock_sum = params
        .start_unlock_amount
        .checked_add(params.cliff_unlock_amount)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::UnlockSumOverflow));

    // Validate: 0 <= unlock_sum <= total_amount
    if unlock_sum < 0 || unlock_sum > params.total_amount {
        panic_with_error!(env, LockupError::InvalidUnlockSum);
    }

    // Validate: granularity must be > 0, default to 1
    let effective_granularity = if params.granularity == 0 {
        1
    } else {
        params.granularity
    };

    // Allocate stream ID (checked increment)
    let stream_id = storage::get_next_stream_id(env);
    let next_id = stream_id
        .checked_add(1)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));
    storage::set_next_stream_id(env, next_id);

    // Build and store the stream
    let stream = LockupStream {
        sender: params.sender.clone(),
        recipient: params.recipient.clone(),
        token: params.token.clone(),
        total_amount: params.total_amount,
        withdrawn_amount: 0,
        refunded_amount: 0,
        start_time: params.start_time,
        end_time: params.end_time,
        cliff_time: params.cliff_time,
        start_unlock_amount: params.start_unlock_amount,
        cliff_unlock_amount: params.cliff_unlock_amount,
        granularity: effective_granularity,
        cancelable: params.cancelable,
        was_canceled: false,
        is_depleted: false,
    };
    storage::set_stream(env, stream_id, &stream);

    // Update aggregate balance (before external call — CEI pattern)
    let agg = storage::get_aggregate_balance(env, &params.token);
    storage::set_aggregate_balance(
        env,
        &params.token,
        agg.checked_add(params.total_amount)
            .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError)),
    );

    // Exact-transfer validation: check balance before and after transfer.
    let token_client = token::Client::new(env, &params.token);
    let contract_addr = env.current_contract_address();
    let balance_before = token_client.balance(&contract_addr);

    // Transfer tokens from sender into the contract (fully pre-funded)
    token_client.transfer(&params.sender, &contract_addr, &params.total_amount);

    let balance_after = token_client.balance(&contract_addr);
    let received = balance_after
        .checked_sub(balance_before)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));
    if received != params.total_amount {
        panic_with_error!(env, LockupError::TokenTransferMismatch);
    }

    // Emit event
    events::emit_lockup_created(
        env,
        stream_id,
        &params.sender,
        &params.recipient,
        &params.token,
        params.total_amount,
        params.start_time,
        params.end_time,
        params.cliff_time,
        params.cancelable,
    );

    stream_id
}

/// Withdraw vested tokens from a Lockup stream.
///
/// Only the recipient can withdraw. The amount is capped at the
/// withdrawable amount (vested - already withdrawn).
pub fn withdraw(env: &Env, stream_id: u64, caller: &Address, to: &Address, amount: i128) {
    if amount <= 0 {
        panic_with_error!(env, LockupError::Overdraw);
    }

    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::StreamNotFound));

    // Only recipient can withdraw
    if *caller != stream.recipient {
        panic_with_error!(env, LockupError::Unauthorized);
    }

    // Stream must not be depleted
    if stream.is_depleted {
        panic_with_error!(env, LockupError::AlreadyCancelled);
    }

    // Check amount doesn't exceed withdrawable
    let withdrawable = withdrawable_amount_of(env, &stream);
    if amount > withdrawable {
        panic_with_error!(env, LockupError::Overdraw);
    }

    // Update withdrawn amount (checked)
    stream.withdrawn_amount = stream
        .withdrawn_amount
        .checked_add(amount)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));

    // Check if stream is now depleted
    // Using >= for safety — if withdrawn + refunded >= total, mark depleted
    let remaining = stream
        .total_amount
        .checked_sub(stream.refunded_amount)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));
    if stream.withdrawn_amount >= remaining {
        stream.is_depleted = true;
        stream.cancelable = false;
    }

    let token_addr = stream.token.clone();
    storage::set_stream(env, stream_id, &stream);

    // Update aggregate balance (checked)
    let agg = storage::get_aggregate_balance(env, &token_addr);
    storage::set_aggregate_balance(
        env,
        &token_addr,
        agg.checked_sub(amount)
            .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError)),
    );

    // Transfer tokens to recipient
    let token_client = token::Client::new(env, &token_addr);
    token_client.transfer(&env.current_contract_address(), to, &amount);

    events::emit_lockup_withdraw(env, stream_id, to, caller, amount);
}

/// Cancel a Lockup stream.
///
/// Only the sender can cancel. The stream must be cancelable and not yet
/// depleted or already canceled. Unvested tokens are returned to the sender.
///
/// # Security
///
/// - `streamed` is clamped to `[0, total_amount]` by `streamed_amount_of`.
/// - `sender_amount` is capped at `total_amount - withdrawn_amount`.
/// - `recipient_amount` is validated `>= 0`.
/// - Aggregate reduction is bounded by the stream's remaining accounted balance.
/// - All arithmetic uses checked operations with explicit contract errors.
pub fn cancel(env: &Env, stream_id: u64) -> i128 {
    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::StreamNotFound));

    // Must not be already canceled
    if stream.was_canceled {
        panic_with_error!(env, LockupError::AlreadyCancelled);
    }

    // Must be cancelable
    if !stream.cancelable {
        panic_with_error!(env, LockupError::NotCancelable);
    }

    // Must not be already depleted
    if stream.is_depleted {
        panic_with_error!(env, LockupError::AlreadyCancelled);
    }

    // Calculate how much has vested.
    // streamed_amount_of guarantees result in [0, total_amount].
    let streamed = streamed_amount_of(env, &stream);

    // Defensive validation: streamed must be in [0, total_amount].
    if streamed < 0 || streamed > stream.total_amount {
        panic_with_error!(env, LockupError::InvalidCancellationAmount);
    }

    // Sender gets back unvested tokens (checked subtraction).
    let sender_amount = stream
        .total_amount
        .checked_sub(streamed)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));

    // Defensive: sender_amount must be non-negative.
    if sender_amount < 0 {
        panic_with_error!(env, LockupError::InvalidCancellationAmount);
    }

    // Cap sender_amount at (total_amount - withdrawn_amount).
    // The sender cannot reclaim tokens already withdrawn by the recipient.
    let max_refundable = stream
        .total_amount
        .checked_sub(stream.withdrawn_amount)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));
    let sender_amount = sender_amount.min(max_refundable);

    // Recipient gets vested minus already withdrawn (checked subtraction).
    let recipient_amount = streamed
        .checked_sub(stream.withdrawn_amount)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError));

    // Defensive: recipient_amount must be non-negative.
    if recipient_amount < 0 {
        panic_with_error!(env, LockupError::InvalidCancellationAmount);
    }

    // Mark as canceled
    stream.was_canceled = true;
    stream.cancelable = false;
    stream.refunded_amount = sender_amount;

    // If no tokens left for recipient, mark as depleted
    if recipient_amount == 0 {
        stream.is_depleted = true;
    }

    let sender = stream.sender.clone();
    let recipient = stream.recipient.clone();
    let token_addr = stream.token.clone();

    storage::set_stream(env, stream_id, &stream);

    // Update aggregate balance.
    // Bound the aggregate reduction: never reduce by more than what this
    // stream contributes (total_amount - withdrawn_amount).
    let agg = storage::get_aggregate_balance(env, &token_addr);
    let aggregate_reduction = sender_amount.min(max_refundable);
    storage::set_aggregate_balance(
        env,
        &token_addr,
        agg.checked_sub(aggregate_reduction)
            .unwrap_or_else(|| panic_with_error!(env, LockupError::ArithmeticError)),
    );

    // Refund unvested tokens to sender
    if sender_amount > 0 {
        let token_client = token::Client::new(env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &sender, &sender_amount);
    }

    events::emit_lockup_canceled(
        env,
        stream_id,
        &sender,
        &recipient,
        sender_amount,
        recipient_amount,
    );

    sender_amount
}

/// Renounce cancelability — permanently makes the stream non-cancelable.
///
/// Only the sender can renounce. The stream must be currently cancelable.
pub fn renounce(env: &Env, stream_id: u64) {
    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::StreamNotFound));

    if !stream.cancelable {
        panic_with_error!(env, LockupError::NotCancelable);
    }

    if stream.is_depleted || stream.was_canceled {
        panic_with_error!(env, LockupError::AlreadyCancelled);
    }

    stream.cancelable = false;
    storage::set_stream(env, stream_id, &stream);

    events::emit_lockup_renounced(env, stream_id);
}
