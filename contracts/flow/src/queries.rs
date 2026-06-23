//! Read-only query functions for the Flow contract.
//!
//! These wrap the internal debt calculations with storage lookups and
//! stream existence validation. They are called by the public API in `lib.rs`.

use crate::internal;
use crate::storage;
use shared::errors::FlowError;
use shared::types::{FlowStream, StreamStatus};
use soroban_sdk::{panic_with_error, Env};

/// Get a stream record, panicking if it doesn't exist.
pub fn require_stream(env: &Env, stream_id: u64) -> FlowStream {
    storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound))
}

/// Returns the amount of debt covered by the stream balance (= withdrawable amount).
pub fn covered_debt_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::covered_debt_of(env, &stream)
}

/// Returns the total debt owed (snapshot + ongoing), in token decimals.
pub fn total_debt_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::total_debt_of(env, &stream)
}

/// Returns the uncovered debt (debt exceeding available balance).
pub fn uncovered_debt_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::uncovered_debt_of(env, &stream)
}

/// Returns the withdrawable amount (alias for covered_debt_of).
pub fn withdrawable_amount_of(env: &Env, stream_id: u64) -> i128 {
    covered_debt_of(env, stream_id)
}

/// Returns the refundable amount (balance - covered_debt).
pub fn refundable_amount_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::refundable_amount_of(env, &stream)
}

/// Returns the ongoing debt since last snapshot, in 18-decimal fixed-point.
pub fn ongoing_debt_scaled_of(env: &Env, stream_id: u64) -> i128 {
    let stream = require_stream(env, stream_id);
    internal::ongoing_debt_scaled_of(env, &stream)
}

/// Calculate the depletion time — when total debt will exceed balance.
///
/// Returns 0 if the stream is already insolvent.
/// Panics if the stream is paused or has zero balance.
pub fn depletion_time_of(env: &Env, stream_id: u64) -> u64 {
    let stream = require_stream(env, stream_id);

    // Cannot query depletion time for paused streams
    if stream.rate_per_second == 0 {
        panic_with_error!(env, FlowError::StreamPaused);
    }

    if stream.balance == 0 {
        panic_with_error!(env, FlowError::BalanceZero);
    }

    let balance_scaled = shared::math::scale_amount(stream.balance, stream.token_decimals);
    let one_mvt_scaled = shared::math::scale_amount(1, stream.token_decimals);

    // If already insolvent, return 0
    let ongoing = internal::ongoing_debt_scaled_of(env, &stream);
    if stream.snapshot_debt_scaled + ongoing >= balance_scaled + one_mvt_scaled {
        return 0;
    }

    // solvency_amount = balance_scaled - snapshot_debt_scaled + one_mvt_scaled
    let solvency_amount = balance_scaled - stream.snapshot_debt_scaled + one_mvt_scaled;
    let solvency_period = solvency_amount / stream.rate_per_second;

    if solvency_amount % stream.rate_per_second == 0 {
        stream.snapshot_time + solvency_period as u64
    } else {
        // Round up — depletion happens at the next second
        stream.snapshot_time + solvency_period as u64 + 1
    }
}

/// Derive the stream status from its current state.
pub fn status_of(env: &Env, stream_id: u64) -> StreamStatus {
    let stream = require_stream(env, stream_id);
    let now = env.ledger().timestamp();

    // Pending: snapshot_time is in the future
    if stream.snapshot_time > now {
        return StreamStatus::Pending;
    }

    // Voided: permanently stopped
    if stream.is_voided {
        return StreamStatus::Voided;
    }

    let has_debt = internal::uncovered_debt_of(env, &stream) > 0;

    if stream.rate_per_second == 0 {
        // Paused
        if has_debt {
            StreamStatus::PausedInsolvent
        } else {
            StreamStatus::PausedSolvent
        }
    } else {
        // Streaming
        if has_debt {
            StreamStatus::StreamingInsolvent
        } else {
            StreamStatus::StreamingSolvent
        }
    }
}
