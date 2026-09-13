#![no_main]

use flow::{FlowContract, FlowContractArgs, FlowContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig, Ledger, LedgerInfo},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

const BASE_TIME: u64 = 1_000;
const INITIAL_SUPPLY: i128 = 100_000_000;
const MAX_AMOUNT: i128 = 1_000_000;
const MAX_RATE: i128 = 100_000_000_000_000_000_000;

struct Input<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Input<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn byte(&mut self) -> u8 {
        let value = self.bytes.get(self.cursor).copied().unwrap_or(0);
        self.cursor = self.cursor.saturating_add(1);
        value
    }

    fn u64(&mut self) -> u64 {
        let mut value = 0_u64;
        for shift in (0..64).step_by(8) {
            value |= u64::from(self.byte()) << shift;
        }
        value
    }

    fn positive_amount(&mut self) -> i128 {
        i128::from(self.u64() % MAX_AMOUNT as u64) + 1
    }

    fn adversarial_amount(&mut self) -> i128 {
        let magnitude = i128::from(self.u64() % (MAX_AMOUNT as u64 + 1));
        match self.byte() % 4 {
            0 => 0,
            1 => -magnitude,
            2 => magnitude,
            _ => magnitude + MAX_AMOUNT,
        }
    }
}

fn setup() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.ledger().set(LedgerInfo {
        timestamp: BASE_TIME,
        protocol_version: 25,
        sequence_number: 100,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 100,
        min_persistent_entry_ttl: 100,
        max_entry_ttl: 10_000_000,
    });
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let outsider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract_v2(token_admin);
    StellarAssetClient::new(&env, &asset.address()).mint(&sender, &INITIAL_SUPPLY);
    let contract = env.register(FlowContract, FlowContractArgs::__constructor(&admin));

    (env, contract, sender, recipient, outsider, asset.address())
}

fn assert_invariants(
    env: &Env,
    contract: &Address,
    token: &Address,
    sender: &Address,
    recipient: &Address,
    outsider: &Address,
    stream_id: u64,
) {
    let client = FlowContractClient::new(env, contract);
    let token_client = TokenClient::new(env, token);
    let stream = client.get_stream(&stream_id);
    let total_debt = client.total_debt_of(&stream_id);
    let covered = client.covered_debt_of(&stream_id);
    let uncovered = client.uncovered_debt_of(&stream_id);
    let refundable = client.refundable_amount_of(&stream_id);

    assert!(stream.balance >= 0);
    assert!(stream.snapshot_debt_scaled >= 0);
    assert!(total_debt >= 0);
    assert_eq!(total_debt, covered + uncovered);
    assert_eq!(stream.balance, covered + refundable);
    assert_eq!(client.withdrawable_amount_of(&stream_id), covered);
    assert_eq!(token_client.balance(contract), stream.balance);
    assert_eq!(
        token_client.balance(sender)
            + token_client.balance(recipient)
            + token_client.balance(outsider)
            + token_client.balance(contract),
        INITIAL_SUPPLY
    );
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let (env, contract, sender, recipient, outsider, token) = setup();
    let client = FlowContractClient::new(&env, &contract);

    let initial_amount = input.positive_amount();
    let rate = i128::from(input.u64()) % MAX_RATE + 1;
    let decimals = u32::from(input.byte() % 19);
    let start_time = match input.byte() % 3 {
        0 => 0,
        1 => BASE_TIME,
        _ => BASE_TIME + input.u64() % 10_001,
    };
    let stream_id = client.create_and_deposit(
        &sender,
        &recipient,
        &token,
        &rate,
        &decimals,
        &start_time,
        &initial_amount,
    );
    assert_invariants(
        &env, &contract, &token, &sender, &recipient, &outsider, stream_id,
    );

    let operation_count = usize::from(input.byte() % 32) + 1;
    for _ in 0..operation_count {
        let elapsed = input.u64() % 100_001;
        env.ledger()
            .set_timestamp(env.ledger().timestamp().saturating_add(elapsed));
        let amount = input.adversarial_amount();
        let new_rate = match input.byte() % 4 {
            0 => 0,
            1 => -i128::from(input.u64() % MAX_RATE as u64),
            _ => i128::from(input.u64()) % MAX_RATE + 1,
        };
        let to = match input.byte() % 3 {
            0 => sender.clone(),
            1 => recipient.clone(),
            _ => outsider.clone(),
        };

        match input.byte() % 11 {
            0 => {
                let _ = client.try_deposit(&stream_id, &sender, &amount);
            }
            1 => {
                let _ = client.try_withdraw(&stream_id, &recipient, &to, &amount);
            }
            2 => {
                let _ = client.try_withdraw_max(&stream_id, &recipient, &to);
            }
            3 => {
                let _ = client.try_refund(&stream_id, &sender, &amount);
            }
            4 => {
                let _ = client.try_refund_max(&stream_id, &sender);
            }
            5 => {
                let _ = client.try_pause(&stream_id, &sender);
            }
            6 => {
                let _ = client.try_restart(&stream_id, &sender, &new_rate);
            }
            7 => {
                let _ = client.try_adjust_rate(&stream_id, &sender, &new_rate);
            }
            8 => {
                let caller = if input.byte() % 2 == 0 {
                    sender.clone()
                } else {
                    recipient.clone()
                };
                let _ = client.try_void_stream(&stream_id, &caller);
            }
            9 => {
                let _ = client.try_withdraw(&stream_id, &outsider, &to, &amount);
            }
            _ => {
                let _ = client.try_pause(&stream_id, &outsider);
            }
        }

        assert_invariants(
            &env, &contract, &token, &sender, &recipient, &outsider, stream_id,
        );
    }
});
