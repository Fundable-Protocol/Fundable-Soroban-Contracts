#!/bin/bash
set -e

# Configuration
NETWORK="testnet"
SOURCE="deployer"

# Extract the sender address from the configured identity
SENDER=$(stellar keys address $SOURCE)

# The recipient requested
RECIPIENT="GCRK2BBUCYTZHMNXQJ66ZDRHKREIR3TJ6FVSVMJV2CDI56NXIJUULMPG"

# Testnet Native XLM Token Contract
TOKEN="CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"

# Lockup Details
# XLM has 7 decimals. 1 XLM = 10,000,000 stroops
# 1000 XLM = 1000 * 10,000,000 = 10,000,000,000
TOTAL_AMOUNT=10000000000

# Time calculations
START_TIME=$(date +%s)
# 5 days in seconds = 5 * 24 * 60 * 60 = 432000
END_TIME=$((START_TIME + 432000))

echo "Creating Lockup Stream via Router"
echo "From: $SENDER"
echo "To: $RECIPIENT"
echo "Amount: 1000 XLM"
echo "Start: $START_TIME"
echo "End: $END_TIME (5 days)"

# Read Router ID from deployed_contracts.json (using a simple grep/awk trick to avoid requiring jq)
ROUTER_ID=$(grep -o '"router": *"[^"]*"' deployed_contracts.json | grep -o '"[^"]*"$' | tr -d '"')

if [ -z "$ROUTER_ID" ]; then
    echo "Error: Could not find router ID in deployed_contracts.json"
    exit 1
fi

echo "Router Contract: $ROUTER_ID"

# Construct the JSON parameter for the CreateLockupParams struct
PARAMS_JSON=$(cat <<EOF
{
  "sender": "$SENDER",
  "recipient": "$RECIPIENT",
  "token": "$TOKEN",
  "total_amount": "$TOTAL_AMOUNT",
  "start_time": $START_TIME,
  "end_time": $END_TIME,
  "cliff_time": 0,
  "start_unlock_amount": "0",
  "cliff_unlock_amount": "0",
  "granularity": 1,
  "cancelable": true
}
EOF
)

# Invoke the router to create the lockup stream
stellar contract invoke \
  --id "$ROUTER_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- \
  create_lockup_stream \
  --params "$PARAMS_JSON"

echo "Stream created successfully!"
