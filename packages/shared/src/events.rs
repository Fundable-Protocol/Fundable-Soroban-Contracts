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
pub fn emit_flow_deposit(env: &Env, stream_id: u64, funder: &Address, amount: i128) {
    let topics = (Symbol::new(env, "flow_deposit"), stream_id);
    let data = (funder.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when tokens are withdrawn from a Flow stream.
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
pub fn emit_flow_refunded(env: &Env, stream_id: u64, sender: &Address, amount: i128) {
    let topics = (Symbol::new(env, "flow_refunded"), stream_id);
    let data = (sender.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when a Flow stream is permanently voided.
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

// ---------------------------------------------------------------------------
// Lockup Events
// ---------------------------------------------------------------------------

/// Emit when a new Lockup stream is created.
pub fn emit_lockup_created(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    token: &Address,
    total_amount: i128,
    start_time: u64,
    end_time: u64,
    cliff_time: u64,
    cancelable: bool,
) {
    let topics = (Symbol::new(env, "lockup_created"), stream_id);
    let data = (
        sender.clone(),
        recipient.clone(),
        token.clone(),
        total_amount,
        (start_time, end_time, cliff_time, cancelable),
    );
    env.events().publish(topics, data);
}

/// Emit when tokens are withdrawn from a Lockup stream.
pub fn emit_lockup_withdraw(
    env: &Env,
    stream_id: u64,
    to: &Address,
    caller: &Address,
    amount: i128,
) {
    let topics = (Symbol::new(env, "lockup_withdraw"), stream_id);
    let data = (to.clone(), caller.clone(), amount);
    env.events().publish(topics, data);
}

/// Emit when a Lockup stream is canceled.
pub fn emit_lockup_canceled(
    env: &Env,
    stream_id: u64,
    sender: &Address,
    recipient: &Address,
    sender_amount: i128,
    recipient_amount: i128,
) {
    let topics = (Symbol::new(env, "lockup_canceled"), stream_id);
    let data = (
        sender.clone(),
        recipient.clone(),
        sender_amount,
        recipient_amount,
    );
    env.events().publish(topics, data);
}

/// Emit when a Lockup stream's cancelability is renounced.
pub fn emit_lockup_renounced(env: &Env, stream_id: u64) {
    let topics = (Symbol::new(env, "lockup_renounced"), stream_id);
    env.events().publish(topics, ());
}

// ---------------------------------------------------------------------------
// NFT Events
// ---------------------------------------------------------------------------

/// Emit when an NFT is transferred (or minted/burned).
pub fn emit_nft_transfer(
    env: &Env,
    from: &Address,
    to: &Address,
    token_id: i128,
) {
    let topics = (Symbol::new(env, "transfer"), from.clone(), to.clone());
    env.events().publish(topics, token_id);
}

// ---------------------------------------------------------------------------
// Admin Events
// ---------------------------------------------------------------------------

/// Emit when a contract admin is initialized.
pub fn emit_admin_initialized(env: &Env, admin: &Address) {
    let topics = (Symbol::new(env, "admin_initialized"),);
    env.events().publish(topics, admin.clone());
}

/// Emit when admin rights are transferred.
pub fn emit_admin_transferred(env: &Env, old_admin: &Address, new_admin: &Address) {
    let topics = (Symbol::new(env, "admin_transferred"),);
    env.events().publish(topics, (old_admin.clone(), new_admin.clone()));
}

