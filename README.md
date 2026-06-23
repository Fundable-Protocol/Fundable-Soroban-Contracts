# Fundable Soroban Contracts

A monorepo workspace containing independent Soroban smart contracts for the Fundable platform on Stellar.

## Project Structure

```
fundable-soroban-contracts/
├── Cargo.toml              # Workspace root — shared deps & profiles
├── Makefile                # Workspace-level build/test/deploy commands
├── contracts/
│   ├── hello-world/        # Reference contract (starter template)
│   │   ├── Cargo.toml
│   │   ├── Makefile
│   │   └── src/
│   │       ├── lib.rs
│   │       └── test.rs
│   └── <your-contract>/    # Add new contracts here
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           └── test.rs
└── README.md
```

Each contract lives in its own directory under `contracts/` with its own `Cargo.toml` that references workspace-level dependencies.

## Prerequisites

- [Rust](https://rustup.rs/) with the `wasm32v1-none` target
- [Stellar CLI](https://developers.stellar.org/docs/tools/stellar-cli)

```bash
# Install WASM target
rustup target add wasm32v1-none

# Install Stellar CLI
cargo install stellar-cli --locked
```

## Quick Start

```bash
# Build all contracts
make build

# Run all tests
make test

# Format code
make fmt

# Run clippy lints
make clippy
```

## Adding a New Contract

1. Create a new directory under `contracts/`:

```bash
mkdir -p contracts/my-contract/src
```

2. Add a `Cargo.toml` for the contract:

```toml
[package]
name = "my-contract"
version.workspace = true
edition.workspace = true
publish = false

[lib]
crate-type = ["lib", "cdylib"]
doctest = false

[dependencies]
soroban-sdk = { workspace = true }

[dev-dependencies]
soroban-sdk = { workspace = true, features = ["testutils"] }
```

3. Create `contracts/my-contract/src/lib.rs` with your contract logic.

4. The workspace auto-discovers contracts via the `contracts/*` glob — no need to manually register them.

## Deployment

```bash
# Generate and fund a testnet identity
stellar keys generate --global alice --network testnet --fund

# Deploy a specific contract
make deploy CONTRACT=my-contract SOURCE=alice

# Or invoke directly
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- \
  <function_name> \
  --arg1 value1
```

## Contracts

| Contract | Description | Status |
|----------|-------------|--------|
| `hello-world` | Starter reference contract | ✅ Template |

> New contracts will be added to this table as they are developed.

## License

MIT