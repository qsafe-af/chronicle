# Chronicle - Quantum-Safe Blockchain Indexer

Chronicle is a high-performance blockchain indexer exclusively for quantum-safe Substrate-based chains using NIST-approved signature schemes like Dilithium. It tracks account balances by walking the chain from genesis to head, recording all balance-affecting events. Chronicle is designed specifically for post-quantum blockchains, as traditional chains using ECDSA or *25519 signatures will be obsolete in the quantum era.

## Features

- **Per-Chain Schema**: Each indexed chain gets its own PostgreSQL schema named by the base58-encoded genesis hash
- **Automatic Runtime Discovery**: Detects and stores all runtime versions automatically
- **Balance Tracking**: Records all balance changes from genesis, including:
  - Genesis endowments
  - Miner rewards (PoW chains)
  - Transfers
  - Fees
  - Slashing events
  - Staking rewards
  - Reserved/unreserved balances
- **Resumable Indexing**: Tracks progress to resume from the last indexed block after restarts
- **Runtime Version Management**: Stores metadata for all runtime versions for proper event decoding
- **TimescaleDB Support**: Optional hypertable support for time-series optimization
- **Connection Pooling**: Efficient database connection management with deadpool-postgres
- **Transaction Safety**: All block data is written atomically

## Architecture

```
chronicle/
├── chronicled/           # Main indexer binary
│   └── src/
│       ├── main.rs      # Main indexer loop with integrated runtime discovery
│       └── balance_decoder.rs  # Event decoding logic
└── crates/
    └── chron-db/        # Database abstraction layer
        └── src/
            ├── config.rs       # Database configuration
            ├── connection.rs   # Connection pool management
            ├── error.rs        # Error types
            ├── models.rs       # Data models
            ├── repository.rs   # Data access patterns
            └── schema.rs       # Schema management
```

## Installation

### Prerequisites

- Rust 1.70+
- PostgreSQL 14+
- TimescaleDB (optional, for hypertable support)

### Building

```bash
cd chronicle
cargo build --release
```

The binary will be available at `target/release/chronicled`.

## Configuration

The indexer is configured via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `WS_URL` | WebSocket endpoint of the quantum-safe blockchain node | `wss://a.t.res.fm` (Resonance) |
| `PG_DSN` | PostgreSQL connection string | `postgres://res:change-me@127.0.0.1:5432/res_index` |
| `ENABLE_TIMESCALE` | Enable TimescaleDB hypertables | `false` |
| `DB_MAX_CONNECTIONS` | Maximum database connections | `10` |
| `DB_MIN_CONNECTIONS` | Minimum database connections | `1` |
| `RUST_LOG` | Logging level | `info` |

## Database Schema

Each indexed chain has its own schema with the following tables:

### `blocks`
- `number` (BIGINT PRIMARY KEY): Block height
- `hash` (BYTEA): Block hash
- `parent_hash` (BYTEA): Parent block hash
- `timestamp` (TIMESTAMPTZ): Block timestamp
- `is_canonical` (BOOLEAN): Whether block is on the canonical chain
- `runtime_spec` (BIGINT): Runtime version

### `metadata`
- `spec_version` (INT PRIMARY KEY): Runtime spec version
- `impl_version` (INT): Implementation version
- `transaction_version` (INT): Transaction version
- `state_version` (INT): State version
- `first_seen_block` (BIGINT): First block using this runtime
- `last_seen_block` (BIGINT): Last block using this runtime (NULL if current)
- `metadata_bytes` (BYTEA): SCALE-encoded metadata
- `metadata_hash` (BYTEA): SHA256 hash of metadata
- `created_at` (TIMESTAMPTZ): When record was created
- `updated_at` (TIMESTAMPTZ): Last update time

### `balance_changes`
- `id` (BIGSERIAL PRIMARY KEY): Auto-incrementing ID
- `account` (BYTEA): Account address
- `block_number` (BIGINT): Block where change occurred
- `event_index` (INT): Event index within block
- `delta` (NUMERIC(78,0)): Balance change amount
- `reason` (TEXT): Reason for change
- `extrinsic_hash` (BYTEA): Associated extrinsic hash
- `event_pallet` (TEXT): Source pallet
- `event_variant` (TEXT): Event name
- `block_ts` (TIMESTAMPTZ): Block timestamp

### `index_progress`
- `chain_id` (TEXT PRIMARY KEY): Base58 genesis hash
- `latest_block` (BIGINT): Last indexed block number
- `latest_block_hash` (BYTEA): Last indexed block hash
- `latest_block_ts` (TIMESTAMPTZ): Last block timestamp
- `blocks_indexed` (BIGINT): Total blocks indexed
- `balance_changes_recorded` (BIGINT): Total balance changes
- `started_at` (TIMESTAMPTZ): Indexing start time
- `updated_at` (TIMESTAMPTZ): Last update time

### `account_stats`
- `account` (BYTEA PRIMARY KEY): Account address
- `balance` (NUMERIC(78,0)): Current balance
- `first_seen_block` (BIGINT): First appearance
- `last_activity_block` (BIGINT): Last activity
- `total_changes` (BIGINT): Total balance changes

## Running the Indexer

### With systemd (using quadlet)

Create a container file at `/etc/containers/systemd/chronicled.container`:

```ini
[Unit]
Description=Chronicle Blockchain Indexer
After=network-online.target postgresql.service

[Container]
Image=localhost/chronicled:latest
Environment=WS_URL=wss://a.t.res.fm
Environment=PG_DSN=postgres://user:pass@localhost/indexdb
Environment=RUST_LOG=info
Restart=always

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
```

Then start the service:
```bash
systemctl daemon-reload
systemctl start chronicled
```

### Docker

```dockerfile
FROM rust:1.70 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/chronicled /usr/local/bin/
CMD ["chronicled"]
```

Build and run:
```bash
docker build -t chronicled .
docker run -e WS_URL=wss://a.t.res.fm \
           -e PG_DSN=postgres://user:pass@db/indexdb \
           chronicled
```

### Direct Execution

```bash
# For Resonance testnet (quantum-safe PoW)
export WS_URL=wss://a.t.res.fm
# Or Heisenberg testnet (quantum-safe)
# export WS_URL=wss://a.i.res.fm
export PG_DSN=postgres://user:pass@localhost/indexdb
export RUST_LOG=info
./target/release/chronicled
```

## Runtime Version Management

Chronicle automatically discovers and stores all runtime versions:

1. **On First Run**: Scans from genesis to current block for all runtime versions
2. **During Operation**: Detects runtime upgrades as they happen in real-time
3. **Storage**: Saves metadata in the database for version-aware event decoding
4. **No Manual Steps**: Everything happens automatically - no separate scanning required

The metadata is stored in the database and used to ensure events are decoded with the correct runtime version for any historical block.

## Extending for Your Chain

The indexer provides a foundation that needs to be customized for your specific chain's event structure. Key areas to modify:

### 1. Event Decoding (`balance_decoder.rs`)

Implement the event decoding methods based on your runtime's specific event structures:

```rust
fn decode_transfer_event(
    &self,
    event: &EventDetails<PolkadotConfig>,
    block_number: i64,
    event_index: i32,
    block_timestamp: DateTime<Utc>,
    extrinsic_hash: Option<Vec<u8>>,
) -> Result<Vec<BalanceChange>> {
    // Decode your chain's Transfer event structure
    // Extract from, to, and amount fields
    // Create BalanceChange records
}
```

### 2. Genesis Endowments

Implement the `query_genesis_endowments` method to read initial balances from your chain's genesis storage:

```rust
pub async fn query_genesis_endowments(&self) -> Result<Vec<BalanceChange>> {
    // Query System.Account storage at genesis
    // Decode AccountInfo to extract balances
    // Create BalanceChange records with reason Endowment
}
```

### 3. Miner Rewards (for PoW chains)

Implement `decode_miner_rewards` to extract block author and calculate rewards:

```rust
pub async fn decode_miner_rewards(
    &self,
    block_hash: [u8; 32],
    block_number: i64,
    block_timestamp: DateTime<Utc>,
) -> Result<Vec<BalanceChange>> {
    // Extract block author from seal/digest
    // Calculate or query block reward
    // Create BalanceChange for miner
}
```

### 4. Using Static Types (Recommended)

For better type safety, generate static types from your chain's metadata:

```bash
# Install subxt-cli
cargo install subxt-cli

# Download metadata from your quantum-safe node
subxt metadata -f bytes --url wss://a.t.res.fm > metadata.scale

# Add to your balance_decoder.rs:
#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod runtime {}

# Use generated types for decoding:
let transfer = event.as_event::<runtime::balances::events::Transfer>()?;
```

## Monitoring

### Check Indexing Progress

```sql
SELECT * FROM "CHAIN_ID".index_progress;
```

### Get Account Balance at Specific Block

```sql
SELECT SUM(delta::NUMERIC) as balance 
FROM "CHAIN_ID".balance_changes 
WHERE account = '\xYOUR_ACCOUNT_BYTES'
  AND block_number <= YOUR_BLOCK_NUMBER;
```

### Find Largest Balance Changes

```sql
SELECT account, block_number, delta, reason, event_variant
FROM "CHAIN_ID".balance_changes
ORDER BY ABS(delta::NUMERIC) DESC
LIMIT 10;
```

## Performance Considerations

1. **Batch Processing**: The indexer processes blocks one at a time for simplicity. For better performance, consider batching multiple blocks per transaction.

2. **Parallel Processing**: For initial sync, you could process historical blocks in parallel while keeping the head synchronized.

3. **TimescaleDB**: Enable TimescaleDB for better query performance on time-series data:
   ```sql
   CREATE EXTENSION IF NOT EXISTS timescaledb;
   ```

4. **Indexes**: The schema includes indexes on commonly queried fields. Add more based on your query patterns.

## Development

### Running Tests

```bash
cargo test
```

### Logging

Set `RUST_LOG` environment variable to control log verbosity:
- `error`: Only errors
- `warn`: Warnings and errors
- `info`: General information (default)
- `debug`: Detailed debugging
- `trace`: Very verbose

Example:
```bash
RUST_LOG=debug,chronicled=trace cargo run
```

## Contributing

Contributions are welcome! Please ensure:
1. Code follows Rust best practices
2. All tests pass
3. New features include tests
4. Documentation is updated

## License

Apache-2.0

## Support

For issues and questions, please open an issue on the project repository.