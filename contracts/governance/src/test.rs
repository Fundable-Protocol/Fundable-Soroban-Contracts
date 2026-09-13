extern crate std;

use super::*;
use router::{RouterContract, RouterContractArgs, RouterContractClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger as _},
    IntoVal, Symbol, Val,
};

#[contracttype]
#[derive(Clone)]
enum TargetKey {
    Admin,
    WasmHash,
}

#[contract]
struct GovernedTarget;

#[contractimpl]
impl GovernedTarget {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&TargetKey::Admin, &admin);
    }

    pub fn upgrade(env: Env, wasm_hash: BytesN<32>) {
        let admin: Address = env.storage().instance().get(&TargetKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&TargetKey::WasmHash, &wasm_hash);
    }

    pub fn set_admin(env: Env, new_admin: Address) {
        let admin: Address = env.storage().instance().get(&TargetKey::Admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&TargetKey::Admin, &new_admin);
    }

    pub fn wasm_hash(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&TargetKey::WasmHash)
    }

    pub fn admin(env: Env) -> Address {
        env.storage().instance().get(&TargetKey::Admin).unwrap()
    }
}

fn setup(env: &Env) -> (Address, GovernanceContractClient<'_>, Vec<Address>) {
    env.ledger().set_protocol_version(25);
    env.mock_all_auths();
    let signers = Vec::from_array(
        env,
        [
            Address::generate(env),
            Address::generate(env),
            Address::generate(env),
            Address::generate(env),
            Address::generate(env),
        ],
    );
    let governance_id = env.register(
        GovernanceContract,
        GovernanceContractArgs::__constructor(&signers),
    );
    let client = GovernanceContractClient::new(env, &governance_id);
    (governance_id, client, signers)
}

fn reason_hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().set(soroban_sdk::testutils::LedgerInfo {
        timestamp,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
}

fn invocation(
    env: &Env,
    contract: &Address,
    function: &str,
    args: Vec<Val>,
) -> AuthorizedInvocation {
    AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            contract.clone(),
            Symbol::new(env, function),
            args,
        )),
        sub_invocations: std::vec![],
    }
}

#[test]
fn exact_authorization_trees_for_governance_votes() {
    let env = Env::default();
    set_time(&env, 500);
    let (governance_id, governance, signers) = setup(&env);
    let target = Address::generate(&env);
    let action = GovernanceAction::Upgrade(target, reason_hash(&env, 21));
    let reason = reason_hash(&env, 22);

    let proposal_id = governance.propose(&signers.get(0).unwrap(), &action, &false, &reason);
    assert_eq!(
        env.auths(),
        std::vec![(
            signers.get(0).unwrap(),
            invocation(
                &env,
                &governance_id,
                "propose",
                (signers.get(0).unwrap(), action, false, reason.clone(),).into_val(&env),
            ),
        )]
    );

    governance.approve(&signers.get(1).unwrap(), &proposal_id);
    assert_eq!(
        env.auths(),
        std::vec![(
            signers.get(1).unwrap(),
            invocation(
                &env,
                &governance_id,
                "approve",
                (signers.get(1).unwrap(), proposal_id).into_val(&env),
            ),
        )]
    );

    governance.approve_cancellation(&signers.get(2).unwrap(), &proposal_id);
    assert_eq!(
        env.auths(),
        std::vec![(
            signers.get(2).unwrap(),
            invocation(
                &env,
                &governance_id,
                "approve_cancellation",
                (signers.get(2).unwrap(), proposal_id).into_val(&env),
            ),
        )]
    );
}

#[test]
fn normal_upgrade_requires_three_approvals_and_48_hours() {
    let env = Env::default();
    set_time(&env, 1_000);
    let (governance_id, governance, signers) = setup(&env);
    let target_id = env.register(
        GovernedTarget,
        GovernedTargetArgs::__constructor(&governance_id),
    );
    let target = GovernedTargetClient::new(&env, &target_id);
    let hash = reason_hash(&env, 9);

    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::Upgrade(target_id, hash.clone()),
        &false,
        &reason_hash(&env, 1),
    );
    governance.approve(&signers.get(1).unwrap(), &proposal_id);
    assert!(governance.try_execute(&proposal_id).is_err());
    governance.approve(&signers.get(2).unwrap(), &proposal_id);
    assert!(governance.try_execute(&proposal_id).is_err());

    set_time(&env, 1_000 + NORMAL_DELAY_SECONDS);
    governance.execute(&proposal_id);
    // Execution is permissionless; the Governance contract authenticates as
    // the immediate contract invoker on the governed target.
    assert!(env.auths().is_empty());
    assert_eq!(target.wasm_hash(), Some(hash));
    assert_eq!(
        governance.get_proposal(&proposal_id).status,
        ProposalStatus::Executed
    );
    assert!(governance.try_execute(&proposal_id).is_err());
}

#[test]
fn emergency_upgrade_requires_four_approvals_without_delay() {
    let env = Env::default();
    set_time(&env, 5_000);
    let (governance_id, governance, signers) = setup(&env);
    let target_id = env.register(
        GovernedTarget,
        GovernedTargetArgs::__constructor(&governance_id),
    );
    let target = GovernedTargetClient::new(&env, &target_id);
    let hash = reason_hash(&env, 8);

    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::Upgrade(target_id, hash.clone()),
        &true,
        &reason_hash(&env, 2),
    );
    governance.approve(&signers.get(1).unwrap(), &proposal_id);
    governance.approve(&signers.get(2).unwrap(), &proposal_id);
    assert!(governance.try_execute(&proposal_id).is_err());
    governance.approve(&signers.get(3).unwrap(), &proposal_id);
    governance.execute(&proposal_id);
    assert_eq!(target.wasm_hash(), Some(hash));
}

#[test]
fn three_signers_can_cancel_and_replay_is_rejected() {
    let env = Env::default();
    set_time(&env, 10_000);
    let (governance_id, governance, signers) = setup(&env);
    let target_id = env.register(
        GovernedTarget,
        GovernedTargetArgs::__constructor(&governance_id),
    );
    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::Upgrade(target_id, reason_hash(&env, 7)),
        &false,
        &reason_hash(&env, 3),
    );

    for i in 0..3 {
        governance.approve_cancellation(&signers.get(i).unwrap(), &proposal_id);
    }
    assert_eq!(
        governance.get_proposal(&proposal_id).status,
        ProposalStatus::Canceled
    );
    assert!(governance.try_execute(&proposal_id).is_err());
    assert!(governance
        .try_approve_cancellation(&signers.get(2).unwrap(), &proposal_id)
        .is_err());
}

#[test]
fn governance_can_rotate_target_admin() {
    let env = Env::default();
    set_time(&env, 20_000);
    let (governance_id, governance, signers) = setup(&env);
    let target_id = env.register(
        GovernedTarget,
        GovernedTargetArgs::__constructor(&governance_id),
    );
    let target = GovernedTargetClient::new(&env, &target_id);
    let new_admin = Address::generate(&env);
    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::SetAdmin(target_id, new_admin.clone()),
        &false,
        &reason_hash(&env, 4),
    );
    governance.approve(&signers.get(1).unwrap(), &proposal_id);
    governance.approve(&signers.get(2).unwrap(), &proposal_id);
    set_time(&env, 20_000 + NORMAL_DELAY_SECONDS);
    governance.execute(&proposal_id);
    assert_eq!(target.admin(), new_admin);
}

#[test]
fn governance_rotates_real_router_admin_after_timelock() {
    let env = Env::default();
    set_time(&env, 25_000);
    let (governance_id, governance, signers) = setup(&env);
    let router_id = env.register(
        RouterContract,
        RouterContractArgs::__constructor(&governance_id),
    );
    let router = RouterContractClient::new(&env, &router_id);
    let new_admin = Address::generate(&env);

    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::SetAdmin(router_id, new_admin.clone()),
        &false,
        &reason_hash(&env, 11),
    );
    governance.approve(&signers.get(1).unwrap(), &proposal_id);
    governance.approve(&signers.get(2).unwrap(), &proposal_id);
    set_time(&env, 25_000 + NORMAL_DELAY_SECONDS);
    governance.execute(&proposal_id);

    router.configure(
        &Address::generate(&env),
        &Address::generate(&env),
        &Address::generate(&env),
    );
    assert_eq!(env.auths()[0].0, new_admin);
}

#[test]
fn rejects_non_signers_duplicate_approvals_and_expired_proposals() {
    let env = Env::default();
    set_time(&env, 30_000);
    let (_governance_id, governance, signers) = setup(&env);
    let target = Address::generate(&env);
    let proposal_id = governance.propose(
        &signers.get(0).unwrap(),
        &GovernanceAction::Upgrade(target, reason_hash(&env, 6)),
        &false,
        &reason_hash(&env, 5),
    );

    assert!(governance
        .try_approve(&Address::generate(&env), &proposal_id)
        .is_err());
    assert!(governance
        .try_approve(&signers.get(0).unwrap(), &proposal_id)
        .is_err());
    set_time(&env, 30_000 + PROPOSAL_LIFETIME_SECONDS + 1);
    assert!(governance
        .try_approve(&signers.get(1).unwrap(), &proposal_id)
        .is_err());

    assert!(governance
        .try_propose(
            &signers.get(0).unwrap(),
            &GovernanceAction::Upgrade(Address::generate(&env), reason_hash(&env, 1)),
            &true,
            &BytesN::from_array(&env, &[0; 32]),
        )
        .is_err());
}
