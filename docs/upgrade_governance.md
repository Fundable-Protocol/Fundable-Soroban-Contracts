# Stellar Streams Upgrade Governance

Status: CTO approved  
Version: 1.0  
Approved: 2026-08-23

## Policy

Production upgrade authority uses an on-chain five-signer governance contract:

- ordinary proposals require three distinct signer approvals and a 48-hour
  execution delay;
- emergency proposals require four distinct signer approvals and may execute
  immediately;
- every proposal includes a nonzero 32-byte reason or document hash;
- proposals expire after seven days and cannot be replayed;
- three distinct signer cancellation votes cancel an active proposal;
- execution is permissionless after the applicable threshold and delay pass.

The emergency route is reserved for active security incidents or an immediate
risk of loss. Operational urgency, feature delivery, or convenience is not an
emergency.

## Authority Topology

The Governance contract is the production admin of Flow, Lockup, and Router.
Router remains the Stream NFT admin because it must mint receipts and is the
only supported route for NFT upgrades. Fundable's transitional Paymaster is
not part of the production governance boundary.

```text
five signers
    -> Governance (3-of-5 + 48h, or emergency 4-of-5)
        -> Flow upgrade/set_admin
        -> Lockup upgrade/set_admin
        -> Router upgrade/set_admin
            -> Stream NFT upgrade
```

The governance signer roster is immutable. Signer replacement or policy
migration requires deploying a replacement Governance contract and executing
timelocked `set_admin` proposals for Flow, Lockup, and Router. Stream NFT
authority follows Router and is not transferred independently.

## Proposal Actions

- `Upgrade(target, wasm_hash)` invokes `upgrade` on Flow, Lockup, or Router.
- `UpgradeNft(router, wasm_hash)` invokes Router's `upgrade_nft` route.
- `SetAdmin(target, new_admin)` transfers a governed admin role, including
  migration to a replacement Governance contract.

## Operational Requirements

- Use five separately controlled hardware- or KMS-backed signer keys.
- Record signer ownership and replacement procedures outside the repository.
- Publish the proposal reason document before ordinary execution.
- Independently verify the proposed WASM hash and reproducible-build manifest.
- Exercise normal, cancellation, emergency, and governance-migration runbooks
  on testnet before mainnet deployment.
- Verify Flow, Lockup, Router, Stream NFT, and Governance contract IDs, admin
  addresses, and WASM hashes after deployment or upgrade.

## Evidence

The Governance tests cover normal and emergency thresholds, timelock
enforcement, cancellation, expiration, duplicate approvals, unauthorized
signers, replay prevention, generic admin transfer, and real Router admin
rotation. Router separately tests that its new admin becomes authoritative.
