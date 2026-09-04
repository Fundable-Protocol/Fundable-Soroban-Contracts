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
  config

docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
  pull relayer

docker compose \
  -f /path/to/openzeppelin-relayer/docker-compose.yaml \
  -f /path/to/fundable-soroban-contracts/ops/openzeppelin-relayer/docker-compose.production.yaml \
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
  config --images
```

The output must exactly match the pinned image reference above.

## Testnet fee strategy and token allowlist

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
