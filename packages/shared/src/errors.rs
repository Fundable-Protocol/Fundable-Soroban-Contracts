//! Error types for the Fundable streaming protocol.
//!
//! Each contract uses a single `#[contracterror]` enum so that error codes are compact and unique.
//!
//! Soroban convention: error values are `u32` and should be stable across
//! contract upgrades to avoid breaking client error handling.

use soroban_sdk::contracterror;

// ---------------------------------------------------------------------------
// Flow Contract Errors
// ---------------------------------------------------------------------------

/// Errors emitted by the Flow streaming contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum FlowError {
    // --- Stream existence & state ---
    /// Stream ID does not exist. 
    StreamNotFound = 1,
    /// Caller is not authorized for this operation. 
    Unauthorized = 2,
    /// Stream is paused; operation requires active streaming. 
    StreamPaused = 3,
    /// Stream is voided; no further mutations allowed. 
    StreamVoided = 4,
    /// Stream is not paused; restart requires a paused stream. 
    StreamNotPaused = 5,
    /// Stream has not started yet (snapshot_time in the future). 
    StreamPending = 6,

    // --- Rate & amount validation ---
    /// New rate per second must be > 0. 
    RatePerSecondZero = 7,
    /// New rate must differ from current rate. 
    RateNotDifferent = 8,
    /// Deposit amount must be > 0. 
    DepositAmountZero = 9,
    /// Withdraw amount must be > 0. 
    WithdrawAmountZero = 10,
    /// Withdraw amount exceeds withdrawable balance. 
    Overdraw = 11,
    /// Refund amount must be > 0. 
    RefundAmountZero = 12,
    /// Refund amount exceeds refundable balance. 
    RefundOverflow = 13,

    // --- Token validation ---
    /// Token has > 18 decimals, unsupported. 
    InvalidTokenDecimals = 14,

    // --- Balance ---
    /// Stream balance is zero (e.g. querying depletion time). 
    BalanceZero = 15,

    // --- Initialization ---
    /// Contract already initialized.
    AlreadyInitialized = 16,
    /// Contract not yet initialized.
    NotInitialized = 17,

    // --- Create validation ---
    /// Cannot create a pending stream with rate_per_second = 0. 
    CreateRatePerSecondZero = 18,

    // --- Internal safety ---
    /// Internal math error — should never occur in production.
    InvalidCalculation = 19,

    // --- Create validation (H-1, H-4) ---
    /// Sender and recipient must be different addresses.
    SenderEqualsRecipient = 20,
    /// Rate per second must not be negative.
    NegativeRate = 21,
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
    /// Sender and recipient must be different addresses.
    SenderEqualsRecipient = 110,
}

// ---------------------------------------------------------------------------
// NFT Errors
// ---------------------------------------------------------------------------

/// Errors specific to the Stream NFT contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NftError {
    AlreadyInitialized = 201,
    NotAuthorized = 202,
    TokenNotFound = 203,
    NotTransferable = 204,
    /// Token ID has already been minted.
    AlreadyMinted = 205,
}

// ---------------------------------------------------------------------------
// Router Errors
// ---------------------------------------------------------------------------

/// Errors specific to the Router contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RouterError {
    AlreadyInitialized = 301,
    NotInitialized = 302,
    NotAuthorized = 303,
    InvalidStreamType = 304,
}
