//! Shared data types for the Fundable streaming protocol.
//!
//! These types are used across the Flow, Lockup, Router, and Stream NFT contracts.

use soroban_sdk::{contracttype, Address};

// ---------------------------------------------------------------------------
// Stream Status
// ---------------------------------------------------------------------------

/// Status of a Flow stream.
///
/// The status is derived from the stream's current state (rate, balance, debt, voided flag) rather than
/// being stored directly — keeping storage minimal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamStatus {
    /// Stream scheduled to start in the future (snapshot_time > now).
    Pending = 0,
    /// Actively streaming with balance covering all debt.
    StreamingSolvent = 1,
    /// Actively streaming but debt exceeds available balance.
    StreamingInsolvent = 2,
    /// Paused by sender with no uncovered debt.
    PausedSolvent = 3,
    /// Paused by sender with uncovered debt.
    PausedInsolvent = 4,
    /// Permanently stopped. Cannot be restarted. Uncovered debt written off.
    Voided = 5,
}

// ---------------------------------------------------------------------------
// Flow Stream
// ---------------------------------------------------------------------------

/// Core data structure for a Flow (open-ended, rate-per-second) stream.
///
/// Ported from Sablier's `Flow.Stream` struct with these adaptations:
///
/// - `UD21x18 ratePerSecond` → `i128 rate_per_second` scaled to 18 decimals.
///   Soroban has no native fixed-point type, so we use i128 with manual scaling.
///   A rate of 1 token/sec for a 7-decimal token = 1e18 internally.
///
/// - `IERC20 token` → `Address` (Soroban token contract address).
///
/// - `uint128 balance` → `i128` (Soroban SDK convention for token amounts).
///
/// - `uint40 snapshotTime` → `u64` (Soroban ledger timestamp is u64).
///
/// - `bool isStream` sentinel removed — we use storage key existence instead.
///
/// - `bool isTransferable` removed — delegated to the NFT contract layer.
///
/// # Storage Layout
///
/// Each stream is stored under `DataKey::Stream(stream_id)` in persistent storage.
/// The struct is kept as flat as possible to minimize serialization overhead.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FlowStream {
    /// The address streaming the tokens (can pause, adjust, refund).
    pub sender: Address,
    /// The address receiving the tokens (can withdraw).
    pub recipient: Address,
    /// The Soroban token contract address (SAC or custom SEP-41).
    pub token: Address,
    /// Number of decimals for the token (e.g. 7 for most Stellar assets).
    /// Used for scale/descale operations. Max 18.
    pub token_decimals: u32,
    /// Current balance held in the stream (deposited - withdrawn), in token decimals.
    pub balance: i128,
    /// Rate at which debt accrues, in 18-decimal fixed-point.
    /// 0 means the stream is paused.
    /// Example: 1 token/sec for a 7-decimal token → 1_000_000_000_000_000_000 (1e18).
    pub rate_per_second: i128,
    /// Unix timestamp of the last debt snapshot.
    /// Debt accrues from this point forward at `rate_per_second`.
    pub snapshot_time: u64,
    /// Accumulated debt at the last snapshot, in 18-decimal fixed-point.
    /// Total debt = snapshot_debt_scaled + ongoing_debt_scaled.
    pub snapshot_debt_scaled: i128,
    /// Whether the stream has been permanently voided.
    pub is_voided: bool,
}

// ---------------------------------------------------------------------------
// Lockup Stream
// ---------------------------------------------------------------------------

/// Status of a Lockup stream.
///
/// Uses "temperature" semantics:
/// - **Warm** (Pending, Streaming): time alone can change the status.
/// - **Cold** (Settled, Canceled, Depleted): time alone cannot change the status.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LockupStatus {
    /// Created but start_time is in the future. No tokens have vested.
    Pending = 0,
    /// Active — tokens are currently vesting over time.
    Streaming = 1,
    /// All tokens have fully vested. Recipient can withdraw the remaining balance.
    Settled = 2,
    /// Sender canceled the stream. Unvested tokens returned to sender.
    Canceled = 3,
    /// Fully withdrawn (and/or refunded). No tokens remain in the stream.
    Depleted = 4,
}

/// Core data structure for a Lockup (fixed-term vesting) stream.
///
/// Supports linear unlock with optional cliff. The vested ("streamed") amount
/// at any time `t` is calculated as:
///
/// ```text
/// if t < start_time:       vested = 0
/// if t < cliff_time:       vested = start_unlock_amount
/// if t >= end_time:         vested = total_amount
/// else:
///   elapsed = floor((t - cliff_time) / granularity) * granularity
///   streamable_duration = end_time - cliff_time
///   streamable_amount = total_amount - start_unlock_amount - cliff_unlock_amount
///   vested = start_unlock_amount + cliff_unlock_amount + (elapsed * streamable_amount / streamable_duration)
/// ```
///
/// This mirrors the reference linear lockup calculation with discrete unlock
/// steps at `granularity`-second intervals.
#[contracttype]
#[derive(Clone, Debug)]
pub struct LockupStream {
    /// The address that created and funded the lockup (can cancel if `cancelable`).
    pub sender: Address,
    /// The address that receives tokens as they unlock (can withdraw).
    pub recipient: Address,
    /// The Soroban token contract address.
    pub token: Address,
    /// Total amount deposited into the stream (in token decimals).
    pub total_amount: i128,
    /// Cumulative amount withdrawn by the recipient.
    pub withdrawn_amount: i128,
    /// Amount refunded to sender on cancellation. Zero unless cancelled.
    pub refunded_amount: i128,
    /// Unix timestamp when the lockup begins.
    pub start_time: u64,
    /// Unix timestamp when the lockup is fully unlocked.
    pub end_time: u64,
    /// Optional cliff timestamp. No tokens beyond `start_unlock_amount`
    /// vest before this time. Set to 0 for no cliff.
    pub cliff_time: u64,
    /// Amount unlocked immediately at `start_time`.
    pub start_unlock_amount: i128,
    /// Amount unlocked at `cliff_time` (in addition to `start_unlock_amount`).
    pub cliff_unlock_amount: i128,
    /// Unlock granularity in seconds. Tokens vest in discrete steps of this
    /// interval. Default = 1 (per-second vesting). Must be > 0.
    pub granularity: u64,
    /// Whether the sender can cancel this stream and reclaim unvested tokens.
    pub cancelable: bool,
    /// Whether the stream has been cancelled.
    pub was_canceled: bool,
    /// Whether all tokens have been withdrawn and/or refunded.
    pub is_depleted: bool,
}

/// Parameters for creating a new Lockup stream.
///
/// Bundled into a struct because Soroban contract functions
/// have a max of 10 parameters.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreateLockupParams {
    /// Address funding the stream (can cancel if `cancelable`).
    pub sender: Address,
    /// Address receiving vested tokens.
    pub recipient: Address,
    /// Soroban token contract address.
    pub token: Address,
    /// Total tokens to vest (in token decimals).
    pub total_amount: i128,
    /// Unix timestamp when vesting begins.
    pub start_time: u64,
    /// Unix timestamp when vesting completes.
    pub end_time: u64,
    /// Optional cliff timestamp. Set to 0 for no cliff.
    pub cliff_time: u64,
    /// Tokens unlocked immediately at start.
    pub start_unlock_amount: i128,
    /// Tokens unlocked at cliff (added to start).
    pub cliff_unlock_amount: i128,
    /// Unlock step interval in seconds. 0 defaults to 1.
    pub granularity: u64,
    /// Whether the sender can cancel the stream.
    pub cancelable: bool,
}
