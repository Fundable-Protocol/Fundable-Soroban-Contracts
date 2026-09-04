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
