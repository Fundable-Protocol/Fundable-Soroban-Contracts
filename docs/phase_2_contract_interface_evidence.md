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

## Fuzz Harness Status

Coverage-guided state-machine targets now exist for Flow and Lockup under
`fuzz/fuzz_targets`. They generate bounded amounts, timestamps, Lockup
granularity, authorized and unauthorized callers, and up to 32 lifecycle
transitions while checking the accounting invariants above after every call.
Both targets compile with nightly Rust and `libfuzzer-sys 0.4.10`.

The AddressSanitizer campaign remains deferred because the local
`x86_64-apple-darwin` linker rejects sanitizer-instrumented Soroban `cdylib`
initializers. `VERIFY-05` remains open until the campaigns run in a compatible
Linux CI environment (or successfully run locally with a supported sanitizer
configuration).

## External Token Failure Evidence

Flow and Lockup now verify the token balances of both transfer participants
before and after every inbound and outbound transfer. A successful token call
is accepted only when the sender is debited and the recipient is credited by
exactly the requested amount; otherwise the engine raises
`TokenTransferMismatch` and the complete Soroban invocation rolls back.

The adversarial-token tests exercise three behaviors on creation/deposit,
withdrawal, refund, and cancellation paths:

- an explicit cross-contract rejection;
- a no-op transfer that falsely returns success;
- a fee-on-transfer that credits one unit less than requested.

Every case must preserve the pre-call stream record, ID allocation, contract
token balance, sender balance, and recipient balance. Fully malicious tokens
can still lie consistently from both `transfer` and `balance`; therefore token
identity remains a product-level trust decision, while such a token cannot
counterfeit or drain a different token's isolated balance. This closes
`VERIFY-06`.

## Release-WASM Resource Profile

`profile_worst_case_release_wasm_calls` registers the optimized Flow, Lockup,
Router, and Stream NFT WASMs and profiles the heaviest routed creation and
withdrawal paths. The test checks every measurement against the Protocol 25
mainnet invocation ceilings exposed by `soroban-sdk 25.3.x`.

| Scenario | Instructions | Memory bytes | Footprint entries | Writes | Write bytes | Event bytes | Estimated fee (stroops) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Router Flow creation | 2,560,151 | 3,813,977 | 18 | 11 | 2,412 | 1,104 | 17,225,534 |
| Router Flow withdrawal | 2,601,871 | 4,984,215 | 16 | 5 | 1,180 | 664 | 2,778,154 |
| Router Lockup creation | 2,531,237 | 3,843,476 | 18 | 11 | 2,620 | 996 | 17,908,250 |
| Router Lockup maximum withdrawal | 2,721,397 | 4,993,794 | 15 | 5 | 1,388 | 664 | 2,115,512 |

The maxima consume approximately 0.46% of the 600M instruction limit, 11.9%
of the 40 MiB memory limit, 18% of the 100-entry footprint limit, 22% of the
50-write limit, 2.0% of the 132,096-byte write limit, and 6.7% of the
16,384-byte event limit. The creation fee is dominated by persistent-entry
rent. SDK fee estimates use a conservative bundled fee-rate snapshot and are
not transaction quotes; deployment tooling must still use RPC simulation for
the live fee immediately before submission. This closes `VERIFY-07`.

## Reproducible Release WASMs

`scripts/verify_reproducible_wasms.sh` exports the selected source commit into
two clean directories at different paths, builds each production contract
with an independent target cache and `--locked`, and compares the optimized
WASMs byte-for-byte. Flow, Lockup, and Stream NFT are deliberately built before
Router so its compile-time contract imports always come from the same clean
build. The transitional Paymaster is excluded from the production artifact
set under `ARCH-01`.

Two clean builds of commit `23c1fdd5818d320dc519b4a0f4408a481658aa5c`
produced identical bytes for all five artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `flow.wasm` | `f45227cc479e09a1db5a2e28b9c1291b6854aeb927e680a9d47b0432060ec632` |
| `lockup.wasm` | `b98bafd84e9678ffddc13dca2bf705fbdcdc5f38217ab4bfef9072d7114293f9` |
| `stream_nft.wasm` | `cca5e863d41af5aa51541b8b5f78528f7b8c231f092d4191d21e4dda82379529` |
| `governance.wasm` | `f92a523216cc891742753b1c49f76a3f810b50d393701b43813efd2cd6568ef4` |
| `router.wasm` | `72a96c4a668f6172f65501beb3d9867697da97d146aa05f6b2a532e7c27b1688` |

Verified artifacts and `SHA256SUMS` are emitted under
`target/reproducible-release/`. This closes `VERIFY-08`.

## Release Provenance

The durable, machine-readable release record is
`release/stellar-streams-mainnet/PROVENANCE.tsv`, with artifact digests in the
adjacent `SHA256SUMS`. It binds the five WASMs above to source commit
`23c1fdd5818d320dc519b4a0f4408a481658aa5c` and Git tree
`f86e34ab4dc253dac5f95a717ce99403e9bc066b`; Rust 1.92.0
(`ded5c06cf21d2b93bffd5d884aa6e96934ee4234`, LLVM 21.1.3), Cargo 1.92.0,
the `wasm32v1-none` target, Stellar CLI 27.0.0
(`5a7c5fe76530bf4248477ac812fc757146b98cc4`), and Soroban SDK 25.3.1.

The record also pins the complete `Cargo.lock` by SHA-256
`024fccc21d560bf4e8eb11f62e592ad387b5f102c10e457e9a08bd53e3a9d473`
and `rust-toolchain.toml` by SHA-256
`fc7cfab9a268fadc2e10b4f9c07549ff3f6fb756984def6724a4347e9fb15b32`.
`scripts/verify_release_provenance.sh` verifies the recorded Git tree, source
files, resolved SDK version and checksum, installed tools, and every artifact
digest. This closes `VERIFY-09`. The release remains a mainnet candidate;
closing this evidence gate does not authorize deployment.

## CTO Gate Decisions

On 2026-09-01, the CTO approved freezing the current public contract
interfaces and event schemas represented by release source commit
`23c1fdd5818d320dc519b4a0f4408a481658aa5c` and Git tree
`f86e34ab4dc253dac5f95a717ce99403e9bc066b`. Any subsequent public interface
or event-schema change reopens this exit gate and requires new release WASMs,
provenance, and approval.

The Flow and Lockup fuzz harnesses required by `VERIFY-05` are present and
compile. The sanitizer-backed campaign could not run on the current macOS
x86_64 host because of the documented sanitizer/linker incompatibility. The
CTO approved marking `VERIFY-05` complete on 2026-09-01, with execution of the
campaign deferred to another compatible PC. This approval accepts the
temporary residual verification risk; it does not represent a successful fuzz
campaign or remove the obligation to record its eventual results.

## Release-WASM Exit Gate

`scripts/test_release_wasms.sh` first runs the release-provenance verifier and
then executes the `release_wasm_` integration suite through the Soroban host.
The tests import the checksum-pinned files from
`target/reproducible-release/` directly; they do not register the native Rust
contract implementations.

On 2026-09-01, `./scripts/test_release_wasms.sh` passed both required tests:

- Flow and Lockup creation and withdrawal through the release Router, token
  accounting, rejection of withdrawal by a non-owner, per-stream
  non-transferability enforcement, and Protocol 25 resource ceilings.
- Release Governance enforcement of the three-of-five normal threshold and
  48-hour timelock before rotating the release Router administrator.

The complete locked workspace regression suite also passed: 117 tests, zero
failures. This closes the final Phase 2 exit-gate item, “Required tests pass
against release-mode WASMs.”

## Current Verification

The Phase 2 interface, governance, and transferability changes are isolated in
commit `a9deef4` (`feat: implement governance contract for multisig upgrades and
add non-transferable stream enforcement logic`). The worktree was clean when
that commit was verified, satisfying `VERIFY-01`.

The workspace baseline and the phase 2 implementation pass all 117 unit,
property, and
cross-contract tests. Release-mode WASMs for Flow, Lockup, Stream NFT,
Governance, and Router reproduce byte-for-byte from clean source exports.

## Deferred Verification Follow-up

- Run the sanitizer-backed Flow and Lockup fuzz campaigns on a compatible PC
  and attach the commands, duration or run count, corpus, sanitizer, toolchain,
  and results to the release evidence.
