# Phase 5: Indexing and Reconciliation Implementation

Status: Ready for review — backend implementation complete; testnet operational
qualification remains. Last updated: 2026-09-09.

## Implemented scope

The working tree of `backend-main` now contains:

- `payment-stream-indexer.service.ts`: scheduled ingestion, persistent deployment
  checkpoints, database row locking, and atomic event/checkpoint commits.
- `payment-stream-indexer-rpc.service.ts`: network verification, bounded RPC
  polling, pagination, failover, and retained-history gap detection.
- `payment-stream-indexer.utils.ts`: deployment validation and lossless XDR
  decoding, including decimal-string serialization of large integers.
- Migration `0032_payment-stream-indexer.sql`: `PaymentStreamIndexedEvent` and
  `PaymentStreamIndexerCheckpoint`, plus the corresponding Drizzle snapshot.
- Migration `0033_payment-stream-indexer-processing.sql`: durable semantic
  processing state, bounded retries, and operator-visible failure codes.
- `payment-stream-reconciliation.service.ts`: event-to-stream mapping,
  canonical projection, pending-submission confirmation, periodic chain-state
  reconciliation, and mismatch/failure alerts.
- `tests/payment-stream-indexer.spec.ts`: regression coverage for configuration,
  contract filtering, large integers, pagination, failover, gaps, network
  mismatches, failed-call filtering, replay, failed writes, and concurrent workers.

The inbox preserves events emitted by all five configured contract roles. A
separate consumer resolves public IDs from Router/NFT events or established
core mappings, queries `Router.get_stream` and the appropriate core contract,
then applies owner, lifecycle, token, schedule, balance, debt, refund, and
settlement fields through the existing atomic projection service. Submission
confirmation remains coupled to indexed chain effects.

## Deployment configuration

In `backend-main`, set `STELLAR_STREAM_INDEXER_DEPLOYMENTS` to a JSON array:

```json
[
  {
    "network": "TESTNET",
    "startLedger": 123456,
    "querySource": "<funded public G-address used for read simulations>",
    "rpcUrls": ["https://primary-rpc.example", "https://backup-rpc.example"],
    "contracts": {
      "router": "<verified Router C-address>",
      "flow": "<verified Flow C-address>",
      "lockup": "<verified Lockup C-address>",
      "streamNft": "<verified Stream NFT C-address>",
      "feeForwarder": "<verified OZ FeeForwarder C-address>"
    }
  }
]
```

These values are placeholders. Select actual addresses and the earliest
relevant deployment ledger from verified deployment evidence. The start ledger
is inclusive. The query source must be a funded account on the configured
network; it signs nothing because contract reads are simulated. All five
distinct contract addresses and at least one HTTPS RPC URL are required. The
configuration is optional; an absent value disables polling. Invalid
configuration prevents worker initialization. RPC URLs remain runtime-only and
are never persisted or logged.

Apply migrations 0032 and 0033 before enabling this configuration. They have
been generated locally, **not applied to the live database**. No worker has been
enabled against testnet or mainnet as part of this implementation.

The checkpoint scope hashes network, start ledger, and ordered contract roles.
Changing an RPC URL preserves the scope. Changing contract addresses or the
start ledger creates a new scope that replays from its configured beginning.
Operators must retire old deployment configuration deliberately; changing
configuration does not delete existing inbox history.

## Ingestion guarantees and limits

Every ten seconds, the worker attempts up to 100 ledgers per deployment. Each
batch fetches at most 100 pages of 1,000 events; every RPC request has a 15-second
timeout. A process-local guard prevents overlapping runs. Across replicas, a
checkpoint row lock and comparison reject a batch if another replica already
advanced the checkpoint. RPC fetching happens outside the database transaction.

The initial request uses an inclusive start ledger and exclusive end ledger.
Pagination requests omit both bounds and use the returned cursor. The worker
retains a fixed upper ledger boundary, continues short nonempty pages, and
commits only after an empty page or events beyond that boundary establish the
end of the batch. This follows the official
[Stellar getEvents contract](https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getEvents).

Provider failure discards the in-memory partial batch and starts the same range
on the next configured provider. Network passphrases are checked before reading
events. If no provider retains the required ledger, the checkpoint stays put and
the worker emits a structured `gap` error. Operators must supply a provider with
that history or implement archive backfill; automatically jumping to the oldest
available ledger would lose activity. Repeated cursors, malformed events, and
page limits also stop advancement.

A successful database transaction inserts all raw events and updates the last
fully ingested ledger together. Empty ranges also advance. Uniqueness within a
scope on both RPC event ID and transaction hash/event index makes replay a
conflict no-op. Failed-call events are excluded. Original XDR, decoded topics,
data, source role, transaction identity, and ledger close time are retained.
Structured error logs omit provider URLs and raw provider failures.

The ingestion checkpoint means **durably fetched**, not reconciled. Each inbox
row has independent processing state. Projection failures remain retryable for
ten attempts, after which the stored error code and failure metric make the row
operator-visible without blocking later events. Replays remain safe because
both the inbox and public activity tables enforce chain-event identity.

Every minute, the worker also walks all non-terminal canonical streams using
keyset pagination and re-queries Router/core state. Chain values overwrite a
stale projection. Owner, lifecycle, or accounting disagreement increments
`fundable_stream_reconciliation_mismatches_total`; query and projection
failures increment `fundable_stream_reconciliation_failures_total`. Both carry
bounded `network` and `reason` labels for alert rules.

The existing authenticated detail and activity endpoints load deployment-scoped
canonical rows from PostgreSQL by database ID or NFT token ID. They accept an
optional `stream_nft_contract` query scope to disambiguate redeployments. User
lists and dashboard statistics exclude legacy browser-authored Stellar rows and
use only reconciled chain projections. Pending submissions continue to recover
through the Phase 3 restart worker and become confirmed only with indexed chain
effects.

## Verification and checklist mapping

Local validation on 2026-09-09:

- Focused indexer, submission, and monitoring suites: 30/30 tests passed.
- Backend Nest/TypeScript build passed.
- Scoped ESLint and whitespace validation passed after formatting fixes.
- Both Drizzle migrations and snapshots generated successfully.

The database transaction tests use an in-memory transactional adapter. They do
not establish PostgreSQL rollback/locking behavior under real connections.
The local runtime was Node 20.11.0; the repository declares Node 22.x. CI must
repeat validation on the declared runtime before release.

INDEX-01 through INDEX-18 have implementation evidence and are checked as ready
for review. The three phase exit gates remain open until migrations are applied
to a controlled database and the testnet fresh-device/replay qualification is
recorded.

## Operational qualification still required

1. Apply migrations to a controlled test database and test restart, concurrent
   replicas, mid-page/provider/database failure, replay, and fresh-device reads.
   Record testnet evidence before checking off the phase exit gates.
2. Configure alert thresholds for both reconciliation counters in the production
   monitoring system and exercise them during testnet qualification.
3. Repeat the suite on the repository's declared Node 22 runtime; local
   verification used Node 20.11.0.
