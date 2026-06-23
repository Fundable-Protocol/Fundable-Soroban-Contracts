default: build

all: test

# Build all contracts in the workspace
build:
	stellar contract build
	@echo ""
	@echo "Built WASM artifacts:"
	@ls -lh target/wasm32v1-none/release/*.wasm 2>/dev/null || echo "  (none found)"

# Run all tests
test: build
	cargo test

# Run tests without building WASM first (faster iteration)
test-quick:
	cargo test

# Format all code
fmt:
	cargo fmt --all

# Check formatting without modifying
fmt-check:
	cargo fmt --all -- --check

# Run clippy lints
clippy:
	cargo clippy --all-targets

# Clean build artifacts
clean:
	cargo clean

# Deploy a specific contract to testnet
# Usage: make deploy CONTRACT=hello-world SOURCE=alice
deploy:
	@if [ -z "$(CONTRACT)" ]; then echo "Usage: make deploy CONTRACT=<name> SOURCE=<identity>"; exit 1; fi
	@if [ -z "$(SOURCE)" ]; then echo "Usage: make deploy CONTRACT=<name> SOURCE=<identity>"; exit 1; fi
	stellar contract deploy \
		--wasm target/wasm32v1-none/release/$(CONTRACT).wasm \
		--source $(SOURCE) \
		--network testnet

.PHONY: default all build test test-quick fmt fmt-check clippy clean deploy
