//! Fundable Shared Library
//!
//! Common types, errors, math utilities, event helpers, and storage
//! definitions shared across all Fundable streaming protocol contracts
//! (Flow, Lockup, Router, Stream NFT).
//!
//! This crate is `no_std` compatible for Soroban WASM compilation.

#![no_std]

pub mod errors;
pub mod events;
pub mod math;
pub mod storage;
pub mod types;
