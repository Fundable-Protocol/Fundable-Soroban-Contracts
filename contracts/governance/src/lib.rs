#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    BytesN, Env, IntoVal, Symbol, Val, Vec,
};

const SIGNER_COUNT: u32 = 5;
const NORMAL_THRESHOLD: u32 = 3;
const EMERGENCY_THRESHOLD: u32 = 4;
const NORMAL_DELAY_SECONDS: u64 = 48 * 60 * 60;
const PROPOSAL_LIFETIME_SECONDS: u64 = 7 * 24 * 60 * 60;
const INSTANCE_TTL_THRESHOLD: u32 = 120_960;
const INSTANCE_TTL_LEDGERS: u32 = 518_400;
const PERSISTENT_TTL_THRESHOLD: u32 = 518_400;
const PERSISTENT_TTL_LEDGERS: u32 = 2_073_600;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GovernanceError {
    InvalidSignerSet = 501,
    NotSigner = 502,
    ProposalNotFound = 503,
    ProposalNotActive = 504,
    AlreadyApproved = 505,
    InsufficientApprovals = 506,
    TimelockActive = 507,
    ProposalExpired = 508,
    AlreadyApprovedCancellation = 509,
    InvalidReasonHash = 510,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernanceAction {
    /// Call `upgrade(new_wasm_hash)` on a governed contract.
    Upgrade(Address, BytesN<32>),
    /// Call `upgrade_nft(new_wasm_hash)` on the governed Router.
    UpgradeNft(Address, BytesN<32>),
    /// Call `set_admin(new_admin)` on a governed contract.
    SetAdmin(Address, Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Active,
    Canceled,
    Executed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub action: GovernanceAction,
    pub approvals: u32,
    pub cancellation_approvals: u32,
    pub created_at: u64,
    pub emergency: bool,
    pub execute_after: u64,
    pub expires_at: u64,
    pub proposer: Address,
    pub reason_hash: BytesN<32>,
    pub status: ProposalStatus,
}

#[contractevent(topics = ["gov_init"])]
pub struct GovernanceInitializedEvent {
    pub signers: Vec<Address>,
}

#[contractevent(topics = ["proposed"])]
pub struct ProposalCreatedEvent {
    #[topic]
    pub proposal_id: u64,
    pub action: GovernanceAction,
    pub emergency: bool,
    pub execute_after: u64,
    pub reason_hash: BytesN<32>,
}

#[contractevent(topics = ["approved"])]
pub struct ProposalApprovedEvent {
    #[topic]
    pub proposal_id: u64,
    pub signer: Address,
    pub approvals: u32,
}

#[contractevent(topics = ["cancel_vote"])]
pub struct CancellationApprovedEvent {
    #[topic]
    pub proposal_id: u64,
    pub signer: Address,
    pub approvals: u32,
}

#[contractevent(topics = ["canceled"])]
pub struct ProposalCanceledEvent {
    #[topic]
    pub proposal_id: u64,
    pub reason_hash: BytesN<32>,
}

#[contractevent(topics = ["executed"])]
pub struct ProposalExecutedEvent {
    #[topic]
    pub proposal_id: u64,
    pub action: GovernanceAction,
    pub emergency: bool,
    pub reason_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Signers,
    NextProposalId,
    Proposal(u64),
    Approval(u64, Address),
    CancellationApproval(u64, Address),
}

#[contract]
pub struct GovernanceContract;

fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
}

fn extend_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_LEDGERS);
}

fn signers(env: &Env) -> Vec<Address> {
    env.storage().instance().get(&DataKey::Signers).unwrap()
}

fn require_signer(env: &Env, signer: &Address) {
    if !signers(env).contains(signer) {
        panic_with_error!(env, GovernanceError::NotSigner);
    }
    signer.require_auth();
}

fn load_proposal(env: &Env, proposal_id: u64) -> Proposal {
    let key = DataKey::Proposal(proposal_id);
    let proposal = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| panic_with_error!(env, GovernanceError::ProposalNotFound));
    extend_persistent_ttl(env, &key);
    proposal
}

fn require_active(env: &Env, proposal: &Proposal) {
    if proposal.status != ProposalStatus::Active {
        panic_with_error!(env, GovernanceError::ProposalNotActive);
    }
    if env.ledger().timestamp() > proposal.expires_at {
        panic_with_error!(env, GovernanceError::ProposalExpired);
    }
}

fn save_proposal(env: &Env, proposal_id: u64, proposal: &Proposal) {
    let key = DataKey::Proposal(proposal_id);
    env.storage().persistent().set(&key, proposal);
    extend_persistent_ttl(env, &key);
}

fn save_vote(env: &Env, key: &DataKey) {
    env.storage().persistent().set(key, &true);
    extend_persistent_ttl(env, key);
}

fn invoke_action(env: &Env, action: &GovernanceAction) {
    match action {
        GovernanceAction::Upgrade(target, wasm_hash) => {
            let args: Vec<Val> = (wasm_hash.clone(),).into_val(env);
            env.invoke_contract::<()>(target, &Symbol::new(env, "upgrade"), args);
        }
        GovernanceAction::UpgradeNft(router, wasm_hash) => {
            let args: Vec<Val> = (wasm_hash.clone(),).into_val(env);
            env.invoke_contract::<()>(router, &Symbol::new(env, "upgrade_nft"), args);
        }
        GovernanceAction::SetAdmin(target, new_admin) => {
            let args: Vec<Val> = (new_admin.clone(),).into_val(env);
            env.invoke_contract::<()>(target, &Symbol::new(env, "set_admin"), args);
        }
    }
}

#[contractimpl]
impl GovernanceContract {
    /// Initialize the immutable five-member signer roster.
    pub fn __constructor(env: Env, signer_set: Vec<Address>) {
        if signer_set.len() != SIGNER_COUNT {
            panic_with_error!(&env, GovernanceError::InvalidSignerSet);
        }
        for i in 0..SIGNER_COUNT {
            for j in (i + 1)..SIGNER_COUNT {
                if signer_set.get(i).unwrap() == signer_set.get(j).unwrap() {
                    panic_with_error!(&env, GovernanceError::InvalidSignerSet);
                }
            }
        }
        env.storage().instance().set(&DataKey::Signers, &signer_set);
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &1_u64);
        extend_instance_ttl(&env);
        GovernanceInitializedEvent {
            signers: signer_set,
        }
        .publish(&env);
    }

    /// Create a normal or emergency proposal. Creation counts as one approval.
    pub fn propose(
        env: Env,
        proposer: Address,
        action: GovernanceAction,
        emergency: bool,
        reason_hash: BytesN<32>,
    ) -> u64 {
        require_signer(&env, &proposer);
        if reason_hash == BytesN::from_array(&env, &[0; 32]) {
            panic_with_error!(&env, GovernanceError::InvalidReasonHash);
        }
        extend_instance_ttl(&env);

        let proposal_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextProposalId)
            .unwrap();
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &(proposal_id + 1));

        let created_at = env.ledger().timestamp();
        let execute_after = if emergency {
            created_at
        } else {
            created_at + NORMAL_DELAY_SECONDS
        };
        let proposal = Proposal {
            action,
            approvals: 1,
            cancellation_approvals: 0,
            created_at,
            emergency,
            execute_after,
            expires_at: created_at + PROPOSAL_LIFETIME_SECONDS,
            proposer: proposer.clone(),
            reason_hash: reason_hash.clone(),
            status: ProposalStatus::Active,
        };
        save_proposal(&env, proposal_id, &proposal);
        save_vote(&env, &DataKey::Approval(proposal_id, proposer));
        ProposalCreatedEvent {
            proposal_id,
            action: proposal.action,
            emergency,
            execute_after,
            reason_hash,
        }
        .publish(&env);
        proposal_id
    }

    /// Add one signer approval to an active proposal.
    pub fn approve(env: Env, signer: Address, proposal_id: u64) {
        require_signer(&env, &signer);
        let mut proposal = load_proposal(&env, proposal_id);
        require_active(&env, &proposal);
        let key = DataKey::Approval(proposal_id, signer.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, GovernanceError::AlreadyApproved);
        }
        save_vote(&env, &key);
        proposal.approvals += 1;
        save_proposal(&env, proposal_id, &proposal);
        ProposalApprovedEvent {
            proposal_id,
            signer,
            approvals: proposal.approvals,
        }
        .publish(&env);
    }

    /// Vote to cancel an active proposal. Three distinct votes cancel it.
    pub fn approve_cancellation(env: Env, signer: Address, proposal_id: u64) {
        require_signer(&env, &signer);
        let mut proposal = load_proposal(&env, proposal_id);
        require_active(&env, &proposal);
        let key = DataKey::CancellationApproval(proposal_id, signer.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, GovernanceError::AlreadyApprovedCancellation);
        }
        save_vote(&env, &key);
        proposal.cancellation_approvals += 1;
        if proposal.cancellation_approvals >= NORMAL_THRESHOLD {
            proposal.status = ProposalStatus::Canceled;
        }
        save_proposal(&env, proposal_id, &proposal);
        CancellationApprovedEvent {
            proposal_id,
            signer,
            approvals: proposal.cancellation_approvals,
        }
        .publish(&env);
        if proposal.status == ProposalStatus::Canceled {
            ProposalCanceledEvent {
                proposal_id,
                reason_hash: proposal.reason_hash,
            }
            .publish(&env);
        }
    }

    /// Execute an approved proposal. Anyone may submit the execution transaction.
    pub fn execute(env: Env, proposal_id: u64) {
        let mut proposal = load_proposal(&env, proposal_id);
        require_active(&env, &proposal);
        let threshold = if proposal.emergency {
            EMERGENCY_THRESHOLD
        } else {
            NORMAL_THRESHOLD
        };
        if proposal.approvals < threshold {
            panic_with_error!(&env, GovernanceError::InsufficientApprovals);
        }
        if env.ledger().timestamp() < proposal.execute_after {
            panic_with_error!(&env, GovernanceError::TimelockActive);
        }

        proposal.status = ProposalStatus::Executed;
        save_proposal(&env, proposal_id, &proposal);
        invoke_action(&env, &proposal.action);
        ProposalExecutedEvent {
            proposal_id,
            action: proposal.action,
            emergency: proposal.emergency,
            reason_hash: proposal.reason_hash,
        }
        .publish(&env);
    }

    pub fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        load_proposal(&env, proposal_id)
    }

    pub fn get_signers(env: Env) -> Vec<Address> {
        extend_instance_ttl(&env);
        signers(&env)
    }

    pub fn normal_threshold(_env: Env) -> u32 {
        NORMAL_THRESHOLD
    }

    pub fn emergency_threshold(_env: Env) -> u32 {
        EMERGENCY_THRESHOLD
    }

    pub fn normal_delay_seconds(_env: Env) -> u64 {
        NORMAL_DELAY_SECONDS
    }
}

#[cfg(test)]
mod test;
