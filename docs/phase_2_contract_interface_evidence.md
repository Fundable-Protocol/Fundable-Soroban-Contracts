# Phase 2 Contract Interface Evidence

Status: In progress  
Last updated: 2026-08-23

## Scope of This Evidence

This record links the completed phase 2 contract-interface requirements to
their implementation and regression evidence. It does not close the phase 2
exit gate: governance deployment, deeper verification, resource profiling,
and reproducible release artifacts remain outstanding.

## Router and Stream NFT

| Requirement | Implementation | Evidence |
| --- | --- | --- |
| Router is the core recipient | Router supplies its own address as the Flow/Lockup recipient. | `test_end_to_end_flow_stream`, `test_end_to_end_lockup_stream` |
| Current owner authorizes withdrawals | Router resolves `owner_of(token_id)` before routing withdrawal. | `test_withdraw_fails_if_not_nft_owner`, `test_withdraw_after_nft_transfer` |
| Per-stream transferability is immutable and enforced on-chain | Router creation supplies the policy to Stream NFT mint; Stream NFT persists it and rejects transfer when false; Router metadata queries the canonical value. | `test_non_transferable_stream_rejects_transfer`, `test_non_transferable_stream_metadata_and_transfer_enforcement`, `test_transfer` |
| Stable identity/lifecycle queries | Router exposes `owner_of`, `stream_type`, `core_stream_id`, `status_of`, and `get_stream`. | `test_end_to_end_flow_stream`, `test_nft_owner_voids_flow_and_terminal_receipt_persists` |
| NFT-owner Flow voiding | `void_flow(token_id, caller)` verifies current ownership and invokes Flow as the core recipient. | `test_nft_owner_voids_flow_and_terminal_receipt_persists`, `test_non_owner_cannot_void_flow` |
| Public/internal ID events | Router creation, withdrawal, and owner-void events contain both IDs; NFT transfer data contains both IDs. | Explicit event assertions in Router and Stream NFT tests |
| Permanent receipts | The normal `burn` entrypoint is removed; completed receipts retain ownership, mapping, and their immutable transferability policy. | `test_nft_owner_voids_flow_and_terminal_receipt_persists` |
| Constructor initialization | Flow, Lockup, Router, Stream NFT, and transitional Paymaster use `__constructor`. | Constructor tests in every contract package |
| Upgrade governance | Five-signer Governance enforces 3-of-5 plus 48 hours for ordinary actions and 4-of-5 for immediate emergency actions; Router supports governed admin rotation and remains NFT admin. | Governance contract tests, `test_set_admin_rotates_router_authority`, `upgrade_governance.md` |

## Lockup Lifecycle

The Lockup test suite and Router integration tests evidence:

- atomic full funding at creation;
- partial and maximum vested withdrawals;
- cancellation freezing vesting at the cancellation timestamp;
- unvested refund to the original sender;
- continued NFT-owner withdrawal of vested funds after cancellation;
- permanent cancellation renunciation;
- NFT transfer before/after partial withdrawal and before/after cancellation.

The cross-layer regression is
`test_lockup_transfer_partial_withdraw_cancel_and_terminal_withdraw`.

## Flow Interface

Router Flow creation now requires `initial_amount` and atomically calls
`create_and_deposit`. Existing Flow entrypoints cover deposit, pause, restart,
rate adjustment, partial/maximum refund, sender void, balance, covered debt,
uncovered debt, refundable amount, and depletion time. Router adds current
NFT-owner voiding.

Flow remains outside the initial mainnet launch scope and its feature flag must
remain disabled until phase 12 passes.

## Authorization Evidence

Tests assert exact Soroban authorization trees for representative high-risk
cross-contract paths:

- Router Flow creation -> Flow `create_and_deposit` -> token `transfer`;
- Router NFT-owner `void_flow`;
- Router NFT-owner `withdraw_max`;
- original-sender Lockup `cancel`.

Broader exact-tree coverage is still required before `VERIFY-03` can close.

## Current Verification

The workspace baseline and the phase 2 implementation pass all unit and
cross-contract tests. Release-mode WASMs for Flow, Lockup, Stream NFT, and
Router also build successfully. These are development artifacts, not yet the
reproducible release artifacts required by `VERIFY-08` and `VERIFY-09`.

## Remaining Contract Gate Work

- Complete exact authorization-tree coverage for every sensitive mutation and
  upgrade path.
- Add accounting invariant/property tests and fuzz campaigns.
- Test failed cross-contract calls and malicious/non-standard tokens.
- Profile worst-case Soroban resources and fees.
- Produce and independently reproduce final release WASMs with a complete
  toolchain and hash manifest.
