---
name: fundable-soroban-contracts
description: >
  Development guidelines for Fundable's Soroban smart contract workspace.
  Enforces security-first development by integrating Stellar Security Portal
  vulnerability patterns and official Stellar documentation into every iteration.
---

# Fundable Soroban Contracts — Development Skill

## Project Overview

Fundable is a token streaming/vesting protocol built on Stellar's Soroban smart
contract platform. It implements Sablier-inspired patterns (Flow + Lockup) for
continuous payment streams, lockup schedules, and composable NFT receipts.

### Workspace Structure

```
fundable-soroban-contracts/
├── contracts/
│   ├── flow/          # Open-ended, rate-per-second streaming
│   ├── lockup/        # Fixed-term vesting with cliff/linear/dynamic curves
│   ├── router/        # Unified entrypoint for creating streams
│   ├── stream-nft/    # NFT receipts representing stream positions
│   └── hello-world/   # Scaffold (to be replaced)
├── packages/          # Shared libraries (types, math, events)
├── docs/              # Architecture & design documents
├── Cargo.toml         # Workspace root (soroban-sdk 22.0.0)
└── SKILL.md           # ← This file
```

---

## ⚠️ MANDATORY: Security Review Checklist

**Every code change MUST be evaluated against the following checklist before
being considered complete.** This checklist is derived from:

- **Stellar Security Portal**: https://stellarsecurityportal.com/vulnerabilities
- **Stellar Security Best Practices**: https://developers.stellar.org/docs/build/security-docs
- **Stellar Threat Modeling (STRIDE)**: https://developers.stellar.org/docs/build/security-docs/threat-modeling
- **Soroban Audit Bank findings** (Veridise, CertiK, CoinFabrik)

### 1. Authorization & Access Control

- [ ] **Every privileged function uses `require_auth()`** — Never rely on
      implicit caller checks. Always call `address.require_auth()` for the
      specific address that should authorize the action.
- [ ] **Admin functions are gated** — Functions like `set_admin`, `pause`,
      `upgrade`, and `set_protocol_fee` must verify the caller is the current
      admin.
- [ ] **No missing auth on token transfers** — Any `token::Client::transfer()`
      must be preceded by `from.require_auth()` to prevent unauthorized drains.
- [ ] **Principle of least privilege** — Contracts should request only the
      minimum permissions needed. Avoid broad admin capabilities.
- [ ] **Multi-sig / timelock for critical operations** — Upgrades and admin
      transfers should consider timelock or multi-sig patterns.

### 2. Integer Arithmetic & Precision

- [ ] **Use checked arithmetic** — Although Rust/Soroban prevents overflow by
      default in debug mode, release builds with `overflow-checks = true` (set
      in our workspace `Cargo.toml`) enforce this. Never disable it.
- [ ] **Fixed-point math for rates** — Streaming rate calculations
      (tokens-per-second) must use sufficient precision. Prefer scaling by 1e18
      before division to avoid truncation.
- [ ] **Rounding direction is deliberate** — When computing withdrawable
      amounts, round DOWN for the recipient (protocol keeps dust). Document
      rounding decisions.
- [ ] **No division by zero** — Guard all division operations, especially in
      rate calculations where `duration` or `total_amount` could be zero.

### 3. Storage & State Management

- [ ] **Bounded data structures** — Never store unbounded `Vec<T>` or
      `Map<K, V>`. Use pagination or capped collections. The 64KB instance
      storage limit is a hard ceiling.
- [ ] **Correct storage types** — Use `Persistent` for long-lived data (stream
      records), `Temporary` for ephemeral data (approvals), `Instance` for
      contract-wide config.
- [ ] **TTL management** — All persistent storage entries must have explicit TTL
      extension calls to prevent state archival. Budget TTL costs in fees.
- [ ] **Storage key collisions** — Use typed enums for storage keys, never raw
      strings. Namespace keys to avoid collisions between contract modules.
- [ ] **State consistency** — Ensure atomic state transitions. If a function
      updates multiple storage entries, all writes should succeed or the
      transaction should revert.

### 4. Denial of Service (DoS) Prevention

- [ ] **No unbounded loops** — Avoid iterating over data structures that can
      grow without limit. Use pagination patterns.
- [ ] **Resource budget awareness** — Soroban has strict CPU and memory limits
      per transaction. Test that complex operations stay within budget.
- [ ] **No griefing vectors** — Ensure users cannot create situations where
      other users' transactions become unfeasible (e.g., by bloating shared
      state).

### 5. Token Handling

- [ ] **SAC compatibility** — All token interactions must work with both Stellar
      Asset Contract (SAC) tokens and custom SEP-41 tokens.
- [ ] **No token assumptions** — Don't assume `decimals()`, `name()`, or
      `symbol()` exist. Handle the case where the token contract doesn't
      implement optional metadata functions.
- [ ] **Transfer-before-state-update pattern** — Update internal state AFTER
      successful token transfers to prevent inconsistencies on transfer failure.
- [ ] **Allowance validation** — If using token allowances, verify sufficient
      allowance before operations and handle approval edge cases.

### 6. Time & Ledger Dependencies

- [ ] **Use `env.ledger().timestamp()`** — For all time-dependent logic
      (streaming, cliffs, unlock schedules). Never use sequence numbers for
      time.
- [ ] **Time boundary checks** — Validate that `start_time < end_time` and
      that `cliff_time` falls within the stream's time window.
- [ ] **Handle active streams** — Functions that modify or cancel streams must
      correctly calculate accrued amounts up to the current timestamp.
- [ ] **No timestamp manipulation assumptions** — While Stellar's consensus is
      more predictable than PoW, don't rely on exact second-level precision.

### 7. Upgradeability & Migration

- [ ] **Admin-only upgrades** — `env.deployer().update_current_contract_wasm()`
      must only be callable by an authorized admin.
- [ ] **Data migration safety** — When upgrading, ensure old storage entries
      are readable by the new code. Version your storage schemas.
- [ ] **Event emission on upgrade** — Emit an event on contract upgrade for
      off-chain indexer awareness.

### 8. Events & Observability

- [ ] **Emit events for all state changes** — Stream creation, withdrawal,
      cancellation, pause, rate changes, and admin changes must emit events.
- [ ] **Structured event topics** — Use consistent topic naming:
      `("stream_created", stream_id)`, `("withdrawal", stream_id, amount)`, etc.
- [ ] **No sensitive data in events** — Events are public. Don't log internal
      calculations or intermediate states that could leak information.

### 9. Cross-Contract Call Safety

- [ ] **Validate return values** — Always check return values from cross-contract
      calls (e.g., token transfers, NFT mints).
- [ ] **Minimize cross-contract calls** — Each call adds to the transaction's
      resource budget. Batch where possible.
- [ ] **Document trust assumptions** — Clearly document which external contracts
      are trusted and which are user-supplied.

### 10. Testing Requirements

- [ ] **Unit tests for every public function** — Cover happy path, edge cases,
      and error conditions.
- [ ] **Integration tests with mock tokens** — Test the full flow: create stream
      → deposit → withdraw → cancel.
- [ ] **Fuzz testing for math-heavy functions** — Rate calculations, pro-rata
      splits, and curve computations should be fuzzed.
- [ ] **Authorization failure tests** — Verify that unauthorized callers are
      rejected for every privileged function.
- [ ] **Boundary value tests** — Test with zero amounts, max u128 values,
      same start/end time, and single-second durations.

---

## Stellar Official Documentation References

When implementing any contract feature, consult these docs:

| Topic | URL |
|-------|-----|
| **Example Contracts** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts |
| **Source Examples (GitHub)** | https://github.com/stellar/soroban-examples |
| **Contract Authorization** | https://developers.stellar.org/docs/build/guides/auth |
| **Contract Storage** | https://developers.stellar.org/docs/build/guides/storage |
| **Contract Events** | https://developers.stellar.org/docs/build/guides/events |
| **Contract Testing** | https://developers.stellar.org/docs/build/guides/testing |
| **Fees & Metering** | https://developers.stellar.org/docs/build/guides/fees |
| **State Archival** | https://developers.stellar.org/docs/build/guides/archival |
| **Token Interface (SEP-41)** | https://developers.stellar.org/docs/tokens/token-interface |
| **Stellar Asset Contract** | https://developers.stellar.org/docs/tokens/stellar-asset-contract |
| **Fungible Token (OZ)** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts/fungible-token |
| **Upgradeable Contract** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts/upgradeable-contract |
| **Timelock** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts/timelock |
| **Atomic Swap** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts/atomic-swap |
| **Security Best Practices** | https://developers.stellar.org/docs/build/security-docs |
| **Threat Modeling (STRIDE)** | https://developers.stellar.org/docs/build/security-docs/threat-modeling |
| **Securing Web Projects** | https://developers.stellar.org/docs/build/security-docs/securing-web-based-projects |
| **OpenZeppelin Contracts** | https://developers.stellar.org/docs/tools/openzeppelin-contracts |
| **Fuzz Testing** | https://developers.stellar.org/docs/build/smart-contracts/example-contracts/fuzzing |
| **Resource Limits & Fees** | https://developers.stellar.org/docs/networks/resource-limits-fees |

### Key Example Contracts to Reference

These Stellar example contracts are directly relevant to Fundable's implementation:

1. **Timelock** — Token lockup with time-based release conditions
2. **Atomic Swap** — Authorized token exchanges between parties
3. **Tokens** — CAP-46-6 compliant token implementation
4. **Auth** — Authentication and authorization patterns
5. **Deployer** — Factory pattern for deploying contracts
6. **Upgradeable Contract** — Wasm bytecode upgrade pattern
7. **Storage** — Data persistence with increment pattern
8. **Events** — Publishing structured events
9. **Cross Contract Calls** — Inter-contract communication
10. **Fungible Token (OpenZeppelin)** — Audited token implementation

---

## Security Portal Integration

### Stellar Security Portal

**URL**: https://stellarsecurityportal.com/vulnerabilities

The Stellar Security Portal (formerly Soroban Security Catalogue) is a
community-funded initiative that centralizes audit reports, vulnerability
disclosures, and security tooling for the Soroban ecosystem.

**Before every PR or major change:**

1. Visit https://stellarsecurityportal.com/vulnerabilities
2. Check for new vulnerability disclosures relevant to:
   - Token streaming / vesting contracts
   - SAC token interactions
   - Authorization patterns
   - Time-dependent contract logic
   - Storage management patterns
3. Cross-reference any new findings against the contract being modified
4. Document any relevant findings in the PR description

### Recommended Security Tools

| Tool | Purpose | Link |
|------|---------|------|
| **Scout** | Static analysis / vulnerability detector for Soroban (Dylint-based) | https://github.com/AuditTools/soroban-scout |
| **Soroban Audit Bank** | SDF-funded professional audit program | Contact SDF |
| **cargo-audit** | Rust dependency vulnerability scanner | `cargo install cargo-audit` |
| **cargo-deny** | License and dependency policy enforcement | `cargo install cargo-deny` |

---

## Soroban Platform Security Properties

Understand what Soroban handles for you (and what it doesn't):

### Built-in Protections (DO NOT re-implement)

| Risk | Status | Notes |
|------|--------|-------|
| Reentrancy | ✅ Prevented by architecture | Soroban disallows re-entrant calls |
| Memory safety | ✅ Rust guarantees | No buffer overflows, use-after-free |
| Integer overflow | ✅ Checked in release | `overflow-checks = true` in workspace |
| `delegatecall` exploits | ✅ Not applicable | No equivalent in Soroban |

### Developer Responsibility (MUST implement)

| Risk | Status | Notes |
|------|--------|-------|
| Authorization | ⚠️ Manual | Must call `require_auth()` correctly |
| Logic errors | ⚠️ Manual | Business logic must match spec |
| Storage bounds | ⚠️ Manual | Cap collections, manage TTLs |
| DoS via resource exhaustion | ⚠️ Manual | Budget-aware operations |
| Rounding errors | ⚠️ Manual | Explicit rounding direction |
| State archival | ⚠️ Manual | Extend TTLs for critical data |

---

## Development Workflow

### For Every Contract Change

```
1. DESIGN    → Document the change in /docs/ with trust assumptions
2. IMPLEMENT → Write the code following this SKILL.md checklist
3. TEST      → Unit tests + integration tests + auth failure tests
4. REVIEW    → Run security checklist (Section "MANDATORY" above)
5. PORTAL    → Check stellarsecurityportal.com/vulnerabilities for new findings
6. AUDIT     → For major features, request review via Soroban Audit Bank
```

### Build & Test Commands

```bash
# Build all contracts
make build
# or
cargo build --release --target wasm32-unknown-unknown

# Run all tests
cargo test

# Build with debug logs enabled
cargo build --profile release-with-logs --target wasm32-unknown-unknown

# Check for dependency vulnerabilities
cargo audit

# Run Scout static analysis (if installed)
cargo scout-audit
```

---

## Contract-Specific Guidelines

### Flow Contract (`contracts/flow/`)

- Implements open-ended, rate-per-second streaming
- Rate can be adjusted mid-stream (with accrual snapshot)
- Must handle: create, deposit, withdraw, pause, resume, void, adjust_rate
- Critical math: `withdrawable = ratePerSecond × elapsedTime - alreadyWithdrawn`

### Lockup Contract (`contracts/lockup/`)

- Implements fixed-term vesting with cliff/linear/dynamic curves
- Stream shape is immutable after creation
- Must handle: create, withdraw, cancel (if cancelable)
- Critical math: curve-based unlock schedule calculations

### Router Contract (`contracts/router/`)

- Single entrypoint for creating streams across Flow and Lockup
- Must validate parameters before delegating to sub-contracts
- Authorization passes through to underlying contracts

### Stream NFT Contract (`contracts/stream-nft/`)

- Mints NFT receipts representing stream positions
- Transfer of NFT = transfer of stream recipient rights
- Must sync NFT ownership with stream recipient on transfer

---

## Coding Conventions

- `#![no_std]` — All contracts must be no_std compatible
- Use `soroban_sdk` types (`Address`, `BytesN`, `String`, `Vec`, `Map`)
- Define errors as `#[contracterror]` enums with descriptive names
- Use `#[contracttype]` for all custom data structures
- Emit events via `env.events().publish()` for every state mutation
- Document public functions with `///` doc comments
- Keep contract binary size minimal (< 64KB per contract)
