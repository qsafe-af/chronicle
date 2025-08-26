# Runtime Version Discovery Tools

This directory contains tools for discovering and downloading all runtime versions from Substrate-based blockchains.

## Overview

Substrate chains can upgrade their runtime without hard forks. Each runtime has a version number (`spec_version`) that increments with each upgrade. These tools help you:

1. Discover all historical runtime versions on a chain
2. Download metadata for each version
3. Track when each version was active

## Methods for Discovering Runtime Versions

### Method 1: Event-Based Discovery (Most Accurate)

Runtime upgrades emit specific events that can be tracked:
- `System.CodeUpdated` - Emitted when runtime code is updated
- `ParachainSystem.ValidationFunctionApplied` - For parachains

```bash
# Using the Rust scanner (recommended)
cargo run --bin scan-runtimes -- --url wss://your-node.example.com --output ./metadata
```

### Method 2: Binary Search (Efficient)

The `get-all-runtime-versions.sh` script uses binary search to find version changes:

```bash
./get-all-runtime-versions.sh
```

This script:
1. Gets genesis and current runtime versions
2. Binary searches between blocks to find version changes
3. Downloads metadata for each discovered version

### Method 3: RPC Queries (Simple)

You can query runtime versions at specific blocks using RPC:

```bash
# Get runtime version at latest block
curl -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getRuntimeVersion","params":[],"id":1}' \
  http://localhost:9944

# Get runtime version at specific block
curl -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getRuntimeVersion","params":["0xBLOCK_HASH"],"id":1}' \
  http://localhost:9944
```

## Tools Included

### 1. `get-runtimes.sh`
Downloads the current runtime metadata for all configured chains.

**Usage:**
```bash
./get-runtimes.sh
```

**Output:**
```
metadata/
└── CHAIN_BASE58/
    ├── metadata.hex
    ├── metadata.json
    └── metadata.scale
```

### 2. `get-all-runtime-versions.sh`
Discovers and downloads ALL historical runtime versions.

**Usage:**
```bash
./get-all-runtime-versions.sh
```

**Output:**
```
metadata/
└── CHAIN_BASE58/
    ├── versions.json           # Summary of all versions
    ├── runtime-v1/
    │   ├── metadata.hex
    │   ├── metadata.json
    │   ├── metadata.scale
    │   └── version.json       # Version details
    ├── runtime-v2/
    │   └── ...
    └── runtime-vN/
        └── ...
```

### 3. `scan-runtimes` Binary
Rust tool for efficient runtime version discovery.

**Features:**
- Binary search for version changes
- Event-based upgrade detection
- Metadata download for each version
- Progress tracking and resumability

**Usage:**
```bash
# Build the tool
cargo build --bin scan-runtimes --release

# Scan a chain
./target/release/scan-runtimes \
  --url wss://your-node.example.com \
  --output ./runtime-metadata \
  --verbose

# Options:
#   --url URL           WebSocket endpoint (default: ws://localhost:9944)
#   --output DIR        Output directory (default: ./runtime-metadata)
#   --scan-only         Only discover versions without downloading metadata
#   --max-blocks N      Maximum blocks to scan (0 for all)
#   --verbose           Enable debug logging
```

## Configuration

Both shell scripts read from `../orchestration/config/chains.yml`:

```yaml
chains:
  - id: my-chain
    endpoint: wss://my-chain.example.com
    genesis:
      hash: "0x..."
      base58: "..."
```

## How Runtime Upgrades Work

1. **Proposal**: Governance proposes a runtime upgrade
2. **Enactment**: After approval, the new code is set
3. **Event**: `System.CodeUpdated` event is emitted
4. **Version Change**: `spec_version` increments
5. **Metadata**: New metadata reflects changes

## Finding Upgrade Blocks

To find the exact block where an upgrade occurred:

```javascript
// Pseudo-code for binary search
function findUpgradeBlock(start, end, oldVersion, newVersion) {
  while (start < end) {
    mid = (start + end) / 2
    version = getVersionAt(mid)
    
    if (version == oldVersion) {
      start = mid + 1
    } else {
      end = mid
    }
  }
  return start
}
```

## Working with Downloaded Metadata

### Using with subxt

```rust
// Generate types from metadata
#[subxt::subxt(runtime_metadata_path = "metadata/CHAIN/runtime-v10/metadata.scale")]
pub mod runtime_v10 {}

// Use versioned types
let transfer = runtime_v10::tx().balances().transfer(...);
```

### Decoding with scale-info

```rust
use parity_scale_codec::Decode;
use frame_metadata::RuntimeMetadata;

let metadata_bytes = std::fs::read("metadata.scale")?;
let metadata = RuntimeMetadata::decode(&mut &metadata_bytes[..])?;

match metadata {
    RuntimeMetadata::V15(v15) => {
        println!("Pallets: {}", v15.pallets.len());
    }
    _ => {}
}
```

## Handling Multiple Versions

When indexing a chain with multiple runtime versions:

1. **Detect Version at Block**: Check runtime version when processing each block
2. **Load Correct Metadata**: Use metadata matching the block's runtime version
3. **Decode with Version**: Use version-specific types for decoding

```rust
// Example: Version-aware event decoding
fn decode_events(block_number: u32, runtime_version: u32, events: Vec<u8>) {
    match runtime_version {
        1..=10 => decode_with_v10_types(events),
        11..=20 => decode_with_v20_types(events),
        _ => decode_with_latest_types(events),
    }
}
```

## Common Issues

### Issue: Missing Historical Blocks
**Solution**: Some archive nodes prune old state. Use a full archive node.

### Issue: Slow Discovery
**Solution**: Use binary search instead of linear scanning.

### Issue: Large Metadata Files
**Solution**: Store only `.scale` format and generate types on demand.

### Issue: Version Detection Fails
**Solution**: Fall back to sampling blocks at intervals.

## Performance Tips

1. **Cache Metadata**: Download once and reuse
2. **Binary Search**: More efficient than linear scanning
3. **Event Filtering**: Only process System.CodeUpdated events
4. **Parallel Downloads**: Download metadata for multiple versions concurrently
5. **Compression**: Compress stored metadata files

## Example Output

```json
{
  "chain_id": "my-chain",
  "genesis_hash": "0x...",
  "versions": [
    {
      "spec_version": 1,
      "first_seen_block": 0,
      "last_seen_block": 99999,
      "block_hash": "0x..."
    },
    {
      "spec_version": 2,
      "first_seen_block": 100000,
      "last_seen_block": 199999,
      "block_hash": "0x..."
    }
  ]
}
```

## Dependencies

- `subxt-cli`: For downloading metadata
- `jq`: For JSON processing
- `yq`: For YAML processing
- `xxd`: For hex conversion (optional)
- `curl`: For RPC calls

Install dependencies:
```bash
# Rust tools
cargo install subxt-cli

# System tools (Ubuntu/Debian)
apt-get install jq curl xxd

# Python tools
pip install yq
```

## Contributing

When adding support for new chains:
1. Update `chains.yml` with endpoint and genesis info
2. Test with current metadata first: `./get-runtimes.sh`
3. Run full discovery: `./get-all-runtime-versions.sh`
4. Verify metadata files are valid

## License

Apache-2.0