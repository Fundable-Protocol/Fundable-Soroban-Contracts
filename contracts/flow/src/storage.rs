//! Storage helpers for the Flow contract.
//!
//! Wraps raw `env.storage()` calls with typed accessors. Uses the shared
//! `DataKey` enum for key construction and enforces TTL extension on every
//! read to prevent state archival (SKILL.md §3).

use shared::storage::{
    DataKey, INSTANCE_TTL_LEDGERS, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS,
    PERSISTENT_TTL_THRESHOLD,
};
use shared::types::FlowStream;
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

/// Get the next stream ID. Starts at 1
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
// Flow Stream Records
// ---------------------------------------------------------------------------

/// Store a Flow stream record in Persistent storage.
pub fn set_stream(env: &Env, stream_id: u64, stream: &FlowStream) {
    let key = DataKey::FlowStream(stream_id);
    env.storage().persistent().set(&key, stream);
    // Extend TTL on write to prevent archival
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

/// Read a Flow stream record. Returns None if not found.
pub fn get_stream(env: &Env, stream_id: u64) -> Option<FlowStream> {
    let key = DataKey::FlowStream(stream_id);
    let result: Option<FlowStream> = env.storage().persistent().get(&key);
    if result.is_some() {
        // Extend TTL on read to keep active streams alive
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
    }
    result
}

/// Check if a stream exists.
pub fn has_stream(env: &Env, stream_id: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::FlowStream(stream_id))
}

// ---------------------------------------------------------------------------
// Aggregate Balance
// ---------------------------------------------------------------------------

/// Get the aggregate balance held for a specific token.
/// Returns 0 if no balance has been recorded.
pub fn get_aggregate_balance(env: &Env, token: &Address) -> i128 {
    let key = DataKey::AggregateBalance(token.clone());
    env.storage().persistent().get(&key).unwrap_or(0i128)
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

/// Extend the Instance storage TTL. Call this on every public entry point
/// to keep the contract configuration alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
}
