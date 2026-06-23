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
// Lockup Stream (stub for Phase 3)
// ---------------------------------------------------------------------------

/// Core data structure for a Lockup (fixed-term vesting) stream.
///
/// Simplified v1: linear unlock only (no cliff or dynamic curves).
/// Will be expanded in Phase 3 implementation.
#[contracttype]
#[derive(Clone, Debug)]
pub struct LockupStream {
    /// The address that created and funded the lockup.
    pub sender: Address,
    /// The address that receives tokens as they unlock.
    pub recipient: Address,
    /// The Soroban token contract address.
    pub token: Address,
    /// Total amount locked (in token decimals).
    pub total_amount: i128,
    /// Amount already withdrawn by the recipient.
    pub withdrawn_amount: i128,
    /// Unix timestamp when the lockup begins unlocking.
    pub start_time: u64,
    /// Unix timestamp when the lockup is fully unlocked.
    pub end_time: u64,
    /// Optional cliff timestamp. No tokens unlock before this time.
    /// Set to 0 for no cliff.
    pub cliff_time: u64,
    /// Whether the sender can cancel this stream and reclaim unvested tokens.
    pub cancelable: bool,
    /// Whether this stream has been cancelled.
    pub is_cancelled: bool,
}
