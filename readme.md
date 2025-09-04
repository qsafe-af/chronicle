# Chronicle

Quantum-safe, multi-chain indexer + GraphQL backend for Substrate-based chains using NIST-approved signature schemes (e.g., Dilithium). Chronicle walks each chain from genesis to head, records per-account balance deltas (including miner rewards for PoW), and serves flexible queries via Postgres/TimescaleDB and Hasura.

- Project homepage: this repository
- License: Apache-2.0

## Table of contents
- Overview
- Features
- Architecture
- Quick start (Podman Quadlet)
- Configuration
- Running Chronicle (Quadlet, Docker, Direct)
- Database schema and conventions
- Query examples
- Extending for your chain
- Performance considerations
- Development
- Contributing and support
- License

## Overview

Chronicle is built for post-quantum Substrate chains. It focuses on:
- Accurate, resumable indexing of balance-affecting events
- Per-chain database isolation using the base58-encoded genesis hash
- Zero-maintenance runtime version discovery and metadata storage
- A single Hasura instance that fronts multiple chain schemas for GraphQL

## Features

- Per-chain schema: each chain gets its own PostgreSQL schema named by the base58-encoded genesis hash
- Automatic runtime discovery: detects and stores runtime versions and metadata
- Balance tracking from genesis:
  - Genesis endowments
  - Miner rewards (PoW)
  - Transfers, fees, slashing, staking rewards
  - Reserved/unreserved changes
- Resumable indexing: continues from the last indexed block
- Optional TimescaleDB hypertables for time-series performance
- Connection pooling via `deadpool-postgres`
- Transaction safety: block writes are atomic

## Architecture

- `chronicled`: Rust daemon that ingests blocks and writes balance facts
- `crates/chron-db`: shared DB helpers (schema DDL, repository, models, connection)
- `orchestration/`: Podman Quadlet units to run TimescaleDB, Hasura, and `chronicled`
- `script/`: helper scripts (e.g., base58/hex conversion for chain IDs)

Typical layout:
- chronicled (main indexer binary)
  - `src/main.rs`: main indexer loop + runtime discovery
  - `src/balance_decoder.rs`: event decoding and balance change extraction
- chron-db (database abstraction layer)
  - `src/config.rs`, `connection.rs`, `models.rs`, `repository.rs`, `schema.rs`, `error.rs`
- orchestration
  - `quadlet/`: `.container` and `.volume` units
  - `config/`: env templates per chain

## Quick start (Podman Quadlet; user scope)

Requirements:
- Podman with systemd user services (Quadlet)
- PostgreSQL/TimescaleDB and Hasura containers are provided via Quadlet units

1) Install Quadlets:
~~~
mkdir -p ~/.config/containers/systemd
cp -v orchestration/quadlet/*.container orchestration/quadlet/*.volume ~/.config/containers/systemd/
systemctl --user daemon-reload
~~~

2) Configure environments (copy/edit; keep real secrets out of Git):
~~~
cp orchestration/config/chronicled-my-chain-id.env.example orchestration/config/chronicled-<base58-genesis-id>.env
# Edit the new env file to point to your node WS URL, DB DSN, etc.
~~~

3) Start DB + Hasura:
~~~
systemctl --user enable --now chronicle-timescaledb.service chronicle-hasura.service
~~~

4) Start an indexer instance (instance name is the base58 genesis ID):
~~~
systemctl --user enable --now chronicle-chronicled@<base58-genesis-id>.service
~~~

GraphQL will be available at:
- http://127.0.0.1:8080/ (single Hasura instance; query the schema matching your chain’s base58 genesis ID)

## Configuration

`chronicled` is configured via environment variables (Quadlet env files or process env):

- `WS_URL`: WebSocket endpoint of your quantum-safe Substrate node (e.g., `wss://a.t.res.fm`)
- `PG_DSN`: PostgreSQL DSN (e.g., `postgresql:///chronicle` or a full URL with auth/host)
- `ENABLE_TIMESCALE`: `true` to enable hypertable creation
- `DB_MAX_CONNECTIONS`: maximum DB connections (default 10)
- `DB_MIN_CONNECTIONS`: minimum DB connections (default 1)
- `RUST_LOG`: log level (`error`, `warn`, `info`, `debug`, `trace`; default `info`)

Example local run:
~~~
export WS_URL=wss://a.t.res.fm
export PG_DSN=postgresql:///chronicle
export RUST_LOG=info
target/release/chronicled
~~~

## Running Chronicle

1) Podman Quadlet (recommended)
- Use the Quick start above. One Hasura + TimescaleDB instance can serve multiple chains; start one `chronicled` unit per chain ID.

2) Docker
- Example containerization:
~~~
# Dockerfile
FROM rust:1.70 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/chronicled /usr/local/bin/
CMD ["chronicled"]
~~~
Build and run:
~~~
docker build -t chronicled .
docker run --rm \
  -e WS_URL=wss://a.t.res.fm \
  -e PG_DSN=postgres://chronicle:${PG_PASSWORD}@db:5432/chronicle \
  -e RUST_LOG=info \
  chronicled
~~~

3) Direct execution
- Prerequisites: Rust 1.70+, PostgreSQL 14+ (TimescaleDB optional)
- Build:
~~~
cargo build --release
~~~
- Binary at `target/release/chronicled`.

## Database schema and conventions

Per-chain isolation:
- Each chain is stored in a dedicated PostgreSQL schema named exactly the base58-encoded genesis hash.
- Drop a chain:
~~~
DROP SCHEMA "<base58-genesis-id>" CASCADE;
~~~

Canonical tables (per chain schema):
- `blocks`
  - `number` (bigint, PK)
  - `hash` (bytea)
  - `parent_hash` (bytea)
  - `timestamp` (timestamptz)
  - `is_canonical` (boolean)
  - `runtime_spec` (bigint)
- `metadata`
  - `spec_version` (int, PK), `impl_version` (int), `transaction_version` (int), `state_version` (int)
  - `first_seen_block` (bigint), `last_seen_block` (bigint null)
  - `metadata_bytes` (bytea), `metadata_hash` (bytea)
  - `created_at` (timestamptz), `updated_at` (timestamptz)
- `balance_changes`
  - `id` (bigserial, PK)
  - `account` (bytea)
  - `block_number` (bigint)
  - `event_index` (int)
  - `delta` (numeric(78,0))
  - `reason` (text)
  - `extrinsic_hash` (bytea)
  - `event_pallet` (text)
  - `event_variant` (text)
  - `block_ts` (timestamptz)
- `index_progress`
  - `chain_id` (text, PK)
  - `latest_block` (bigint)
  - `latest_block_hash` (bytea)
  - `latest_block_ts` (timestamptz)
  - `blocks_indexed` (bigint)
  - `balance_changes_recorded` (bigint)
  - `started_at` (timestamptz), `updated_at` (timestamptz)
- `account_stats`
  - `account` (bytea, PK)
  - `balance` (numeric(78,0))
  - `first_seen_block` (bigint)
  - `last_activity_block` (bigint)
  - `total_changes` (bigint)

Helpers:
- Base58/hex tools and examples in `script/`.

## Query examples

Indexing progress:
~~~
SELECT * FROM "CHAIN_BASE58".index_progress;
~~~

Account balance at a specific block:
~~~
SELECT SUM(delta::NUMERIC) AS balance
FROM "CHAIN_BASE58".balance_changes
WHERE account = '\xDEADBEEF...'::bytea
  AND block_number <= 123456;
~~~

Largest balance changes:
~~~
SELECT account, block_number, delta, reason, event_variant
FROM "CHAIN_BASE58".balance_changes
ORDER BY ABS(delta::NUMERIC) DESC
LIMIT 10;
~~~

Hasura:
- One Hasura instance fronts multiple schemas; query the schema matching your chain’s base58 genesis ID (configure Hasura to expose schemas as needed).

## Extending for your chain

You’ll adapt decoding to your runtime’s event structure (recommended: generate static types from metadata).

1) Event decoding (`balance_decoder.rs`)
~~~rust
fn decode_transfer_event(
    &self,
    event: &EventDetails<PolkadotConfig>,
    block_number: i64,
    event_index: i32,
    block_timestamp: DateTime<Utc>,
    extrinsic_hash: Option<Vec<u8>>,
) -> Result<Vec<BalanceChange>> {
    // Decode your chain's balances::Transfer (from, to, amount)
    // Return one negative delta for 'from' and one positive for 'to'
}
~~~

2) Genesis endowments
~~~rust
pub async fn query_genesis_endowments(&self) -> Result<Vec<BalanceChange>> {
    // Read System.Account at genesis and create Endowment deltas
}
~~~

3) Miner rewards (PoW)
~~~rust
pub async fn decode_miner_rewards(
    &self,
    block_hash: [u8; 32],
    block_number: i64,
    block_timestamp: DateTime<Utc>,
) -> Result<Vec<BalanceChange>> {
    // Extract author from digest, determine reward, create BalanceChange
}
~~~

4) Using static types (recommended)
- Generate types with `subxt-cli`:
~~~
cargo install subxt-cli
subxt metadata -f bytes --url wss://a.t.res.fm > metadata.scale
~~~
- Use in code:
~~~rust
#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod runtime {}

// Example:
let transfer = event.as_event::<runtime::balances::events::Transfer>()?;
~~~

## Performance considerations

- Batch processing: consider committing multiple blocks per transaction for throughput
- Parallel historical sync: parallelize catch-up while keeping the tip synchronized
- TimescaleDB: enable and tune for time-series queries
- Indexes: add/adjust secondary indexes based on query patterns

## Development

Prerequisites:
- Rust 1.70+, PostgreSQL 14+ (TimescaleDB optional for local dev)

Build:
~~~
cargo build --release
~~~

Run:
~~~
cargo run -p chronicled
~~~

Test:
~~~
cargo test
~~~

Logging:
~~~
RUST_LOG=debug,chronicled=trace cargo run -p chronicled
~~~

Binary:
- `target/release/chronicled`

## Contributing and support

Contributions are welcome. Please:
- Follow Rust best practices
- Include tests for new features
- Update documentation

For questions or issues, open an issue in this repository.

## License

Apache-2.0