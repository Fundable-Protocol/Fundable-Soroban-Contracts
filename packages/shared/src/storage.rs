//! Storage key definitions and TTL helpers for the Fundable streaming protocol.
//!
//! Per SKILL.md §3:
//! - `Instance` storage for contract-wide config (admin, next_stream_id).
//! - `Persistent` storage for long-lived data (stream records, aggregate amounts).
//! - Typed enum keys to avoid collisions between modules.
//! - Explicit TTL extension to prevent state archival.

use soroban_sdk::{contracttype, Address, BytesN};

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
    /// Paymaster configuration: list of allowed fee token addresses.
    AllowedFeeTokens,
    /// Pending admin address for two-step admin transfer (Instance storage).
    PendingAdmin,
    /// Proposed upgrade WASM hash (Instance storage).
    ProposedUpgrade(BytesN<32>),
    /// Ledger sequence at which a proposed upgrade becomes executable (Instance storage).
    UpgradeUnlockLedger,
    /// Next distribution ID counter (Instance storage, Distributor).
    NextDistributionId,
    /// Protocol fee percentage in basis points (Instance storage, Distributor).
    ProtocolFeePercent,
    /// Address to receive protocol fees (Instance storage, Distributor).
    ProtocolFeeAddress,
    /// A distribution record, keyed by distribution ID (Persistent storage, Distributor).
    Distribution(u32),
    /// Whether a user has claimed from a distribution (Persistent storage, Distributor).
    /// Key: (distribution_id, claimant_address).
    Claimed(u32, Address),
}

// ---------------------------------------------------------------------------
// TTL Constants
// ---------------------------------------------------------------------------

/// TTL for instance storage entries (admin, config).
/// ~90 days at ~5 sec/ledger = 1_555_200 ledgers.
pub const INSTANCE_TTL_LEDGERS: u32 = 1_555_200;

/// TTL threshold — extend when remaining TTL drops below this.
/// ~30 days = 518_400 ledgers.
pub const INSTANCE_TTL_THRESHOLD: u32 = 518_400;

/// TTL for persistent storage entries (stream records).
/// ~365 days at ~5 sec/ledger = 6_312_000 ledgers.
/// Streams can be long-lived (multi-year lockups), so we use the
/// maximum practical TTL. The `extend_stream_ttl()` keepalive
/// covers durations beyond 1 year.
pub const PERSISTENT_TTL_LEDGERS: u32 = 6_312_000;

/// Threshold to trigger persistent TTL extension.
/// ~120 days = 2_073_600 ledgers.
pub const PERSISTENT_TTL_THRESHOLD: u32 = 2_073_600;

/// Upgrade timelock: number of ledgers to wait between proposing and
/// executing an upgrade. ~24 hours at ~5 sec/ledger = 17_280 ledgers.
pub const UPGRADE_TIMELOCK_LEDGERS: u32 = 17_280;
