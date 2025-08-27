# Chronicle Deployment & Testing Scripts

This directory contains scripts for deploying and testing Chronicle locally.

## Quick Start

```bash
# 1. Deploy TimescaleDB locally (run as root)
sudo ./deploy-timescaledb-podman.sh

# 2. (Optional) Deploy Hasura GraphQL Engine for API access
sudo ./deploy-hasura-podman.sh

# 3. Run Chronicle with test configuration
./test-local.sh
```

## Scripts Overview

### `deploy-timescaledb-podman.sh`

Deploys TimescaleDB as a Podman container with a dedicated system user.

**Features:**
- Creates `qsafe` system user with home directory `/var/lib/qsafe`
- Deploys TimescaleDB as a Podman container with optional systemd service
- Configures TimescaleDB for optimal performance
- Sets up persistent storage
- Enables automatic startup on boot

**Usage:**
```bash
# Deploy and start TimescaleDB
sudo ./deploy-timescaledb-podman.sh

# Other commands
sudo ./deploy-timescaledb-podman.sh status    # Check service status
sudo ./deploy-timescaledb-podman.sh stop      # Stop the container
sudo ./deploy-timescaledb-podman.sh start     # Start the container
sudo ./deploy-timescaledb-podman.sh restart   # Restart the container
sudo ./deploy-timescaledb-podman.sh logs      # Follow container logs
sudo ./deploy-timescaledb-podman.sh shell     # Open bash shell in container
sudo ./deploy-timescaledb-podman.sh psql      # Open psql client
sudo ./deploy-timescaledb-podman.sh remove    # Remove container (preserves data)
sudo ./deploy-timescaledb-podman.sh purge     # Remove container and all data
```

**Default Configuration:**
- User: `qsafe` (UID: 9001)
- Home: `/var/lib/qsafe`
- Data: `/var/lib/qsafe/timescaledb-data`
- Port: `5432`
- Database: `res_index`
- Username: `qsafe`
- Password: `changeme` (set via `POSTGRES_PASSWORD` env var)

### `deploy-hasura-podman.sh`

Deploys Hasura GraphQL Engine to provide a GraphQL API over the Chronicle database.

**Features:**
- Auto-connects to TimescaleDB database
- Provides GraphQL API for querying blockchain data
- Includes web console for API exploration
- Configures health checks and monitoring
- Optional systemd service for automatic startup

**Usage:**
```bash
# Deploy and start Hasura
sudo ./deploy-hasura-podman.sh

# Other commands
sudo ./deploy-hasura-podman.sh status    # Check service status
sudo ./deploy-hasura-podman.sh stop      # Stop the container
sudo ./deploy-hasura-podman.sh start     # Start the container
sudo ./deploy-hasura-podman.sh restart   # Restart the container
sudo ./deploy-hasura-podman.sh logs      # Follow container logs
sudo ./deploy-hasura-podman.sh shell     # Open shell in container
sudo ./deploy-hasura-podman.sh console   # Open Hasura Console in browser
sudo ./deploy-hasura-podman.sh graphql   # Execute GraphQL query interactively
sudo ./deploy-hasura-podman.sh remove    # Remove container (preserves metadata)
sudo ./deploy-hasura-podman.sh purge     # Remove container and all metadata
```

**Default Configuration:**
- Port: `8080`
- Console: `http://localhost:8080/console`
- GraphQL Endpoint: `http://localhost:8080/v1/graphql`
- Admin Secret: `changeme` (set via `HASURA_ADMIN_SECRET` env var)
- Database: Connects to TimescaleDB at `localhost:5432`

**GraphQL Query Example:**
```bash
# Query with curl
curl -X POST http://localhost:8080/v1/graphql \
  -H "X-Hasura-Admin-Secret: changeme" \
  -H "Content-Type: application/json" \
  -d '{"query": "{ blocks(limit: 10) { number hash timestamp } }"}'

# Interactive query
./deploy-hasura-podman.sh graphql
# Then type your query and press Ctrl+D
```

### `test-local.sh`

Automated test runner for local development.

**Features:**
- Checks if TimescaleDB is running
- Tests database connectivity
- Verifies blockchain node connection
- Builds Chronicle
- Runs Chronicle with test configuration

**Usage:**
```bash
# Build and run Chronicle
./test-local.sh

# Just check dependencies
./test-local.sh check

# Build only
./test-local.sh build

# Test database connection
./test-local.sh test-db

# Clean build artifacts
./test-local.sh clean
```

### `test-local.env`

Environment configuration for local testing. Source this file to configure Chronicle.

**Usage:**
```bash
# Load configuration and run manually
source test-local.env
cargo run --release
```

**Configuration Options:**
```bash
# Database
PG_DSN="postgres://qsafe:changeme@localhost:5432/res_index"

# Blockchain Node
WS_URL="ws://localhost:9944"

# TimescaleDB
ENABLE_TIMESCALE=true

# Logging
RUST_LOG=info  # or debug, trace

# Performance
DB_MAX_CONNECTIONS=10
BLOCK_BATCH_SIZE=100
EVENT_BATCH_SIZE=1000
```

## Testing Workflow

### 1. Initial Setup

```bash
# Deploy TimescaleDB (one-time setup)
sudo ./deploy-timescaledb-podman.sh

# Verify deployment
sudo ./deploy-timescaledb-podman.sh status

# (Optional) Deploy Hasura for GraphQL API
sudo ./deploy-hasura-podman.sh
```

### 2. Configure Test Environment

Edit `test-local.env` to set your blockchain endpoint:

```bash
# For local development node
export WS_URL="ws://localhost:9944"

# For quantum-safe testnets
export WS_URL="wss://a.t.res.fm"  # Resonance (quantum-safe PoW)
export WS_URL="wss://a.i.res.fm"  # Heisenberg (quantum-safe)
```

### 3. Run Tests

```bash
# Automated test (checks everything and runs)
./test-local.sh

# Manual test with custom settings
source test-local.env
export RUST_LOG=debug
cargo run --release
```

### 4. Monitor

```bash
# Watch Chronicle logs
./test-local.sh run

# Watch TimescaleDB logs
sudo ./deploy-timescaledb-podman.sh logs

# Watch Hasura logs (if deployed)
sudo ./deploy-hasura-podman.sh logs

# Check database content
psql -h localhost -U qsafe -d res_index

# Inside psql:
\dt *.*                    # List all tables in all schemas
\dn+                       # List all schemas
SELECT * FROM "CHAIN_ID".index_progress;  # Check indexing progress
```

## Database Access

### Via psql

```bash
# Connect to database
psql -h localhost -U qsafe -d res_index
# Password: changeme

# Useful queries
\l                         # List databases
\dn+                       # List schemas with details
\dt "CHAIN_ID".*          # List tables in chain schema
\d+ "CHAIN_ID".blocks     # Describe blocks table
```

### Via Podman

```bash
# Execute SQL in container
sudo ./deploy-timescaledb-podman.sh psql

# Backup database
podman exec qsafe-timescaledb pg_dump -U qsafe res_index > backup.sql

# Restore database
cat backup.sql | podman exec -i qsafe-timescaledb psql -U qsafe -d res_index
```

## Service Management

### TimescaleDB Service

```bash
# Service control
sudo ./deploy-timescaledb-podman.sh status
sudo ./deploy-timescaledb-podman.sh stop
sudo ./deploy-timescaledb-podman.sh start
sudo ./deploy-timescaledb-podman.sh restart

# View logs
sudo ./deploy-timescaledb-podman.sh logs

# Check container
podman ps -a --filter name=qsafe-timescaledb
```

### Hasura Service

```bash
# Service control
sudo ./deploy-hasura-podman.sh status
sudo ./deploy-hasura-podman.sh stop
sudo ./deploy-hasura-podman.sh start
sudo ./deploy-hasura-podman.sh restart

# View logs
sudo ./deploy-hasura-podman.sh logs

# Open console
sudo ./deploy-hasura-podman.sh console

# Check container
podman ps -a --filter name=qsafe-hasura
```

## Troubleshooting

### TimescaleDB Won't Start

```bash
# Check service status
sudo ./deploy-timescaledb-podman.sh status

# Check logs
sudo ./deploy-timescaledb-podman.sh logs

# Check if port is in use
ss -tlnp | grep 5432

# Check container status
podman ps -a --filter name=qsafe-timescaledb
```

### Hasura Won't Start

```bash
# Check service status
sudo ./deploy-hasura-podman.sh status

# Check logs
sudo ./deploy-hasura-podman.sh logs

# Verify TimescaleDB is running
sudo ./deploy-timescaledb-podman.sh status

# Test database connectivity
pg_isready -h localhost -p 5432
```

### Database Connection Failed

```bash
# Test connection
pg_isready -h localhost -p 5432

# Check firewall
sudo firewall-cmd --list-all

# Test with psql
PGPASSWORD=changeme psql -h localhost -U qsafe -d res_index -c "SELECT 1"
```

### Permission Issues

```bash
# Fix ownership of qsafe directories
sudo chown -R qsafe:qsafe /var/lib/qsafe

# Check lingering is enabled
loginctl show-user qsafe | grep Linger
sudo loginctl enable-linger qsafe
```

### Chronicle Can't Connect

```bash
# Verify environment variables
echo $PG_DSN
echo $WS_URL

# Test database from Chronicle's perspective
source test-local.env
./test-local.sh test-db

# Check if schema was created
psql -h localhost -U qsafe -d res_index -c "\dn"
```

## Clean Up

### Stop Services

```bash
# Stop Chronicle (Ctrl+C in terminal)

# Stop TimescaleDB
sudo ./deploy-timescaledb-podman.sh stop

# Stop Hasura (if deployed)
sudo ./deploy-hasura-podman.sh stop
```

### Remove TimescaleDB

```bash
# Remove container but keep data
sudo ./deploy-timescaledb-podman.sh remove

# Completely remove including data
sudo ./deploy-timescaledb-podman.sh purge
```

### Remove Hasura

```bash
# Remove container but keep metadata
sudo ./deploy-hasura-podman.sh remove

# Completely remove including metadata
sudo ./deploy-hasura-podman.sh purge
```

### Remove Test Data

```bash
# Connect to database
psql -h localhost -U qsafe -d res_index

# Drop specific chain schema
DROP SCHEMA "CHAIN_ID" CASCADE;

# Or reset entire database
DROP DATABASE res_index;
CREATE DATABASE res_index;
```

## Security Notes

1. **Default Passwords**: Change `changeme` passwords in production!
   - TimescaleDB: Set via `POSTGRES_PASSWORD` environment variable
   - Hasura: Set via `HASURA_ADMIN_SECRET` environment variable
2. **Network**: By default, listens on all interfaces. Restrict in production.
3. **User Isolation**: Runs as dedicated `qsafe` user for security.
4. **Capabilities**: Drops unnecessary capabilities, only keeps required ones.

## Performance Tuning

The deployment configures TimescaleDB with reasonable defaults:
- Max connections: 100
- Shared buffers: 256MB
- Effective cache size: 1GB
- Work memory: 4MB

For production, adjust based on your hardware by modifying environment variables in the deployment scripts.

## Support

For issues:
1. Check the troubleshooting section
2. Review logs with `./deploy-timescaledb-podman.sh logs` or `./deploy-hasura-podman.sh logs`
3. Ensure all dependencies are installed
4. Verify network connectivity to quantum-safe blockchain node