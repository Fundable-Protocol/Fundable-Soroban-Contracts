#!/bin/bash
set -e

# Configuration
NETWORK="testnet"
SOURCE="deployer"

# Fee token contract ID - pass as first argument or use a default testnet token (USDC)
FEE_TOKEN=${1:-"CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA"}

# Extract the admin address from the configured identity
ADMIN=$(stellar keys address $SOURCE)

echo "Admin address: $ADMIN"
echo "Using Fee Token Address: $FEE_TOKEN"

# 1. Deploy the contract
echo "Deploying paymaster contract..."
PAYMASTER_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/paymaster.wasm --source $SOURCE --network $NETWORK)
echo "Paymaster deployed at: $PAYMASTER_ID"

# 2. Initialize the contract
echo "Initializing paymaster contract..."
stellar contract invoke \
  --id $PAYMASTER_ID \
  --source $SOURCE \
  --network $NETWORK \
  -- \
  initialize \
  --admin $ADMIN \
  --allowed_fee_tokens "[\"$FEE_TOKEN\"]"

echo "Paymaster contract deployed and initialized successfully!"

# 3. Update deployed_contracts.json if it exists
if [ -f "deployed_contracts.json" ]; then
  # Simple append (Note: this makes it invalid JSON if not careful, better to use node/jq)
  echo "Remember to add the following to deployed_contracts.json:"
  echo "\"paymaster\": \"$PAYMASTER_ID\""
fi
