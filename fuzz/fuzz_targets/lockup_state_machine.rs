#![no_main]

use libfuzzer_sys::fuzz_target;
use lockup::{LockupContract, LockupContractArgs, LockupContractClient};
use shared::types::CreateLockupParams;
use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig, Ledger, LedgerInfo},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

const BASE_TIME: u64 = 1_000;
const INITIAL_SUPPLY: i128 = 100_000_000;
const MAX_AMOUNT: i128 = 1_000_000;

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
    let contract = env.register(LockupContract, LockupContractArgs::__constructor(&admin));

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
    let client = LockupContractClient::new(env, contract);
    let token_client = TokenClient::new(env, token);
    let stream = client.get_stream(&stream_id);
    let streamed = client.streamed_amount_of(&stream_id);
    let withdrawable = client.withdrawable_amount_of(&stream_id);
    let refundable = client.refundable_amount_of(&stream_id);
    let contract_balance = token_client.balance(contract);

    assert!(stream.total_amount > 0);
    assert!(stream.withdrawn_amount >= 0);
    assert!(stream.refunded_amount >= 0);
    assert!(stream.granularity > 0);
    assert!(streamed >= stream.withdrawn_amount);
    assert!(streamed <= stream.total_amount - stream.refunded_amount);
    assert_eq!(withdrawable, streamed - stream.withdrawn_amount);
    assert_eq!(
        stream.total_amount,
        stream.withdrawn_amount + stream.refunded_amount + contract_balance
    );
    if stream.cancelable && !stream.was_canceled && !stream.is_depleted {
        assert_eq!(refundable, stream.total_amount - streamed);
    } else {
        assert_eq!(refundable, 0);
    }
    if stream.is_depleted {
        assert_eq!(contract_balance, 0);
    }
    assert_eq!(
        token_client.balance(sender)
            + token_client.balance(recipient)
            + token_client.balance(outsider)
            + contract_balance,
        INITIAL_SUPPLY
    );
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let (env, contract, sender, recipient, outsider, token) = setup();
    let client = LockupContractClient::new(&env, &contract);

    let total_amount = input.positive_amount();
    let start_time = BASE_TIME + input.u64() % 1_001;
    let duration = input.u64() % 10_000 + 2;
    let end_time = start_time + duration;
    let cliff_time = match input.byte() % 3 {
        0 => 0,
        _ => start_time + 1 + input.u64() % (duration - 1),
    };
    let start_unlock_amount = total_amount * i128::from(input.byte() % 101) / 100;
    let remaining = total_amount - start_unlock_amount;
    let cliff_unlock_amount = remaining * i128::from(input.byte() % 101) / 100;
    let granularity = input.u64() % (duration.saturating_mul(2) + 1);
    let params = CreateLockupParams {
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount,
        start_time,
        end_time,
        cliff_time,
        start_unlock_amount,
        cliff_unlock_amount,
        granularity,
        cancelable: input.byte() % 2 == 0,
    };
    let stream_id = client.create(&params);
    assert_invariants(
        &env, &contract, &token, &sender, &recipient, &outsider, stream_id,
    );

    let operation_count = usize::from(input.byte() % 32) + 1;
    for _ in 0..operation_count {
        let elapsed = input.u64() % 100_001;
        env.ledger()
            .set_timestamp(env.ledger().timestamp().saturating_add(elapsed));
        let amount = input.adversarial_amount();
        let to = match input.byte() % 3 {
            0 => sender.clone(),
            1 => recipient.clone(),
            _ => outsider.clone(),
        };

        match input.byte() % 8 {
            0 => {
                let _ = client.try_withdraw(&stream_id, &recipient, &to, &amount);
            }
            1 => {
                let _ = client.try_withdraw_max(&stream_id, &recipient, &to);
            }
            2 => {
                let _ = client.try_cancel(&stream_id, &sender);
            }
            3 => {
                let _ = client.try_renounce(&stream_id, &sender);
            }
            4 => {
                let _ = client.try_withdraw(&stream_id, &outsider, &to, &amount);
            }
            5 => {
                let _ = client.try_cancel(&stream_id, &outsider);
            }
            6 => {
                let _ = client.try_renounce(&stream_id, &outsider);
            }
            _ => {
                let _ = client.status_of(&stream_id);
            }
        }

        assert_invariants(
            &env, &contract, &token, &sender, &recipient, &outsider, stream_id,
        );
    }
});
