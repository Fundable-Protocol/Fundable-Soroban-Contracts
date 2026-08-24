# Phase 2 Contract Interface Evidence

Status: In progress  
Last updated: 2026-08-24

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

Exact authorization-tree assertions now cover every production-sensitive
entrypoint:

| Surface | Exact-tree coverage |
| --- | --- |
| Flow | `upgrade`, `set_admin`, `create`, `create_and_deposit`, `deposit`, `withdraw`, `withdraw_max`, `pause`, `restart`, `adjust_rate`, `refund`, `refund_max`, and `void_stream` |
| Lockup | `upgrade`, `set_admin`, `create`, `withdraw`, `withdraw_max`, `cancel`, and `renounce` |
| Stream NFT | `upgrade`, `mint`, and `transfer` |
| Router | `configure`, `upgrade`, `set_admin`, `upgrade_nft`, Flow/Lockup creation, `withdraw`, `withdraw_max`, and `void_flow` |
| Governance | `propose`, `approve`, and `approve_cancellation`; execution is asserted permissionless while the Governance contract authenticates as the immediate invoker |

Creation and deposit assertions include the nested token `transfer`
sub-invocations and exact arguments. Router creation assertions include the
Router-to-core call and nested asset transfer. Governance integration tests
exercise real Router admin rotation after the approved threshold and delay.

The Paymaster is intentionally excluded because ARCH-01 designates it as
transitional testnet code and prohibits it from the production transaction
path. This closes `VERIFY-03` for the production contract boundary.

## Accounting Invariant Evidence

Property tests run 32 generated cases per property and disable per-case Soroban
snapshot output so CI retains only actionable failure regressions.

Flow properties vary deposits, rates, elapsed time, withdrawal/refund shares,
pause duration, and restart rates. Every generated transition must preserve:

- `total debt = covered debt + uncovered debt`;
- `stream balance = covered debt + refundable balance`;
- nonnegative balances and debt partitions;
- contract token balance equals the recorded stream balance;
- sender, recipient, and contract token balances conserve the minted supply;
- pausing freezes debt and restarting accrues only from the new snapshot.

Lockup properties vary total amounts, duration, elapsed time, granularity,
initial unlock share, and withdrawal share. Every generated transition must
preserve:

- `deposited = withdrawn + refunded + remaining contract balance`;
- `withdrawable = streamed - withdrawn`;
- streamed, withdrawn, refunded, and remaining amounts stay within bounds;
- cancellation freezes `streamed = deposited - refunded`;
- terminal withdrawal leaves no contract balance and conserves token supply.

The evidence is provided by
`prop_flow_conserves_assets_and_partitions_debt`,
`prop_flow_pause_preserves_debt_and_restart_accrues_from_snapshot`, and
`prop_lockup_conserves_assets_through_withdrawal_and_cancellation`. This closes
`VERIFY-04`.

## Current Verification

The Phase 2 interface, governance, and transferability changes are isolated in
commit `a9deef4` (`feat: implement governance contract for multisig upgrades and
add non-transferable stream enforcement logic`). The worktree was clean when
that commit was verified, satisfying `VERIFY-01`.

The workspace baseline and the phase 2 implementation pass all 113 unit,
property, and
cross-contract tests. Release-mode WASMs for Flow, Lockup, Stream NFT, and
Router also build successfully. These are development artifacts, not yet the
reproducible release artifacts required by `VERIFY-08` and `VERIFY-09`.

## Remaining Contract Gate Work

- Add fuzz campaigns for amounts, timestamps, granularity, and state
  transitions.
- Test failed cross-contract calls and malicious/non-standard tokens.
- Profile worst-case Soroban resources and fees.
- Produce and independently reproduce final release WASMs with a complete
  toolchain and hash manifest.
