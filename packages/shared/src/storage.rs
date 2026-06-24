//! Storage key definitions and TTL helpers for the Fundable streaming protocol.
//!
//! Per SKILL.md §3:
//! - `Instance` storage for contract-wide config (admin, next_stream_id).
//! - `Persistent` storage for long-lived data (stream records, aggregate amounts).
//! - Typed enum keys to avoid collisions between modules.
//! - Explicit TTL extension to prevent state archival.

use soroban_sdk::{contracttype, Address};

// ---------------------------------------------------------------------------
// Storage Keys
// ---------------------------------------------------------------------------

/// Storage keys for contract data.
///
/// Using a typed enum prevents key collisions (SKILL.md §3).
/// Keys are namespaced by variant to keep different data types separate.
#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    /// Admin address (Instance storage).
    Admin,
    /// Next stream ID counter (Instance storage).
    NextStreamId,
    /// A Flow stream record, keyed by stream ID (Persistent storage).
    FlowStream(u64),
    /// A Lockup stream record, keyed by stream ID (Persistent storage).
    LockupStream(u64),
    /// Aggregate token balance held by the contract for a given token address.
    /// Used for surplus recovery / accounting reconciliation (Persistent storage).
    AggregateBalance(Address),
    /// NFT token owner, keyed by token ID (Persistent storage).
    TokenOwner(i128),
    /// NFT stream data mapping token ID to stream type and ID (Persistent storage).
    TokenStreamData(i128),
    /// Number of NFTs owned by an address (Persistent storage).
    NftBalance(Address),
    /// Token metadata, e.g., name, symbol, URI (Instance storage).
    TokenMetadata(soroban_sdk::Symbol),
    /// Router configuration: Flow contract address.
    FlowContract,
    /// Router configuration: Lockup contract address.
    LockupContract,
    /// Router configuration: NFT contract address.
    NftContract,
}

// ---------------------------------------------------------------------------
// TTL Constants
// ---------------------------------------------------------------------------

/// TTL for instance storage entries (admin, config).
/// ~30 days at ~5 sec/ledger = 518_400 ledgers.
pub const INSTANCE_TTL_LEDGERS: u32 = 518_400;

/// TTL threshold — extend when remaining TTL drops below this.
/// ~7 days = 120_960 ledgers.
pub const INSTANCE_TTL_THRESHOLD: u32 = 120_960;

/// TTL for persistent storage entries (stream records).
/// ~120 days at ~5 sec/ledger = 2_073_600 ledgers.
/// Streams can be long-lived, so we use a generous TTL.
pub const PERSISTENT_TTL_LEDGERS: u32 = 2_073_600;

/// Threshold to trigger persistent TTL extension.
/// ~30 days = 518_400 ledgers.
pub const PERSISTENT_TTL_THRESHOLD: u32 = 518_400;
