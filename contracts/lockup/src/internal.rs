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
pub fn streamed_amount_of(env: &Env, stream: &LockupStream) -> i128 {
    // If depleted, the streamed amount is the withdrawn amount (no more to stream).
    if stream.is_depleted {
        return stream.withdrawn_amount;
    }

    // If canceled, the streamed amount is total minus refunded.
    if stream.was_canceled {
        return stream.total_amount - stream.refunded_amount;
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
        return stream.start_unlock_amount;
    }

    // Between cliff and end: linear interpolation with discrete steps.
    let unlock_amounts_sum = stream.start_unlock_amount + stream.cliff_unlock_amount;

    // Safety: if unlock amounts >= total, everything is unlocked.
    if unlock_amounts_sum >= stream.total_amount {
        return stream.total_amount;
    }

    // Determine the reference point for elapsed time calculation.
    let reference_time = if stream.cliff_time > 0 {
        stream.cliff_time
    } else {
        stream.start_time
    };

    let streamable_duration = (stream.end_time - reference_time) as i128;
    let streamable_amount = stream.total_amount - unlock_amounts_sum;

    // Calculate elapsed time in granularity units (discrete steps).
    let raw_elapsed = (now - reference_time) as i128;
    let granularity = stream.granularity as i128;
    let elapsed_in_granularity_units = raw_elapsed / granularity;
    let discrete_elapsed = elapsed_in_granularity_units * granularity;

    // streamed_portion = discrete_elapsed * streamable_amount / streamable_duration
    let streamed_portion = discrete_elapsed
        .checked_mul(streamable_amount)
        .expect("streamed portion overflow")
        / streamable_duration;

    let vested = unlock_amounts_sum + streamed_portion;

    // Safety: clamp to total_amount to avoid overshoot from rounding.
    if vested > stream.total_amount {
        return stream.withdrawn_amount;
    }

    vested
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
    stream.total_amount - streamed
}

// ---------------------------------------------------------------------------
// State-changing internal functions
// ---------------------------------------------------------------------------

/// Create a new Lockup stream.
///
/// Validates inputs, transfers tokens from sender to the contract, stores the
/// stream record, and emits the creation event.
pub fn create(env: &Env, params: &CreateLockupParams) -> u64 {
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

    // Validate: unlock amounts don't exceed total
    let unlock_sum = params.start_unlock_amount + params.cliff_unlock_amount;
    if unlock_sum > params.total_amount {
        panic_with_error!(env, LockupError::AmountZero);
    }

    // Validate: granularity must be > 0, default to 1
    let effective_granularity = if params.granularity == 0 {
        1
    } else {
        params.granularity
    };

    // Allocate stream ID
    let stream_id = storage::get_next_stream_id(env);
    storage::set_next_stream_id(env, stream_id + 1);

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

    // Transfer tokens from sender into the contract (fully pre-funded)
    let token_client = token::Client::new(env, &params.token);
    token_client.transfer(
        &params.sender,
        &env.current_contract_address(),
        &params.total_amount,
    );

    // Update aggregate balance
    let agg = storage::get_aggregate_balance(env, &params.token);
    storage::set_aggregate_balance(
        env,
        &params.token,
        agg.checked_add(params.total_amount)
            .expect("aggregate overflow"),
    );

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

    // Update withdrawn amount
    stream.withdrawn_amount += amount;

    // Check if stream is now depleted
    // Using >= for safety — if withdrawn + refunded >= total, mark depleted
    if stream.withdrawn_amount >= stream.total_amount - stream.refunded_amount {
        stream.is_depleted = true;
        stream.cancelable = false;
    }

    let token_addr = stream.token.clone();
    storage::set_stream(env, stream_id, &stream);

    // Update aggregate balance
    let agg = storage::get_aggregate_balance(env, &token_addr);
    storage::set_aggregate_balance(env, &token_addr, agg - amount);

    // Transfer tokens to recipient
    let token_client = token::Client::new(env, &token_addr);
    token_client.transfer(&env.current_contract_address(), to, &amount);

    events::emit_lockup_withdraw(env, stream_id, to, caller, amount);
}

/// Cancel a Lockup stream.
///
/// Only the sender can cancel. The stream must be cancelable and not yet
/// depleted or already canceled. Unvested tokens are returned to the sender.
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

    // Calculate how much has vested
    let streamed = streamed_amount_of(env, &stream);

    // Sender gets back unvested tokens
    let sender_amount = stream.total_amount - streamed;

    // Recipient gets vested minus already withdrawn
    let recipient_amount = streamed - stream.withdrawn_amount;

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

    // Update aggregate balance
    let agg = storage::get_aggregate_balance(env, &token_addr);
    storage::set_aggregate_balance(env, &token_addr, agg - sender_amount);

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
