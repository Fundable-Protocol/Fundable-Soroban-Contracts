//! Internal (private) implementation functions for the Flow contract.
//!
//! These functions contain the core business logic. They are called by the public
//! API in `lib.rs` after authorization checks.
//!
//! # Architecture Notes
//!
//! - **Debt tracking**: All debt is tracked in 18-decimal fixed-point.
//!   `snapshot_debt_scaled` captures historical debt; `ongoing_debt_scaled`
//!   is computed on-the-fly from elapsed time × rate.
//!
//! - **Pause via rate = 0**: Rather than a separate `is_paused` flag,
//!   pausing sets `rate_per_second = 0`. The ongoing
//!   debt calculation naturally returns 0 when rate is 0.
//!
//! - **Snapshot pattern**: Before any rate change (adjust, pause, restart),
//!   we "snapshot" the ongoing debt into `snapshot_debt_scaled` and update
//!   `snapshot_time` to the current timestamp. This freezes historical debt.

use crate::storage;
use shared::errors::FlowError;
use shared::events;
use shared::math;
use shared::types::FlowStream;
use soroban_sdk::{panic_with_error, token, Address, Env};

// ---------------------------------------------------------------------------
// Read-only debt calculations
// ---------------------------------------------------------------------------

/// Calculate the ongoing debt since the last snapshot, in 18-decimal fixed-point.
///
/// Returns 0 if the stream is paused (rate = 0) or if the snapshot time
/// is in the future (stream is pending).
pub fn ongoing_debt_scaled_of(env: &Env, stream: &FlowStream) -> i128 {
    let now = env.ledger().timestamp();

    // If snapshot is in the future, stream hasn't started yet
    if stream.snapshot_time >= now {
        return 0;
    }

    // If rate is 0, stream is paused — no ongoing debt accrual
    if stream.rate_per_second == 0 {
        return 0;
    }

    let elapsed = (now - stream.snapshot_time) as i128;
    // ongoing_debt = elapsed_seconds × rate_per_second (both in 18-dec)
    elapsed
        .checked_mul(stream.rate_per_second)
        .expect("ongoing debt overflow")
}

/// Calculate the total debt (snapshot + ongoing), in token decimals.
///
/// This is the total amount owed from sender to recipient, regardless
/// of whether the stream has sufficient balance to cover it.
pub fn total_debt_of(env: &Env, stream: &FlowStream) -> i128 {
    let total_scaled = ongoing_debt_scaled_of(env, stream)
        .checked_add(stream.snapshot_debt_scaled)
        .expect("total debt overflow");
    math::descale_amount(total_scaled, stream.token_decimals)
}

/// Calculate the covered debt — the portion of total debt backed by balance.
///
/// `covered_debt = min(balance, total_debt)`
///
/// This is also the withdrawable amount for the recipient.
pub fn covered_debt_of(env: &Env, stream: &FlowStream) -> i128 {
    if stream.balance == 0 {
        return 0;
    }

    let total_debt = total_debt_of(env, stream);

    if stream.balance < total_debt {
        stream.balance
    } else {
        total_debt
    }
}

/// Calculate the uncovered debt — debt exceeding available balance.
///
/// `uncovered_debt = max(0, total_debt - balance)`
pub fn uncovered_debt_of(env: &Env, stream: &FlowStream) -> i128 {
    let total_debt = total_debt_of(env, stream);
    if stream.balance < total_debt {
        total_debt - stream.balance
    } else {
        0
    }
}

/// Calculate the refundable amount — excess balance not owed to recipient.
///
/// `refundable = balance - covered_debt`
pub fn refundable_amount_of(env: &Env, stream: &FlowStream) -> i128 {
    stream.balance - covered_debt_of(env, stream)
}

// ---------------------------------------------------------------------------
// State-changing internal functions
// ---------------------------------------------------------------------------

/// Create a new Flow stream.
///
/// Validates inputs, stores the stream record, emits the creation event.
/// Does NOT transfer tokens — the stream starts with balance = 0.
pub fn create(
    env: &Env,
    sender: &Address,
    recipient: &Address,
    token: &Address,
    rate_per_second: i128,
    token_decimals: u32,
    start_time: u64,
) -> u64 {
    // Validate token decimals (SKILL.md §5)
    if token_decimals > 18 {
        panic_with_error!(env, FlowError::InvalidTokenDecimals);
    }

    // Validate: sender and recipient must differ (H-1)
    if sender == recipient {
        panic_with_error!(env, FlowError::SenderEqualsRecipient);
    }

    // Validate: rate must not be negative (H-4)
    if rate_per_second < 0 {
        panic_with_error!(env, FlowError::NegativeRate);
    }

    let now = env.ledger().timestamp();

    // If start_time is in the future, rate must be > 0 (can't create a pending paused stream)
    if start_time > now && rate_per_second == 0 {
        panic_with_error!(env, FlowError::CreateRatePerSecondZero);
    }

    // Determine snapshot time: 0 sentinel → use current timestamp
    let snapshot_time = if start_time == 0 { now } else { start_time };

    // Allocate stream ID
    let stream_id = storage::get_next_stream_id(env);
    storage::set_next_stream_id(env, stream_id + 1);

    // Build and store the stream
    let stream = FlowStream {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        token_decimals,
        balance: 0,
        rate_per_second,
        snapshot_time,
        snapshot_debt_scaled: 0,
        is_voided: false,
    };
    storage::set_stream(env, stream_id, &stream);

    // Emit event (SKILL.md §8)
    events::emit_flow_created(
        env,
        stream_id,
        sender,
        recipient,
        token,
        rate_per_second,
        snapshot_time,
    );

    stream_id
}

/// Deposit tokens into a Flow stream.
///
/// Transfers tokens from the caller into the contract, then updates
/// the stream balance and aggregate accounting.
pub fn deposit(env: &Env, stream_id: u64, funder: &Address, amount: i128) {
    if amount <= 0 {
        panic_with_error!(env, FlowError::DepositAmountZero);
    }

    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // Cannot deposit into a voided stream
    if stream.is_voided {
        panic_with_error!(env, FlowError::StreamVoided);
    }

    // Transfer tokens from funder to this contract
    let token_client = token::Client::new(env, &stream.token);
    token_client.transfer(funder, &env.current_contract_address(), &amount);

    // Update stream balance
    stream.balance = stream
        .balance
        .checked_add(amount)
        .expect("balance overflow");

    // Update aggregate balance for accounting
    let agg = storage::get_aggregate_balance(env, &stream.token);
    storage::set_aggregate_balance(
        env,
        &stream.token,
        agg.checked_add(amount).expect("aggregate overflow"),
    );

    storage::set_stream(env, stream_id, &stream);

    events::emit_flow_deposit(env, stream_id, funder, amount);
}

/// Withdraw tokens from a Flow stream.
///
/// The recipient (or approved caller) withdraws accrued debt that is
/// covered by the stream's balance.
pub fn withdraw(env: &Env, stream_id: u64, caller: &Address, to: &Address, amount: i128) {
    if amount <= 0 {
        panic_with_error!(env, FlowError::WithdrawAmountZero);
    }

    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // Authorization: only recipient can withdraw (SKILL.md §1)
    if *caller != stream.recipient {
        panic_with_error!(env, FlowError::Unauthorized);
    }

    // Calculate withdrawable amount
    let withdrawable = covered_debt_of(env, &stream);
    if amount > withdrawable {
        panic_with_error!(env, FlowError::Overdraw);
    }

    // Scale the withdrawal amount for debt bookkeeping
    let amount_scaled = math::scale_amount(amount, stream.token_decimals);

    // Update debt tracking
    let total_debt_scaled = ongoing_debt_scaled_of(env, &stream)
        .checked_add(stream.snapshot_debt_scaled)
        .expect("debt overflow");

    if amount_scaled <= stream.snapshot_debt_scaled {
        // Withdrawal fits entirely within snapshot debt
        stream.snapshot_debt_scaled -= amount_scaled;
    } else {
        // Withdrawal exceeds snapshot debt — adjust ongoing debt too
        stream.snapshot_debt_scaled = total_debt_scaled - amount_scaled;
        stream.snapshot_time = env.ledger().timestamp();
    }

    // Update stream balance
    stream.balance -= amount;

    // Update aggregate balance (L-6: descriptive error)
    let agg = storage::get_aggregate_balance(env, &stream.token);
    storage::set_aggregate_balance(
        env,
        &stream.token,
        agg.checked_sub(amount)
            .expect("aggregate balance underflow on withdraw"),
    );

    storage::set_stream(env, stream_id, &stream);

    // Transfer tokens to recipient (SKILL.md §5: transfer-before-state-update
    // not needed here since Soroban doesn't have reentrancy, but we update
    // state first by convention)
    let token_client = token::Client::new(env, &stream.token);
    token_client.transfer(&env.current_contract_address(), to, &amount);

    events::emit_flow_withdraw(env, stream_id, to, caller, amount);
}

/// Adjust the rate per second of a Flow stream.
///
/// Snapshots the current ongoing debt before applying the new rate.
/// Only the sender can adjust the rate.
///
/// Sablier reference: `_adjustRatePerSecond()`
pub fn adjust_rate(env: &Env, stream_id: u64, new_rate: i128) {
    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // The new rate must differ from current
    if new_rate == stream.rate_per_second {
        panic_with_error!(env, FlowError::RateNotDifferent);
    }

    let now = env.ledger().timestamp();

    // Snapshot ongoing debt if snapshot time is in the past
    if stream.snapshot_time < now {
        let ongoing = ongoing_debt_scaled_of(env, &stream);
        if ongoing > 0 {
            stream.snapshot_debt_scaled = stream
                .snapshot_debt_scaled
                .checked_add(ongoing)
                .expect("snapshot overflow");
        }
        stream.snapshot_time = now;
    }

    let old_rate = stream.rate_per_second;
    stream.rate_per_second = new_rate;
    storage::set_stream(env, stream_id, &stream);

    let total_debt = total_debt_of(env, &stream);
    events::emit_flow_adjusted(env, stream_id, total_debt, old_rate, new_rate);
}

/// Pause a Flow stream.
///
/// Snapshots the ongoing debt and sets rate to 0.
///
/// Sablier reference: `_pause()`
pub fn pause(env: &Env, stream_id: u64) {
    let stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // Cannot pause a pending stream
    let now = env.ledger().timestamp();
    if stream.snapshot_time > now {
        panic_with_error!(env, FlowError::StreamPending);
    }

    // Defense-in-depth: cannot pause a voided stream (H-3)
    if stream.is_voided {
        panic_with_error!(env, FlowError::StreamVoided);
    }

    // Use adjust_rate to snapshot debt and set rate to 0
    adjust_rate(env, stream_id, 0);

    // Re-read the stream after adjustment to get updated total_debt
    let stream = storage::get_stream(env, stream_id).unwrap();
    let total_debt = total_debt_of(env, &stream);

    events::emit_flow_paused(env, stream_id, &stream.sender, &stream.recipient, total_debt);
}

/// Restart a paused Flow stream with a new rate.
///
/// Sablier reference: `_restart()`
pub fn restart(env: &Env, stream_id: u64, caller: &Address, rate_per_second: i128) {
    let stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // Must be paused (rate = 0)
    if stream.rate_per_second != 0 {
        panic_with_error!(env, FlowError::StreamNotPaused);
    }

    // Must not be voided
    if stream.is_voided {
        panic_with_error!(env, FlowError::StreamVoided);
    }

    // New rate must be > 0
    if rate_per_second <= 0 {
        panic_with_error!(env, FlowError::RatePerSecondZero);
    }

    // Use adjust_rate to set the new rate (snapshots debt)
    adjust_rate(env, stream_id, rate_per_second);

    events::emit_flow_restarted(env, stream_id, caller, rate_per_second);
}

/// Refund excess balance from a stream back to the sender.
///
/// Only unowed tokens (balance - covered_debt) can be refunded.
///
/// Sablier reference: `_refund()`
pub fn refund(env: &Env, stream_id: u64, amount: i128) {
    if amount <= 0 {
        panic_with_error!(env, FlowError::RefundAmountZero);
    }

    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    let refundable = refundable_amount_of(env, &stream);
    if amount > refundable {
        panic_with_error!(env, FlowError::RefundOverflow);
    }

    // Safety check: refundable should never exceed balance
    if refundable > stream.balance {
        panic_with_error!(env, FlowError::InvalidCalculation);
    }

    let sender = stream.sender.clone();
    let token_addr = stream.token.clone();

    // Update balance
    stream.balance -= amount;

    // Update aggregate (L-6: descriptive error)
    let agg = storage::get_aggregate_balance(env, &token_addr);
    storage::set_aggregate_balance(
        env,
        &token_addr,
        agg.checked_sub(amount)
            .expect("aggregate balance underflow on refund"),
    );

    storage::set_stream(env, stream_id, &stream);

    // Transfer back to sender
    let token_client = token::Client::new(env, &token_addr);
    token_client.transfer(&env.current_contract_address(), &sender, &amount);

    events::emit_flow_refunded(env, stream_id, &sender, amount);
}

/// Void a Flow stream permanently.
///
/// Can be called by sender OR recipient. Writes off uncovered debt,
/// sets rate to 0, and marks the stream as voided (irreversible).
///
/// Sablier reference: `_void()`
pub fn void_stream(env: &Env, stream_id: u64, caller: &Address) {
    let mut stream = storage::get_stream(env, stream_id)
        .unwrap_or_else(|| panic_with_error!(env, FlowError::StreamNotFound));

    // Cannot void an already-voided stream
    if stream.is_voided {
        panic_with_error!(env, FlowError::StreamVoided);
    }

    // Authorization: sender OR recipient can void (SKILL.md §1)
    if *caller != stream.sender && *caller != stream.recipient {
        panic_with_error!(env, FlowError::Unauthorized);
    }

    let debt_to_write_off = uncovered_debt_of(env, &stream);

    if debt_to_write_off == 0 {
        // Solvent: snapshot ongoing debt normally
        let ongoing = ongoing_debt_scaled_of(env, &stream);
        if ongoing > 0 {
            stream.snapshot_debt_scaled = stream
                .snapshot_debt_scaled
                .checked_add(ongoing)
                .expect("snapshot overflow");
        }
    } else {
        // Insolvent: write off uncovered debt by capping snapshot to balance
        stream.snapshot_debt_scaled = math::scale_amount(stream.balance, stream.token_decimals);
    }

    stream.snapshot_time = env.ledger().timestamp();
    stream.rate_per_second = 0;
    stream.is_voided = true;

    let sender = stream.sender.clone();
    let recipient = stream.recipient.clone();

    storage::set_stream(env, stream_id, &stream);

    let new_total_debt = total_debt_of(env, &stream);
    events::emit_flow_voided(
        env,
        stream_id,
        &sender,
        &recipient,
        caller,
        new_total_debt,
        debt_to_write_off,
    );
}
