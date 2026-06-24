//! Read-only query functions for the Lockup contract.
//!
//! These wrap the internal vesting calculations with storage lookups and
//! stream existence validation.

use crate::internal;
use crate::storage;
use shared::errors::LockupError;
use shared::types::{LockupStatus, LockupStream};
use soroban_sdk::{panic_with_error, Env};

/// Get a stream record, panicking if it doesn't exist.
pub fn require_stream(env: &Env, stream_id: u64) -> LockupStream {
    storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, LockupError::StreamNotFound))
}

/// Returns the total vested ("streamed") amount at the current time.
pub fn streamed_amount_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::streamed_amount_of(env, &stream)
}

/// Returns the withdrawable amount (vested - already withdrawn).
pub fn withdrawable_amount_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::withdrawable_amount_of(env, &stream)
}

/// Returns the refundable amount (total - vested).
/// Returns 0 if the stream is not cancelable.
pub fn refundable_amount_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::refundable_amount_of(env, &stream)
}

/// Derive the Lockup stream status from its current state.
///
/// Temperature semantics:
/// - Warm: Pending, Streaming (time alone can change the status)
/// - Cold: Settled, Canceled, Depleted (frozen, time cannot change)
pub fn status_of(env: &Env, stream_id: u64) -> LockupStatus {
    let stream = require_stream(env, stream_id);
    let now = env.ledger().timestamp();

    // Cold statuses first (order matters):
    if stream.is_depleted {
        return LockupStatus::Depleted;
    }
    if stream.was_canceled {
        return LockupStatus::Canceled;
    }

    // Warm statuses:
    if now < stream.start_time {
        return LockupStatus::Pending;
    }

    // Check if all tokens have vested (settled)
    let streamed = internal::streamed_amount_of(env, &stream);
    if streamed >= stream.total_amount {
        LockupStatus::Settled
    } else {
        LockupStatus::Streaming
    }
}

/// Returns true if the stream is in a "cold" state (time can't change it).
pub fn is_cold(env: &Env, stream_id: u64) -> bool {
    let status = status_of(env, stream_id);
    matches!(
        status,
        LockupStatus::Settled | LockupStatus::Canceled | LockupStatus::Depleted
    )
}

/// Returns true if the stream is in a "warm" state (time can change it).
pub fn is_warm(env: &Env, stream_id: u64) -> bool {
    let status = status_of(env, stream_id);
    matches!(status, LockupStatus::Pending | LockupStatus::Streaming)
}
