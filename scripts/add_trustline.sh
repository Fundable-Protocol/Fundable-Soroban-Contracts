#!/bin/bash
set -e

# Configuration (defaults can be overridden via arguments)
ACCOUNT=${1:-"lawal"}
ASSET_CODE=${2:-"USDC"}
ISSUER=${3:-"GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"}
NETWORK="testnet"

echo "Adding trustline for $ASSET_CODE:$ISSUER to account '$ACCOUNT' on $NETWORK..."

# Build, sign, and send the transaction
stellar tx new change-trust \
  --source-account "$ACCOUNT" \
  --line "$ASSET_CODE:$ISSUER" \
  --network "$NETWORK" \
  --build-only \
  | stellar tx sign \
  --sign-with-key "$ACCOUNT" \
  --network "$NETWORK" \
  | stellar tx send \
  --network "$NETWORK"

echo "Trustline added successfully!"
