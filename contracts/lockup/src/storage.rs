//! Storage helpers for the Lockup contract.
//!
//! Wraps raw `env.storage()` calls with typed accessors. Uses the shared
//! `DataKey` enum for key construction and enforces TTL extension on every
//! read to prevent state archival (SKILL.md §3).

use shared::storage::{
    DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS,
    PERSISTENT_TTL_THRESHOLD,
};
use shared::types::LockupStream;
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
// Next Stream ID
// ---------------------------------------------------------------------------

/// Get the next stream ID. Starts at 1.
pub fn get_next_stream_id(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::NextStreamId)
        .unwrap_or(1u64)
}

/// Increment and store the next stream ID.
pub fn set_next_stream_id(env: &Env, id: u64) {
    env.storage().instance().set(&DataKey::NextStreamId, &id);
}

// ---------------------------------------------------------------------------
// Lockup Stream Records
// ---------------------------------------------------------------------------

/// Store a Lockup stream record in Persistent storage.
pub fn set_stream(env: &Env, stream_id: u64, stream: &LockupStream) {
    let key = DataKey::LockupStream(stream_id);
    env.storage().persistent().set(&key, stream);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

/// Read a Lockup stream record. Returns None if not found.
pub fn get_stream(env: &Env, stream_id: u64) -> Option<LockupStream> {
    let key = DataKey::LockupStream(stream_id);
    let result: Option<LockupStream> = env.storage().persistent().get(&key);
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
// Aggregate Balance
// ---------------------------------------------------------------------------

/// Get the aggregate balance held for a specific token.
pub fn get_aggregate_balance(env: &Env, token: &Address) -> i128 {
    let key = DataKey::AggregateBalance(token.clone());
    let result = env.storage().persistent().get(&key).unwrap_or(0i128);
    if result != 0 {
        // Extend TTL on read to prevent archival (M-1)
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_TTL_THRESHOLD,
            PERSISTENT_TTL_LEDGERS,
        );
    }
    result
}

/// Set the aggregate balance for a specific token.
pub fn set_aggregate_balance(env: &Env, token: &Address, amount: i128) {
    let key = DataKey::AggregateBalance(token.clone());
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

// ---------------------------------------------------------------------------
// TTL Extension
// ---------------------------------------------------------------------------

/// Extend the Instance storage TTL. Call on every public entry point.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
}
