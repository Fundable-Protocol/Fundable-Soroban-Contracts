//! Storage helpers for the Distributor contract.
//!
//! Wraps raw `env.storage()` calls with typed accessors. Uses the shared
//! `DataKey` enum for key construction and enforces TTL extension on every
//! read to prevent state archival (SKILL.md §3).

use shared::storage::{
    DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS,
    PERSISTENT_TTL_THRESHOLD,
};
use shared::types::DistributionRecord;
use soroban_sdk::{Address, Env};

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

/// Store the admin address in Instance storage.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Read the admin address. Panics if not initialized.
pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("not initialized")
}

/// Check if the contract has been initialized (admin is set).
pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

// ---------------------------------------------------------------------------
// Next Distribution ID
// ---------------------------------------------------------------------------

/// Get the next distribution ID. Starts at 1.
pub fn get_next_distribution_id(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::NextDistributionId)
        .unwrap_or(1u32)
}

/// Increment and store the next distribution ID.
pub fn set_next_distribution_id(env: &Env, id: u32) {
    env.storage()
        .instance()
        .set(&DataKey::NextDistributionId, &id);
}

// ---------------------------------------------------------------------------
// Protocol Fee
// ---------------------------------------------------------------------------

/// Get the protocol fee percentage (basis points, 10000 = 100%).
pub fn get_protocol_fee_percent(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolFeePercent)
        .unwrap_or(0u32)
}

/// Set the protocol fee percentage.
pub fn set_protocol_fee_percent(env: &Env, percent: u32) {
    env.storage()
        .instance()
        .set(&DataKey::ProtocolFeePercent, &percent);
}

/// Get the protocol fee address.
pub fn get_protocol_fee_address(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::ProtocolFeeAddress)
}

/// Set the protocol fee address.
pub fn set_protocol_fee_address(env: &Env, address: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::ProtocolFeeAddress, address);
}

// ---------------------------------------------------------------------------
// Distribution Records
// ---------------------------------------------------------------------------

/// Store a distribution record in Persistent storage.
pub fn set_distribution(env: &Env, distribution_id: u32, record: &DistributionRecord) {
    let key = DataKey::Distribution(distribution_id);
    env.storage().persistent().set(&key, record);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

/// Read a distribution record. Returns None if not found.
pub fn get_distribution(env: &Env, distribution_id: u32) -> Option<DistributionRecord> {
    let key = DataKey::Distribution(distribution_id);
    let result: Option<DistributionRecord> = env.storage().persistent().get(&key);
    if result.is_some() {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );
    }
    result
}

// ---------------------------------------------------------------------------
// Claim Tracking
// ---------------------------------------------------------------------------

/// Mark a user as having claimed from a distribution.
pub fn set_claimed(env: &Env, distribution_id: u32, claimant: &Address) {
    let key = DataKey::Claimed(distribution_id, claimant.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

/// Check if a user has already claimed from a distribution.
pub fn has_claimed(env: &Env, distribution_id: u32, claimant: &Address) -> bool {
    let key = DataKey::Claimed(distribution_id, claimant.clone());
    let result = env.storage().persistent().get(&key).unwrap_or(false);
    if result {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );
    }
    result
}

// ---------------------------------------------------------------------------
// TTL Extension
// ---------------------------------------------------------------------------

/// Extend the Instance storage TTL. Call this on every public entry point
/// to keep the contract configuration alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
}
