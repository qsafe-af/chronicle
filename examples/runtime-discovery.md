# Integrated Runtime Discovery - Zero Configuration Indexing

With the latest updates, Chronicle now automatically discovers and stores runtime versions as part of the main indexing process. No separate scanning step required!

## How It Works

### Automatic Discovery During Indexing

When you start `chronicled`, it automatically:

1. **On First Run**: Scans from genesis to current block to discover all runtime versions
2. **During Operation**: Detects runtime upgrades as they happen
3. **Stores Metadata**: Saves runtime metadata in the database for future use

```bash
# Just run the indexer - runtime discovery happens automatically!
export WS_URL=wss://your-node.example.com
export PG_DSN=postgresql:///chronicle
./chronicled
```

Output:
```
[INFO] Connected to chain; computed base58 chain ID from genesis hash
[INFO] Database schema initialized for chain ABC123...
[INFO] Scanning for runtime versions...
[INFO] No existing runtime versions found, scanning from genesis to block 1500000
[INFO] Stored runtime v1 at block 0
[INFO] Stored runtime v2 at block 500000
[INFO] Stored runtime v3 at block 1000000
[INFO] Discovered 3 runtime versions
[INFO] Resuming indexing from block 1500001
[INFO] Processing finalized block #1500001
```

## Database Schema

Runtime versions are stored in the `metadata` table within each chain's schema:

```sql
-- View all discovered runtime versions
SELECT 
    spec_version,
    first_seen_block,
    last_seen_block,
    pg_size_pretty(length(metadata_bytes)) as metadata_size
FROM "YOUR_CHAIN_ID".metadata
ORDER BY spec_version;
```

Example output:
```
 spec_version | first_seen_block | last_seen_block | metadata_size
--------------+------------------+-----------------+---------------
            1 |                0 |          499999 | 256 KB
            2 |           500000 |          999999 | 258 KB
            3 |          1000000 |            NULL | 260 KB
```

## Runtime Upgrade Detection

When a runtime upgrade occurs while indexing:

```
[INFO] Runtime upgraded from v3 to v4 at block 1500123
[INFO] Storing new runtime metadata for v4
[INFO] Updated previous runtime v3 last_seen_block to 1500122
```

The indexer automatically:
- Detects the version change
- Downloads and stores the new metadata
- Updates the previous version's `last_seen_block`
- Continues indexing with the new version

## Using Runtime Metadata for Decoding

### Get Metadata for a Specific Block

```sql
-- Find which runtime version was active at block 750000
SELECT spec_version, metadata_bytes 
FROM "YOUR_CHAIN_ID".metadata
WHERE first_seen_block <= 750000
  AND (last_seen_block IS NULL OR last_seen_block >= 750000);
```

### In Your Application

```rust
use chron_db::{RuntimeMetadataRepository, ConnectionPool};

// Get metadata for decoding events at a specific block
let conn = pool.get().await?;
let metadata_repo = RuntimeMetadataRepository::new(&conn);

// Get the runtime metadata for block 750000
let metadata = metadata_repo.get_for_block(750000).await?;

if let Some(runtime_metadata) = metadata {
    println!("Block 750000 uses runtime v{}", runtime_metadata.spec_version);
    
    // Use metadata_bytes to decode events
    let metadata_bytes = runtime_metadata.metadata_bytes;
    // ... decode events using this metadata version
}
```

## Benefits of Integrated Discovery

### 1. **Zero Configuration**
No need to run separate scripts or tools. Just start the indexer.

### 2. **Automatic Updates**
Runtime upgrades are detected and stored automatically as they happen.

### 3. **Efficient Storage**
Metadata is stored once per version in the database, not per block.

### 4. **Version-Aware Decoding**
Always use the correct metadata version for decoding historical events.

### 5. **Resumable**
If indexing stops and restarts, runtime versions are preserved.

## Monitoring Runtime Versions

### Check Current Runtime Version

```sql
-- Get the currently active runtime version
SELECT 
    spec_version,
    first_seen_block,
    pg_size_pretty(length(metadata_bytes)) as size,
    metadata_hash_hex
FROM "YOUR_CHAIN_ID".metadata
WHERE last_seen_block IS NULL;
```

### Runtime Version History

```sql
-- Show runtime upgrade history
SELECT 
    spec_version,
    first_seen_block,
    last_seen_block,
    CASE 
        WHEN last_seen_block IS NULL THEN 'Active'
        ELSE 'Historical'
    END as status,
    last_seen_block - first_seen_block + 1 as blocks_active
FROM "YOUR_CHAIN_ID".metadata
ORDER BY spec_version;
```

### Find Upgrade Points

```sql
-- Find exact blocks where upgrades occurred
SELECT 
    m1.spec_version as from_version,
    m2.spec_version as to_version,
    m1.last_seen_block + 1 as upgrade_block
FROM "YOUR_CHAIN_ID".metadata m1
JOIN "YOUR_CHAIN_ID".metadata m2 
    ON m2.first_seen_block = m1.last_seen_block + 1
ORDER BY upgrade_block;
```

## Performance Considerations

### Initial Scan
- The first run scans from genesis to current
- This happens once per chain
- Subsequent runs use stored metadata

### Ongoing Operation
- Runtime version checks are lightweight
- Metadata is only fetched when versions change
- No impact on normal indexing performance

### Storage Requirements
- Each runtime version stores ~250-500KB
- Most chains have < 50 versions total
- Total overhead: ~25MB per chain

## Troubleshooting

### Missing Runtime Versions

If runtime versions are missing:

```sql
-- Check what versions are stored
SELECT spec_version, first_seen_block, last_seen_block 
FROM "YOUR_CHAIN_ID".metadata 
ORDER BY spec_version;

-- Force a rescan by clearing metadata
DELETE FROM "YOUR_CHAIN_ID".metadata;
-- Then restart chronicled
```

### Verify Metadata Integrity

```sql
-- Check metadata hash consistency
SELECT 
    spec_version,
    encode(metadata_hash, 'hex') as hash,
    pg_size_pretty(length(metadata_bytes)) as size
FROM "YOUR_CHAIN_ID".metadata;
```

## Advanced Usage

### Export Metadata for External Tools

```sql
-- Export metadata to file
\copy (SELECT metadata_bytes FROM "YOUR_CHAIN_ID".metadata WHERE spec_version = 10) 
TO '/tmp/runtime-v10.scale' WITH (FORMAT binary);
```

### Generate Types from Stored Metadata

```rust
// Use stored metadata with subxt
let metadata_bytes = get_metadata_from_db(version);
std::fs::write("runtime-v10.scale", metadata_bytes)?;

// Then in your code:
#[subxt::subxt(runtime_metadata_path = "runtime-v10.scale")]
pub mod runtime_v10 {}
```

## Summary

The integrated runtime discovery makes Chronicle truly zero-configuration:

1. **Start the indexer** - No preparation needed
2. **Automatic discovery** - Finds all runtime versions
3. **Live detection** - Catches upgrades as they happen
4. **Persistent storage** - Metadata saved in database
5. **Version-aware** - Always use correct metadata for decoding

This eliminates the need for:
- ❌ Separate metadata scanning scripts
- ❌ Manual runtime version tracking
- ❌ External metadata storage
- ❌ Complex version management

Just run `chronicled` and it handles everything! 🚀