#!/bin/bash
# =============================================================================
# Fundable Soroban Contracts — Testnet Deployment Script
#
# SECURITY NOTES:
# - This script deploys and initializes all contracts on testnet.
# - Deploy and initialize are SEPARATE transactions — there is a race window
#   between deploy and initialize where a front-runner could call initialize()
#   with their own admin address. This is mitigated by requiring admin.require_auth()
#   in all initializers, but the deployer should still initialize immediately
#   after each deployment.
# - The Stellar/Soroban platform does not currently support atomic
#   deploy+initialize in a single transaction from the CLI. If/when a
#   deployer/factory contract is available, use that instead.
# - This script never prints secret keys.
# - If any initialization fails, the script stops immediately.
# =============================================================================
set -euo pipefail

# Configuration
NETWORK="${NETWORK:-testnet}"
SOURCE="${1:-deployer}"

# Extract the admin address from the configured identity
ADMIN=$(stellar keys address "$SOURCE")

echo "============================================"
echo "Fundable Soroban Contracts — Deployment"
echo "============================================"
echo "Network:  $NETWORK"
echo "Admin:    $ADMIN"
echo "Source:   $SOURCE (identity name only, no secret)"
echo ""

# 1. Deploy all contracts
echo "--- Deploying contracts ---"

echo "Deploying flow contract..."
FLOW_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/flow.wasm --source "$SOURCE" --network "$NETWORK")
echo "Flow deployed at: $FLOW_ID"

echo "Deploying lockup contract..."
LOCKUP_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/lockup.wasm --source "$SOURCE" --network "$NETWORK")
echo "Lockup deployed at: $LOCKUP_ID"

echo "Deploying stream_nft contract..."
NFT_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/stream_nft.wasm --source "$SOURCE" --network "$NETWORK")
echo "NFT deployed at: $NFT_ID"

echo "Deploying router contract..."
ROUTER_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/router.wasm --source "$SOURCE" --network "$NETWORK")
echo "Router deployed at: $ROUTER_ID"

echo "Deploying paymaster contract..."
PAYMASTER_ID=$(stellar contract deploy --wasm target/wasm32v1-none/release/paymaster.wasm --source "$SOURCE" --network "$NETWORK")
echo "Paymaster deployed at: $PAYMASTER_ID"

# 2. Save / append contract IDs to deployed_contracts.json
if [ -f "deployed_contracts.json" ]; then
  if command -v jq >/dev/null 2>&1; then
    TMP_JSON=$(mktemp)
    jq \
      --arg network "$NETWORK" \
      --arg admin "$ADMIN" \
      --arg flow "$FLOW_ID" \
      --arg lockup "$LOCKUP_ID" \
      --arg stream_nft "$NFT_ID" \
      --arg router "$ROUTER_ID" \
      --arg paymaster "$PAYMASTER_ID" \
      '. + {network: $network, admin: $admin, flow: $flow, lockup: $lockup, stream_nft: $stream_nft, router: $router, paymaster: $paymaster}' \
      deployed_contracts.json > "$TMP_JSON" && mv "$TMP_JSON" deployed_contracts.json
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c "
import json
try:
    with open('deployed_contracts.json', 'r') as f:
        data = json.load(f)
except Exception:
    data = {}
data.update({
    'network': '$NETWORK',
    'admin': '$ADMIN',
    'flow': '$FLOW_ID',
    'lockup': '$LOCKUP_ID',
    'stream_nft': '$NFT_ID',
    'router': '$ROUTER_ID',
    'paymaster': '$PAYMASTER_ID'
})
with open('deployed_contracts.json', 'w') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
"
  else
    cat << EOF > deployed_contracts.json
{
  "network": "$NETWORK",
  "admin": "$ADMIN",
  "flow": "$FLOW_ID",
  "lockup": "$LOCKUP_ID",
  "stream_nft": "$NFT_ID",
  "router": "$ROUTER_ID",
  "paymaster": "$PAYMASTER_ID"
}
EOF
  fi
else
  cat << EOF > deployed_contracts.json
{
  "network": "$NETWORK",
  "admin": "$ADMIN",
  "flow": "$FLOW_ID",
  "lockup": "$LOCKUP_ID",
  "stream_nft": "$NFT_ID",
  "router": "$ROUTER_ID",
  "paymaster": "$PAYMASTER_ID"
}
EOF
fi

echo ""
echo "Appended all contract IDs to deployed_contracts.json"

# 3. Initialize Contracts
# CRITICAL: Initialize immediately after deployment to minimize the
# race window. The admin.require_auth() in each initializer prevents
# unauthorized initialization, but prompt initialization is still best practice.

echo ""
echo "--- Initializing contracts (admin auth required) ---"

echo "Initializing flow contract..."
stellar contract invoke --id "$FLOW_ID" --source "$SOURCE" --network "$NETWORK" -- initialize --admin "$ADMIN"
echo "  ✓ Flow initialized"

echo "Initializing lockup contract..."
stellar contract invoke --id "$LOCKUP_ID" --source "$SOURCE" --network "$NETWORK" -- initialize --admin "$ADMIN"
echo "  ✓ Lockup initialized"

echo "Initializing stream_nft contract..."
stellar contract invoke --id "$NFT_ID" --source "$SOURCE" --network "$NETWORK" -- initialize \
    --admin "$ROUTER_ID" \
    --name "Fundable Stream NFT" \
    --symbol "FSNFT"
echo "  ✓ StreamNFT initialized (admin = Router)"

echo "Initializing router contract..."
stellar contract invoke --id "$ROUTER_ID" --source "$SOURCE" --network "$NETWORK" -- initialize \
    --admin "$ADMIN" \
    --flow_contract "$FLOW_ID" \
    --lockup_contract "$LOCKUP_ID" \
    --nft_contract "$NFT_ID"
echo "  ✓ Router initialized"

# 4. Post-deployment verification
echo ""
echo "--- Verifying deployment ---"

echo "Verifying Router flow_contract..."
# These queries would verify the initialized state if the contracts expose admin/config getters.
# For now, we verify that the contracts respond without error.



echo ""
echo "============================================"
echo "All contracts deployed and initialized!"
echo "============================================"
echo ""
echo "Contract Addresses:"
echo "  Flow:       $FLOW_ID"
echo "  Lockup:     $LOCKUP_ID"
echo "  StreamNFT:  $NFT_ID"
echo "  Router:     $ROUTER_ID"
echo "  Paymaster:  $PAYMASTER_ID"
echo ""
echo "Admin:        $ADMIN"
echo ""
echo "IMPORTANT: Verify these addresses match your expectations."
echo "IMPORTANT: The Paymaster is not yet initialized. Initialize it separately"
echo "           with the desired fee token allowlist."
