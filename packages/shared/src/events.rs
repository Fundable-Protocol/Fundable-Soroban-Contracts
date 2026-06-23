//! Event emission helpers for the Fundable streaming protocol.
//!
//! All state-changing operations emit events per SKILL.md §8.
//! Events use structured topics for off-chain indexer consumption:
//! `("event_name", stream_id)` as topic, with data as the event payload.
//!
//! Soroban events are emitted via `env.events().publish(topics, data)`.
//! Topics are limited to 4 elements and data must be a single value.

use soroban_sdk::{Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Flow Events
// ---------------------------------------------------------------------------

/// Emit when a new Flow stream is created.
///
/// Sablier equivalent: `CreateFlowStream` event.
pub fn emit_flow_created(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    token: &Address,
    rate_per_second: i128,
    snapshot_time: u64,
) {
    let topics = (Symbol::new(env, "flow_created"), stream_id);
    let data = (
        sender.clone(),
        recipient.clone(),
        token.clone(),
        rate_per_second,
        snapshot_time,
    );
    env.events().publish(topics, data);
}

/// Emit when tokens are deposited into a Flow stream.
///
/// Sablier equivalent: `DepositFlowStream` event.
pub fn emit_flow_deposit(env: &Env, stream_id: u64, funder: &Address, amount: i128) {
    let topics = (Symbol::new(env, "flow_deposit"), stream_id);
    let data = (funder.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when tokens are withdrawn from a Flow stream.
///
/// Sablier equivalent: `WithdrawFromFlowStream` event.
pub fn emit_flow_withdraw(
    env: &Env,
    stream_id: u64,
    to: &Address,
    caller: &Address,
    amount: i128,
) {
    let topics = (Symbol::new(env, "flow_withdraw"), stream_id);
    let data = (to.clone(), caller.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when a Flow stream is paused.
///
/// Sablier equivalent: `PauseFlowStream` event.
pub fn emit_flow_paused(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    total_debt: i128,
) {
    let topics = (Symbol::new(env, "flow_paused"), stream_id);
    let data = (sender.clone(), recipient.clone(), total_debt);
    env.events().publish(topics, data);
}

/// Emit when a paused Flow stream is restarted.
///
/// Sablier equivalent: `RestartFlowStream` event.
pub fn emit_flow_restarted(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    rate_per_second: i128,
) {
    let topics = (Symbol::new(env, "flow_restarted"), stream_id);
    let data = (sender.clone(), rate_per_second);
    env.events().publish(topics, data);
}

/// Emit when the rate per second of a Flow stream is adjusted.
///
/// Sablier equivalent: `AdjustFlowStream` event.
pub fn emit_flow_adjusted(
    env: &Env,
    stream_id: u64,
    total_debt: i128,
    old_rate: i128,
    new_rate: i128,
) {
    let topics = (Symbol::new(env, "flow_adjusted"), stream_id);
    let data = (total_debt, old_rate, new_rate);
    env.events().publish(topics, data);
}

/// Emit when excess balance is refunded from a Flow stream to the sender.
///
/// Sablier equivalent: `RefundFromFlowStream` event.
pub fn emit_flow_refunded(env: &Env, stream_id: u64, sender: &Address, amount: i128) {
    let topics = (Symbol::new(env, "flow_refunded"), stream_id);
    let data = (sender.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when a Flow stream is permanently voided.
///
/// Sablier equivalent: `VoidFlowStream` event.
pub fn emit_flow_voided(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    caller: &Address,
    new_total_debt: i128,
    written_off_debt: i128,
) {
    let topics = (Symbol::new(env, "flow_voided"), stream_id);
    let data = (
        sender.clone(),
        recipient.clone(),
        caller.clone(),
        new_total_debt,
        written_off_debt,
    );
    env.events().publish(topics, data);
}
