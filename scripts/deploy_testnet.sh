#!/bin/bash
set -e

# Configuration
NETWORK="testnet"
SOURCE=${1:-"deployer"}

# Extract the admin address from the configured identity
ADMIN=$(stellar keys address $SOURCE)

echo "Admin address: $ADMIN"

# 1. Deploy all contracts
echo "Deploying flow contract..."
FLOW_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/flow.wasm --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN")
echo "Flow deployed at: $FLOW_ID"

echo "Deploying lockup contract..."
LOCKUP_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/lockup.wasm --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN")
echo "Lockup deployed at: $LOCKUP_ID"

echo "Deploying router contract..."
ROUTER_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/router.wasm --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN")
echo "Router deployed at: $ROUTER_ID"

echo "Deploying stream_nft contract..."
NFT_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/stream_nft.wasm --source "$SOURCE" --network "$NETWORK" -- \
    --admin "$ROUTER_ID" \
    --name "Fundable Stream NFT" \
    --symbol "FSNFT")
echo "NFT deployed at: $NFT_ID"

# 2. Save output key details to a file
cat << EOF > deployed_contracts.json
{
  "network": "$NETWORK",
  "admin": "$ADMIN",
  "flow": "$FLOW_ID",
  "lockup": "$LOCKUP_ID",
  "stream_nft": "$NFT_ID",
  "router": "$ROUTER_ID"
}
EOF

echo "Saved contract IDs to deployed_contracts.json"

# 3. Configure the Router after the NFT has been constructed with Router admin.
echo "Configuring router contract..."
stellar contract invoke --id "$ROUTER_ID" --source "$SOURCE" --network "$NETWORK" -- configure \
    --flow_contract "$FLOW_ID" \
    --lockup_contract "$LOCKUP_ID" \
    --nft_contract "$NFT_ID"

echo "All contracts deployed and configured successfully!"
