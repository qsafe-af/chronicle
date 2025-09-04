# Chronicle Multi-Chain GraphQL API

## Overview

Chronicle is designed to index multiple blockchain networks simultaneously, with each chain's data stored in its own PostgreSQL schema. Hasura GraphQL Engine provides a unified GraphQL API to query data across all indexed chains.

## Architecture

### Chain Identification

Each blockchain is identified by its **Chain ID**, which is the base58-encoded genesis block hash. This ensures a unique identifier for every chain.

Example Chain IDs:
- Resonance Testnet: `FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i` (Quantum-safe PoW chain)
- Heisenberg Testnet: `7wwWHpnmxRf1Cnvri1cQ6LUiwvDw1czPjL7ytnoxNNNg` (Quantum-safe chain)

Chronicle exclusively indexes quantum-safe chains using NIST-approved signature schemes like Dilithium, as traditional chains using ECDSA or *25519 will be obsolete post-quantum.

### Database Schema Structure

Each indexed chain gets its own PostgreSQL schema named after the chain ID:

```
chronicle (database)
├── public (default schema)
├── FnX4ttSwm8kTZUvUkDbyPYS2... (Resonance schema)
│   ├── blocks
│   ├── balance_changes
│   ├── index_progress
│   ├── account_stats
│   └── metadata
├── 7wwWHpnmxRf1Cnvri1cQ6LU... (Heisenberg schema)
│   ├── blocks
│   ├── balance_changes
│   └── ...
└── ... (other quantum-safe chains)
```

## Setting Up Hasura

### 1. Deploy Hasura

```bash
# Deploy Hasura GraphQL Engine
sudo ./scripts/deploy-hasura-podman.sh

# Check status
sudo ./scripts/deploy-hasura-podman.sh status
```

### 2. Track Chronicle Tables

After Chronicle has indexed at least one chain:

```bash
# Interactive mode - shows all indexed chains
./scripts/hasura-track-tables.sh

# Track specific chain (Resonance)
./scripts/hasura-track-tables.sh FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i

# Or track Heisenberg
./scripts/hasura-track-tables.sh 7wwWHpnmxRf1Cnvri1cQ6LUiwvDw1czPjL7ytnoxNNNg
```

### 3. Access Hasura Console

Open http://localhost:8080/console in your browser and enter the admin secret (from the HASURA_ADMIN_SECRET environment variable).

## GraphQL Schema

### Table Structure

Each chain schema contains the following tables:

#### `blocks`
- `number` (bigint): Block number
- `hash` (bytea): Block hash
- `parent_hash` (bytea): Parent block hash
- `timestamp` (timestamptz): Block timestamp
- `is_canonical` (boolean): Whether block is on canonical chain
- `runtime_spec` (bigint): Runtime specification version
- `created_at` (timestamptz): When record was created

#### `balance_changes`
- `id` (bigserial): Unique identifier
- `account` (bytea): Account address
- `block_number` (bigint): Block number where change occurred
- `event_index` (int): Event index within block
- `delta` (numeric): Balance change amount
- `reason` (text): Reason for balance change
- `extrinsic_hash` (bytea): Associated extrinsic hash (if any)
- `event_pallet` (text): Pallet that emitted the event
- `event_variant` (text): Event variant name
- `block_ts` (timestamptz): Block timestamp
- `created_at` (timestamptz): When record was created

#### `account_stats`
- `account` (bytea): Account address
- `balance` (numeric): Current balance
- `first_seen_block` (bigint): First block where account was seen
- `last_activity_block` (bigint): Last block with account activity
- `total_changes` (bigint): Total number of balance changes
- `updated_at` (timestamptz): Last update time

#### `index_progress`
- `chain_id` (text): Chain identifier
- `latest_block` (bigint): Latest indexed block
- `latest_block_hash` (bytea): Hash of latest block
- `latest_block_ts` (timestamptz): Timestamp of latest block
- `blocks_indexed` (bigint): Total blocks indexed
- `balance_changes_recorded` (bigint): Total balance changes recorded
- `started_at` (timestamptz): When indexing started
- `updated_at` (timestamptz): Last update time

#### `metadata`
- `spec_version` (int): Runtime spec version
- `impl_version` (int): Implementation version
- `transaction_version` (int): Transaction version
- `state_version` (int): State version
- `first_seen_block` (bigint): First block with this runtime
- `last_seen_block` (bigint): Last block with this runtime
- `metadata_bytes` (bytea): Raw metadata bytes
- `metadata_hash` (bytea): Hash of metadata
- `created_at` (timestamptz): When record was created

### Relationships

- `blocks` → `balance_changes[]`: One-to-many relationship
- `balance_changes` → `block`: Many-to-one relationship

## GraphQL Queries

### Query Naming Convention

Tables are exposed with their schema name as a prefix:
- `{chain_id}_blocks`
- `{chain_id}_balance_changes`
- `{chain_id}_account_stats`
- etc.

### Example Queries

#### Get Latest Blocks

```graphql
query GetLatestBlocks($limit: Int = 10) {
  # For Resonance testnet (quantum-safe PoW chain)
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    limit: $limit
    order_by: {number: desc}
  ) {
    number
    hash
    timestamp
    is_canonical
    balance_changes_aggregate {
      aggregate {
        count
      }
    }
  }
}
```

#### Get Account Balance History

```graphql
query GetAccountHistory($account: bytea!, $limit: Int = 100) {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_balance_changes(
    where: {account: {_eq: $account}}
    order_by: {block_number: desc}
    limit: $limit
  ) {
    block_number
    delta
    reason
    event_pallet
    event_variant
    block {
      timestamp
      hash
    }
  }
}
```

#### Get Top Accounts by Balance

```graphql
query GetTopAccounts($limit: Int = 20) {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_account_stats(
    order_by: {balance: desc}
    limit: $limit
  ) {
    account
    balance
    total_changes
    first_seen_block
    last_activity_block
    updated_at
  }
}
```

#### Get Chain Indexing Status

```graphql
query GetIndexStatus {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_index_progress {
    chain_id
    latest_block
    latest_block_ts
    blocks_indexed
    balance_changes_recorded
    started_at
    updated_at
  }
}
```

#### Cross-Chain Query Example

Query the same data from multiple chains:

```graphql
query CompareChains {
  # Resonance data (quantum-safe PoW chain)
  resonance: FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_index_progress {
    chain_id
    latest_block
    blocks_indexed
  }
  
  # Heisenberg data (quantum-safe chain)
  heisenberg: 7wwWHpnmxRf1Cnvri1cQ6LUiwvDw1czPjL7ytnoxNNNg_index_progress {
    chain_id
    latest_block
    blocks_indexed
  }
}
```

### Subscriptions

Hasura supports GraphQL subscriptions for real-time data:

```graphql
subscription WatchLatestBlocks {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    limit: 1
    order_by: {number: desc}
  ) {
    number
    hash
    timestamp
  }
}
```

## Working with Binary Data

Account addresses and hashes are stored as `bytea` in PostgreSQL. When querying:

### Encoding/Decoding

- **From Hex**: Use `\x` prefix: `\x1234abcd...`
- **To Base58**: Use client-side libraries to decode bytea and encode to base58
- **From Base58**: Decode to bytes client-side, then query with `\x` prefix

### Example JavaScript Client

```javascript
import { ApolloClient, InMemoryCache, gql } from '@apollo/client';
import bs58 from 'bs58';

const client = new ApolloClient({
  uri: 'http://localhost:8080/v1/graphql',
  headers: {
    'X-Hasura-Admin-Secret': process.env.HASURA_ADMIN_SECRET
  },
  cache: new InMemoryCache()
});

// Convert base58 address to hex for query
function addressToHex(base58Address) {
  const bytes = bs58.decode(base58Address);
  return '\\x' + Buffer.from(bytes).toString('hex');
}

// Query account balance
async function getAccountBalance(address) {
  const hexAddress = addressToHex(address);
  
  const { data } = await client.query({
    query: gql`
      query GetAccount($account: bytea!) {
        FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_account_stats(
          where: {account: {_eq: $account}}
        ) {
          balance
          total_changes
        }
      }
    `,
    variables: { account: hexAddress }
  });
  
  return data;
}
```

## Best Practices

### 1. Use Pagination

Always paginate large result sets:

```graphql
query PaginatedBlocks($offset: Int!, $limit: Int!) {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    offset: $offset
    limit: $limit
    order_by: {number: desc}
  ) {
    number
    hash
  }
}
```

### 2. Use Aggregations

For statistics, use Hasura's aggregation queries:

```graphql
query ChainStats {
  blocks: FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks_aggregate {
    aggregate {
      count
      max {
        number
      }
      min {
        number
      }
    }
  }
  
  changes: FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_balance_changes_aggregate {
    aggregate {
      count
      sum {
        delta
      }
    }
  }
}
```

### 3. Filter by Time Range

Use timestamp fields for time-based queries:

```graphql
query RecentActivity($since: timestamptz!) {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    where: {timestamp: {_gte: $since}}
    order_by: {number: desc}
  ) {
    number
    timestamp
    balance_changes_aggregate {
      aggregate {
        count
      }
    }
  }
}
```

### 4. Use Variables

Always use variables for dynamic values to enable query caching:

```graphql
# Good - uses variables
query GetBlock($number: bigint!) {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    where: {number: {_eq: $number}}
  ) {
    hash
    timestamp
  }
}

# Bad - hardcoded values
query {
  FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i_blocks(
    where: {number: {_eq: 12345}}
  ) {
    hash
    timestamp
  }
}
```

## Troubleshooting

### Tables Not Appearing in GraphQL

1. Ensure Chronicle has indexed the chain:
   ```bash
   psql -d chronicle -c "\dn"
   ```

2. Track tables in Hasura:
   ```bash
   ./scripts/hasura-track-tables.sh
   ```

3. Reload Hasura metadata:
   ```bash
   curl -X POST http://localhost:8080/v1/metadata \
     -H "X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}" \
     -H "Content-Type: application/json" \
     -d '{"type": "reload_metadata"}'
   ```

### Query Performance Issues

1. Check if indexes exist:
   ```sql
   SELECT indexname FROM pg_indexes 
   WHERE schemaname = 'FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i';
   ```

2. Use EXPLAIN in Hasura Console to analyze query plans

3. Consider adding custom indexes for common query patterns

### Connection Issues

1. Verify Hasura is running:
   ```bash
   sudo ./scripts/deploy-hasura-podman.sh status
   ```

2. Check Hasura logs:
   ```bash
   sudo ./scripts/deploy-hasura-podman.sh logs
   ```

3. Test database connectivity:
   ```bash
   pg_isready -h localhost -p 5432
   ```

## Security Considerations

### Production Deployment

1. **Change default passwords**:
   - Set `HASURA_ADMIN_SECRET` to a strong secret
   - Change database passwords

2. **Enable authentication**:
   - Configure JWT authentication
   - Set up role-based access control (RBAC)

3. **Restrict network access**:
   - Use firewall rules
   - Deploy behind a reverse proxy
   - Enable HTTPS

4. **Rate limiting**:
   - Configure query depth limits
   - Set up rate limiting at proxy level

### Example Hasura Permissions

Create a read-only role for public access:

```json
{
  "type": "create_select_permission",
  "args": {
    "table": {
      "schema": "FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i",
      "name": "blocks"
    },
    "role": "public",
    "permission": {
      "columns": ["number", "hash", "timestamp"],
      "filter": {},
      "limit": 100
    }
  }
}
```

## Additional Resources

- [Hasura Documentation](https://hasura.io/docs/latest/index/)
- [GraphQL Best Practices](https://graphql.org/learn/best-practices/)
- [Chronicle GitHub Repository](https://github.com/your-org/chronicle)
- [Quantum-Safe Blockchain Resources](https://csrc.nist.gov/projects/post-quantum-cryptography)
- [Dilithium Signature Scheme](https://pq-crystals.org/dilithium/)