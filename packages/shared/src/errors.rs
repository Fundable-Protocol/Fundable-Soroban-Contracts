//! Error types for the Fundable streaming protocol.
//!
//! Ported from Sablier's `Errors.sol` library. Each contract uses a single
//! `#[contracterror]` enum so that error codes are compact and unique.
//!
//! Soroban convention: error values are `u32` and should be stable across
//! contract upgrades to avoid breaking client error handling.

use soroban_sdk::contracterror;

// ---------------------------------------------------------------------------
// Flow Contract Errors
// ---------------------------------------------------------------------------

/// Errors emitted by the Flow streaming contract.
///
/// Maps to Sablier's `Errors.sol` — see inline comments for the original
/// Solidity error name.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum FlowError {
    // --- Stream existence & state ---
    /// Stream ID does not exist. (SablierFlowState_Null)
    StreamNotFound = 1,
    /// Caller is not authorized for this operation. (SablierFlowState_Unauthorized)
    Unauthorized = 2,
    /// Stream is paused; operation requires active streaming. (SablierFlowState_StreamPaused)
    StreamPaused = 3,
    /// Stream is voided; no further mutations allowed. (SablierFlowState_StreamVoided)
    StreamVoided = 4,
    /// Stream is not paused; restart requires a paused stream. (SablierFlow_StreamNotPaused)
    StreamNotPaused = 5,
    /// Stream has not started yet (snapshot_time in the future). (SablierFlow_StreamPending)
    StreamPending = 6,

    // --- Rate & amount validation ---
    /// New rate per second must be > 0. (SablierFlow_NewRatePerSecondZero)
    RatePerSecondZero = 7,
    /// New rate must differ from current rate. (SablierFlow_RatePerSecondNotDifferent)
    RateNotDifferent = 8,
    /// Deposit amount must be > 0. (SablierFlow_DepositAmountZero)
    DepositAmountZero = 9,
    /// Withdraw amount must be > 0. (SablierFlow_WithdrawAmountZero)
    WithdrawAmountZero = 10,
    /// Withdraw amount exceeds withdrawable balance. (SablierFlow_Overdraw)
    Overdraw = 11,
    /// Refund amount must be > 0. (SablierFlow_RefundAmountZero)
    RefundAmountZero = 12,
    /// Refund amount exceeds refundable balance. (SablierFlow_RefundOverflow)
    RefundOverflow = 13,

    // --- Token validation ---
    /// Token has > 18 decimals, unsupported. (SablierFlow_InvalidTokenDecimals)
    InvalidTokenDecimals = 14,

    // --- Balance ---
    /// Stream balance is zero (e.g. querying depletion time). (SablierFlow_StreamBalanceZero)
    BalanceZero = 15,

    // --- Initialization ---
    /// Contract already initialized.
    AlreadyInitialized = 16,
    /// Contract not yet initialized.
    NotInitialized = 17,

    // --- Create validation ---
    /// Cannot create a pending stream with rate_per_second = 0. (SablierFlow_CreateRatePerSecondZero)
    CreateRatePerSecondZero = 18,

    // --- Internal safety ---
    /// Internal math error — should never occur in production.
    InvalidCalculation = 19,
}

// ---------------------------------------------------------------------------
// Lockup Contract Errors (stub for Phase 3)
// ---------------------------------------------------------------------------

/// Errors emitted by the Lockup vesting contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum LockupError {
    /// Stream ID does not exist.
    StreamNotFound = 101,
    /// Caller is not authorized.
    Unauthorized = 102,
    /// Stream is already cancelled.
    AlreadyCancelled = 103,
    /// Stream is not cancelable.
    NotCancelable = 104,
    /// Withdraw amount exceeds unlocked balance.
    Overdraw = 105,
    /// Invalid time parameters (start >= end, cliff outside range).
    InvalidTimeRange = 106,
    /// Total amount must be > 0.
    AmountZero = 107,
    /// Contract already initialized.
    AlreadyInitialized = 108,
    /// Contract not yet initialized.
    NotInitialized = 109,
}
