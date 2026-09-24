//! Fundable Distributor Contract — Merkle-based token distribution.
//!
//! # Overview
//!
//! The Distributor allows an admin to create token distributions using
//! Merkle trees. Instead of pushing tokens to recipients (which fails on
//! Stellar if any recipient is missing a trustline), users pull (claim)
//! their allocation by providing a Merkle proof.
//!
//! # Flow
//!
//! 1. Admin calls `create_distribution()` with a Merkle root, token, amount,
//!    and optional deadline. Tokens are transferred from the admin into the
//!    contract.
//! 2. Users call `claim()` with their amount and Merkle proof. The contract
//!    verifies the proof, marks the claim, deducts protocol fees, and
//!    transfers tokens to the claimant.
//! 3. Admin can cancel unclaimed distributions to reclaim remaining tokens.
//!
//! # Design Decisions
//!
//! - **Pull pattern** solves the Stellar trustline problem: users add their
//!   trustline before claiming, so transfers never fail.
//! - **Events replace on-chain history**: distribution stats are tracked via
//!   events rather than expensive on-chain storage (Soroban state rent).
//! - **Protocol fees** are collected on each claim (basis points, same as
//!   the Cairo Distributor).
//! - Admin + upgrade support from day one (SKILL.md §7).
//!
//! # Security
//!
//! - All privileged functions use `require_auth()` (SKILL.md §1).
//! - Checked arithmetic via workspace `overflow-checks = true` (SKILL.md §2).
//! - Events emitted on every state change (SKILL.md §8).
//! - TTL extended on every storage access (SKILL.md §3).
//! - Two-step admin transfer prevents accidental transfer to unusable address.
//! - Timelocked upgrades provide mainnet safety with emergency override.

#![no_std]
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Bytes, BytesN, Env, Vec};

use shared::errors::DistributorError;
use shared::events;
use shared::storage::{DataKey, UPGRADE_TIMELOCK_LEDGERS};
use shared::types::DistributionRecord;

mod merkle;
mod storage;

#[cfg(test)]
mod test;

#[contract]
pub struct DistributorContract;

/// Public API for the Fundable Distributor contract.
///
/// Functions are organized into:
/// 1. **Admin** — initialize, upgrade, set_admin, propose_admin, accept_admin
/// 2. **Fee Management** — set_protocol_fee_percent, set_protocol_fee_address
/// 3. **Distribution Management** — create_distribution, cancel_distribution
/// 4. **Claims** — claim
/// 5. **Query** — get_distribution, has_claimed, get_protocol_fee_percent, etc.
#[contractimpl]
impl DistributorContract {
    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Must be called exactly once before any other function.
    /// The intended admin must authorize the initialization to prevent
    /// first-caller takeover attacks.
    pub fn initialize(env: Env, admin: Address, protocol_fee_address: Address) {
        if storage::has_admin(&env) {
            panic_with_error!(&env, DistributorError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_protocol_fee_address(&env, &protocol_fee_address);
        storage::extend_instance_ttl(&env);
        events::emit_admin_initialized(&env, &admin);
    }

    /// Propose a timelocked upgrade. The upgrade can be executed after
    /// UPGRADE_TIMELOCK_LEDGERS have passed.
    ///
    /// Admin-only.
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let unlock_ledger = env
            .ledger()
            .sequence()
            .checked_add(UPGRADE_TIMELOCK_LEDGERS)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::ArithmeticError));

        env.storage()
            .instance()
            .set(&DataKey::UpgradeUnlockLedger, &unlock_ledger);
        env.storage()
            .instance()
            .set(&DataKey::ProposedUpgrade(new_wasm_hash.clone()), &true);
        storage::extend_instance_ttl(&env);

        events::emit_upgrade_proposed(&env, &admin, &new_wasm_hash, unlock_ledger);
    }

    /// Execute a previously proposed upgrade after the timelock has expired.
    ///
    /// Admin-only.
    pub fn execute_upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let proposed: bool = env
            .storage()
            .instance()
            .get(&DataKey::ProposedUpgrade(new_wasm_hash.clone()))
            .unwrap_or(false);
        if !proposed {
            panic_with_error!(&env, DistributorError::NoUpgradeProposed);
        }

        let unlock_ledger: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeUnlockLedger)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::NoUpgradeProposed));

        if env.ledger().sequence() < unlock_ledger {
            panic_with_error!(&env, DistributorError::UpgradeTimelocked);
        }

        env.storage()
            .instance()
            .remove(&DataKey::ProposedUpgrade(new_wasm_hash.clone()));
        env.storage()
            .instance()
            .remove(&DataKey::UpgradeUnlockLedger);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::extend_instance_ttl(&env);

        events::emit_upgrade_executed(&env, &admin, &new_wasm_hash);
    }

    /// Emergency upgrade — bypasses timelock. Use only in critical situations.
    ///
    /// Admin-only.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        storage::extend_instance_ttl(&env);
        events::emit_upgrade_executed(&env, &admin, &new_wasm_hash);
    }

    /// Propose a new admin (two-step transfer, step 1).
    ///
    /// Admin-only.
    pub fn propose_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_proposed(&env, &admin, &new_admin);
    }

    /// Accept an admin transfer (two-step transfer, step 2).
    ///
    /// Must be called by the address that was proposed via `propose_admin()`.
    pub fn accept_admin(env: Env) {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::NoAdminTransferPending));
        pending.require_auth();

        let old_admin = storage::get_admin(&env);
        storage::set_admin(&env, &pending);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_accepted(&env, &old_admin, &pending);
    }

    /// Direct admin transfer (legacy, kept for backwards compatibility).
    ///
    /// Admin-only. Prefer `propose_admin` + `accept_admin` for safety.
    pub fn set_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_admin(&env, &new_admin);
        storage::extend_instance_ttl(&env);
        events::emit_admin_transferred(&env, &admin, &new_admin);
    }

    // -----------------------------------------------------------------------
    // Fee Management
    // -----------------------------------------------------------------------

    /// Set the protocol fee percentage (in basis points, max 10000).
    ///
    /// Admin-only.
    pub fn set_protocol_fee_percent(env: Env, new_fee_percent: u32) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        if new_fee_percent > 10000 {
            panic_with_error!(&env, DistributorError::InvalidFeePercent);
        }
        storage::set_protocol_fee_percent(&env, new_fee_percent);
        storage::extend_instance_ttl(&env);
        events::emit_fee_updated(&env, &admin, new_fee_percent);
    }

    /// Set the protocol fee collection address.
    ///
    /// Admin-only.
    pub fn set_protocol_fee_address(env: Env, new_fee_address: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_protocol_fee_address(&env, &new_fee_address);
        storage::extend_instance_ttl(&env);
    }

    // -----------------------------------------------------------------------
    // Distribution Management
    // -----------------------------------------------------------------------

    /// Create a new Merkle distribution.
    ///
    /// Transfers `total_amount` of `token` from the caller into this contract.
    /// The `merkle_root` represents the tree of (claimant, amount) leaves.
    /// An optional `deadline` (unix timestamp) can be set — after which no
    /// more claims are accepted (0 = no deadline).
    ///
    /// # Arguments
    /// * `admin` — The admin creating the distribution (must authorize).
    /// * `token` — Token contract address to distribute.
    /// * `merkle_root` — Root hash of the Merkle tree.
    /// * `total_amount` — Total tokens to deposit into the distribution.
    /// * `deadline` — Unix timestamp deadline (0 = no deadline).
    /// * `unique_ref` — A unique reference identifier for this distribution.
    ///
    /// # Returns
    /// The newly assigned distribution ID.
    pub fn create_distribution(
        env: Env,
        admin: Address,
        token: Address,
        merkle_root: BytesN<32>,
        total_amount: i128,
        deadline: u64,
        unique_ref: Bytes,
    ) -> u32 {
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        if total_amount <= 0 {
            panic_with_error!(&env, DistributorError::AmountZero);
        }

        // Transfer tokens from user to this contract
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&admin, env.current_contract_address(), &total_amount);

        // Create the distribution record
        let distribution_id = storage::get_next_distribution_id(&env);
        let record = DistributionRecord {
            admin: admin.clone(),
            token: token.clone(),
            merkle_root,
            total_amount,
            claimed_amount: 0,
            deadline,
            is_cancelled: false,
            unique_ref,
        };

        storage::set_distribution(&env, distribution_id, &record);
        storage::set_next_distribution_id(&env, distribution_id + 1);

        events::emit_distribution_created(
            &env,
            distribution_id,
            &admin,
            &token,
            total_amount,
            deadline,
        );

        distribution_id
    }

    /// Cancel a distribution and reclaim remaining (unclaimed) tokens.
    ///
    /// The distribution creator (or contract admin) can cancel. Prevents further claims.
    pub fn cancel_distribution(env: Env, distribution_id: u32) {
        storage::extend_instance_ttl(&env);

        let mut record = storage::get_distribution(&env, distribution_id)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::DistributionNotFound));

        // The creator of the distribution must authorize the cancellation
        record.admin.require_auth();

        if record.is_cancelled {
            panic_with_error!(&env, DistributorError::DistributionCancelled);
        }

        // Calculate remaining tokens
        let remaining = record
            .total_amount
            .checked_sub(record.claimed_amount)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::ArithmeticError));

        // Transfer remaining tokens back to distribution creator
        if remaining > 0 {
            let token_client = soroban_sdk::token::Client::new(&env, &record.token);
            token_client.transfer(&env.current_contract_address(), &record.admin, &remaining);
        }

        // Mark as cancelled
        record.is_cancelled = true;
        storage::set_distribution(&env, distribution_id, &record);

        events::emit_distribution_cancelled(&env, distribution_id, &record.admin, remaining);
    }

    // -----------------------------------------------------------------------
    // Claims
    // -----------------------------------------------------------------------

    /// Claim tokens from a distribution by providing a Merkle proof.
    ///
    /// The claimant must authorize the call. The Merkle leaf is computed as
    /// `keccak256(claimant || amount)` and verified against the stored root.
    ///
    /// Protocol fees are deducted from the claimed amount.
    ///
    /// # Arguments
    /// * `claimant` — The address claiming tokens (must authorize).
    /// * `distribution_id` — ID of the distribution to claim from.
    /// * `amount` — The amount the claimant is entitled to (per the Merkle tree).
    /// * `proof` — The Merkle proof (list of sibling hashes).
    pub fn claim(
        env: Env,
        claimant: Address,
        distribution_id: u32,
        amount: i128,
        proof: Vec<BytesN<32>>,
    ) {
        claimant.require_auth();
        storage::extend_instance_ttl(&env);

        // Load distribution
        let mut record = storage::get_distribution(&env, distribution_id)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::DistributionNotFound));

        // Check not cancelled
        if record.is_cancelled {
            panic_with_error!(&env, DistributorError::DistributionCancelled);
        }

        // Check deadline
        if record.deadline > 0 && env.ledger().timestamp() > record.deadline {
            panic_with_error!(&env, DistributorError::DistributionExpired);
        }

        // Check not already claimed
        if storage::has_claimed(&env, distribution_id, &claimant) {
            panic_with_error!(&env, DistributorError::AlreadyClaimed);
        }

        if amount <= 0 {
            panic_with_error!(&env, DistributorError::AmountZero);
        }

        // Verify Merkle proof
        let leaf = merkle::compute_leaf(&env, &claimant, amount);
        if !merkle::verify_proof(&env, &proof, &record.merkle_root, &leaf) {
            panic_with_error!(&env, DistributorError::InvalidProof);
        }

        // Keep every distribution solvent independently. Token balances are
        // held by the contract in aggregate, so without this bound an
        // over-allocated Merkle tree could consume funds deposited for a
        // different distribution of the same token.
        let new_claimed_amount = record
            .claimed_amount
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::ArithmeticError));
        if new_claimed_amount > record.total_amount {
            panic_with_error!(&env, DistributorError::DistributionExhausted);
        }

        // Calculate protocol fee
        let fee_percent = storage::get_protocol_fee_percent(&env) as i128;
        let protocol_fee = amount
            .checked_mul(fee_percent)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::ArithmeticError))
            / 10000i128;
        let claim_amount = amount
            .checked_sub(protocol_fee)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::ArithmeticError));

        // Commit claim bookkeeping before external token calls. Soroban rolls
        // the transaction back atomically if either transfer fails.
        record.claimed_amount = new_claimed_amount;
        storage::set_distribution(&env, distribution_id, &record);
        storage::set_claimed(&env, distribution_id, &claimant);

        // Transfer protocol fee
        let token_client = soroban_sdk::token::Client::new(&env, &record.token);
        if protocol_fee > 0 {
            let fee_address = storage::get_protocol_fee_address(&env)
                .unwrap_or_else(|| panic_with_error!(&env, DistributorError::FeeAddressNotSet));
            token_client.transfer(&env.current_contract_address(), &fee_address, &protocol_fee);
        }

        // Transfer tokens to claimant
        token_client.transfer(&env.current_contract_address(), &claimant, &claim_amount);

        events::emit_claim(&env, distribution_id, &claimant, amount);
    }

    // -----------------------------------------------------------------------
    // Read-Only Queries
    // -----------------------------------------------------------------------

    /// Get the full distribution record.
    pub fn get_distribution(env: Env, distribution_id: u32) -> DistributionRecord {
        storage::extend_instance_ttl(&env);
        storage::get_distribution(&env, distribution_id)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::DistributionNotFound))
    }

    /// Check if a user has already claimed from a distribution.
    pub fn has_claimed(env: Env, distribution_id: u32, claimant: Address) -> bool {
        storage::extend_instance_ttl(&env);
        storage::has_claimed(&env, distribution_id, &claimant)
    }

    /// Get the protocol fee percentage (basis points).
    pub fn get_protocol_fee_percent(env: Env) -> u32 {
        storage::extend_instance_ttl(&env);
        storage::get_protocol_fee_percent(&env)
    }

    /// Get the protocol fee address.
    pub fn get_protocol_fee_address(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_protocol_fee_address(&env)
            .unwrap_or_else(|| panic_with_error!(&env, DistributorError::FeeAddressNotSet))
    }
}
