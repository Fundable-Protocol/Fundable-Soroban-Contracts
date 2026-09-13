# Fundable OpenZeppelin Relayer Production Pin

Fundable production uses OpenZeppelin Relayer `v1.6.0` from source commit
`554f15adb4a20b180a367154d7d383351bb75b5a`. The official multi-architecture
container is pinned by OCI index digest:

```text
openzeppelin/openzeppelin-relayer:1.6.0@sha256:89d7b3df96949322dacb6df25732d4d747b9612b562ba87324997c2ae2c937ae
```

Use this file as the production override for the upstream Compose definition:

```bash
docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.testnet.yaml \
  config

docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.testnet.yaml \
  pull relayer

docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.testnet.yaml \
  up -d --no-build relayer redis
```

The override removes the upstream development `build` section. Production
must use `--no-build`; building from a mutable checkout bypasses the verified
release artifact. Keep configuration and secrets outside this repository.

To verify the resolved image without printing secrets:

```bash
docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.testnet.yaml \
  config --images
```

The output must exactly match the pinned image reference above.

## FeeForwarder identity

OZ Relayer does not provide a usable implicit FeeForwarder address for every
Stellar network. Always include exactly one network overlay. The testnet
overlay pins the verified deployment directly; the mainnet overlay refuses to
start Compose interpolation unless `STELLAR_MAINNET_FEE_FORWARDER_ADDRESS` is
set. DEPLOY-05 remains responsible for supplying the verified mainnet address.

The testnet FeeForwarder is the permissionless example from OpenZeppelin
Stellar Contracts `v0.7.1`, commit
`3f81125bed3114cc93f5fca6d13240082050269a`. It is deployed at
`CDJM3SROZG3TY3URXSFH7J5GEIVFHZZKWX5DVJISED6YBIONA76WBU7D`, from contract
creation transaction
`baf15fc4fe87ae5dc12e1389baf404505d8428ccf67d3496c6bcb471b9bc50d2`.
The locally built and RPC-fetched WASM are byte-for-byte identical with SHA-256
`c0292b4a994c0c94280a5a1783d907ae54b52f5e34e74bb7d5d65645ca7508fa`.
The complete source, ABI, build, transaction, and verification record is in
`fee-forwarder.testnet.json`.

Reproduce the build with Stellar CLI `27.0.0` and Rust `1.92.0`, then compare
it with the deployed code:

```bash
git clone https://github.com/OpenZeppelin/stellar-contracts.git
cd stellar-contracts
git checkout 3f81125bed3114cc93f5fca6d13240082050269a
stellar contract build \
  --package fee-forwarder-permissionless-example \
  --locked \
  --out-dir ./fee-forwarder-build
stellar contract fetch \
  --id CDJM3SROZG3TY3URXSFH7J5GEIVFHZZKWX5DVJISED6YBIONA76WBU7D \
  --network testnet \
  --out-file ./deployed-fee-forwarder.wasm
shasum -a 256 \
  ./fee-forwarder-build/fee_forwarder_permissionless_example.wasm \
  ./deployed-fee-forwarder.wasm
cmp ./fee-forwarder-build/fee_forwarder_permissionless_example.wasm \
  ./deployed-fee-forwarder.wasm
```

Inspect the fetched artifact and confirm that `forward` takes, in order:
`fee_token`, `fee_amount`, `max_fee_amount`, `expiration_ledger`,
`target_contract`, `target_fn`, `target_args`, `user`, and `relayer`. This is the
ABI hard-coded by OZ Relayer `v1.6.0`. The older Fundable Paymaster is not a
compatible substitute because its argument order differs.

For mainnet, use `docker-compose.mainnet.yaml` in place of the testnet overlay.
Never copy a testnet address into the mainnet variable.

## Backend-only credential boundary

The production override binds the relayer API to `127.0.0.1` by default and
disables Swagger and metrics. If the backend runs on another host, set
`RELAYER_BIND_ADDRESS` only to a private service address and enforce a network
policy or firewall that allows the backend service identity alone. Do not bind
the relayer to `0.0.0.0` or expose it through the public frontend ingress.

Store `API_KEY`, `KEYSTORE_PASSPHRASE`, and `WEBHOOK_SIGNING_KEY` in the
deployment secret manager. Supply `OZ_RELAYER_API_KEY` only to the backend
runtime. Never place these values in tracked configuration, browser storage,
frontend environment variables, `NEXT_PUBLIC_*`/`VITE_*` variables, logs, or
client responses. The public relayer signing address is not a credential and
may be exposed for transaction validation.

Before a release, verify the rendered Compose configuration in a controlled
shell, check that its published address is loopback/private, and scan tracked
backend and frontend files for credential variable misuse. Do not paste the
rendered environment or secret values into CI output.

## Testnet fee strategy and token allowlist

Soroban fee quotes require `swap_config.strategies: ["soroswap"]` and the
Router, Factory, and native XLM wrapper addresses in the testnet Compose
overlay. Addresses come from the
[Soroswap deployment record](https://github.com/soroswap/core/blob/main/public/testnet.contracts.json).
The six-field schedule `0 0 */6 * * *` runs conversion every six hours;
the five-field expression in some upstream examples fails startup validation.
Reverify the deployed contracts and USDC/XLM liquidity after a testnet reset.

The tracked testnet configuration template is
`config.testnet.example.json`. Its Stellar relayer policy sets
`fee_payment_strategy` to `user`, which is required by the native OpenZeppelin
gas-abstraction flow. Deploy the template as `config/config.json` and keep the
referenced signer keystore and passphrase outside this repository.

The allowlist contains only Circle's Stellar testnet USDC Soroban token
contract, `CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA`. The
deployed Fundable Paymaster must independently allow the same contract. Fee
authorization is capped at `10,000,000` of USDC's seven-decimal base units,
exactly `1.0000000 USDC`. This is a maximum the user may authorize, not a fixed
charge. OpenZeppelin Relayer rejects a request whose signed `max_fee_amount`
exceeds it. Do not add another token to one allowlist without adding and
verifying it in the other and assigning an explicit cap.

The `1 USDC` testnet ceiling is an operational guardrail with approximately
3x price-adjusted headroom over the repository's conservative `1.790825 XLM`
worst-case routed-creation estimate using the 2026-09-04 reference price of
`$0.1798/XLM`. Review it against live fee and XLM/USDC telemetry before mainnet
rollout and whenever either nears the ceiling.

The platform XLM policy sets `max_fee` to `30,000,000` stroops (`3 XLM`) and
`fee_margin_percentage` to `10.0`. OpenZeppelin Relayer applies the margin to
the simulated XLM fee before enforcing `max_fee` and converting the charge to
USDC. The conservative `17,908,250`-stroop routed-creation profile becomes
`19,699,075` stroops after the margin, leaving approximately 52% ceiling
headroom for FeeForwarder overhead and network variance. A request is rejected
if its margin-adjusted fee exceeds `3 XLM`; live RPC simulation remains
authoritative.

Validate the non-secret policy fields before starting the service:

```bash
node -e '
  const config = require("./ops/openzeppelin-relayer/config.testnet.example.json");
  const relayer = config.relayers.find((entry) => entry.id === "fundable-stellar-relayer");
  if (relayer?.network !== "testnet") throw new Error("expected testnet");
  if (relayer?.network_type !== "stellar") throw new Error("expected Stellar");
  if (relayer?.policies?.fee_payment_strategy !== "user") {
    throw new Error("expected user fee strategy");
  }
  const xlmCap = relayer.policies.max_fee;
  const feeMargin = relayer.policies.fee_margin_percentage;
  if (xlmCap !== 30_000_000) throw new Error("unexpected XLM fee cap");
  if (feeMargin !== 10.0) throw new Error("unexpected fee margin");
  const marginAdjusted = (fee) => Math.trunc(fee * (1 + feeMargin / 100));
  if (marginAdjusted(17_908_250) !== 19_699_075) {
    throw new Error("unexpected profiled fee after margin");
  }
  if (marginAdjusted(17_908_250) > xlmCap) {
    throw new Error("profiled fee exceeds XLM cap");
  }
  const acceptsXlmFee = (fee) => fee > 0 && fee <= xlmCap;
  if (!acceptsXlmFee(xlmCap) || acceptsXlmFee(xlmCap + 1)) {
    throw new Error("XLM fee cap boundary check failed");
  }
  const assets = relayer?.policies?.allowed_tokens?.map(({ asset }) => asset);
  const expected = ["CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA"];
  if (JSON.stringify(assets) !== JSON.stringify(expected)) {
    throw new Error("unexpected testnet token allowlist");
  }
  const cap = relayer.policies.allowed_tokens[0].max_allowed_fee;
  if (cap !== 10_000_000) throw new Error("unexpected USDC fee cap");
  const accepts = (fee) => Number.isSafeInteger(fee) && fee > 0 && fee <= cap;
  if (!accepts(cap) || accepts(cap + 1)) {
    throw new Error("USDC fee cap boundary check failed");
  }
'
```
