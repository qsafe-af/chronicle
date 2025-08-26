# Chronicle Deployment & Testing Scripts

This directory contains scripts for deploying and testing Chronicle locally.

## Quick Start

```bash
# 1. Deploy TimescaleDB locally (run as root)
sudo ./deploy-timescaledb.sh

# 2. Run Chronicle with test configuration
./test-local.sh
```

## Scripts Overview

### `deploy-timescaledb.sh`

Deploys TimescaleDB as a Podman quadlet with a dedicated system user.

**Features:**
- Creates `qsafe` system user with home directory `/var/lib/qsafe`
- Deploys TimescaleDB as a user systemd service (quadlet)
- Configures TimescaleDB for optimal performance
- Sets up persistent storage
- Enables automatic startup on boot

**Usage:**
```bash
# Deploy and start TimescaleDB
sudo ./deploy-timescaledb.sh

# Other commands
sudo ./deploy-timescaledb.sh status    # Check service status
sudo ./deploy-timescaledb.sh stop      # Stop the service
sudo ./deploy-timescaledb.sh start     # Start the service
sudo ./deploy-timescaledb.sh restart   # Restart the service
sudo ./deploy-timescaledb.sh logs      # Follow service logs
sudo ./deploy-timescaledb.sh uninstall # Remove service (preserves data)
```

**Default Configuration:**
- User: `qsafe` (UID: 9001)
- Home: `/var/lib/qsafe`
- Data: `/var/lib/qsafe/timescaledb-data`
- Port: `5432`
- Database: `res_index`
- Username: `qsafe`
- Password: `changeme` (set via `POSTGRES_PASSWORD` env var)

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
sudo ./deploy-timescaledb.sh

# Verify deployment
sudo -u qsafe systemctl --user status qsafe-timescaledb
```

### 2. Configure Test Environment

Edit `test-local.env` to set your blockchain endpoint:

```bash
# For local development node
export WS_URL="ws://localhost:9944"

# For public testnets
export WS_URL="wss://westend-rpc.polkadot.io"  # Westend
export WS_URL="wss://rococo-rpc.polkadot.io"   # Rococo
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
sudo -u qsafe journalctl --user -u qsafe-timescaledb -f

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
podman exec -it qsafe-timescaledb psql -U qsafe -d res_index

# Backup database
podman exec qsafe-timescaledb pg_dump -U qsafe res_index > backup.sql

# Restore database
cat backup.sql | podman exec -i qsafe-timescaledb psql -U qsafe -d res_index
```

## Service Management

### TimescaleDB Service

The TimescaleDB runs as a user systemd service under the `qsafe` user:

```bash
# Service control (as root or with sudo)
sudo -u qsafe systemctl --user status qsafe-timescaledb
sudo -u qsafe systemctl --user stop qsafe-timescaledb
sudo -u qsafe systemctl --user start qsafe-timescaledb
sudo -u qsafe systemctl --user restart qsafe-timescaledb

# View logs
sudo -u qsafe journalctl --user -u qsafe-timescaledb -f

# Check container
sudo -u qsafe podman ps
sudo -u qsafe podman logs qsafe-timescaledb
```

### Quadlet File Location

The quadlet configuration is stored at:
```
/var/lib/qsafe/.config/containers/systemd/qsafe-timescaledb.container
```

## Troubleshooting

### TimescaleDB Won't Start

```bash
# Check service status
sudo -u qsafe systemctl --user status qsafe-timescaledb

# Check logs
sudo -u qsafe journalctl --user -u qsafe-timescaledb -xe

# Check if port is in use
ss -tlnp | grep 5432

# Check container status
sudo -u qsafe podman ps -a
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
sudo ./deploy-timescaledb.sh stop
```

### Remove TimescaleDB

```bash
# Remove service but keep data
sudo ./deploy-timescaledb.sh uninstall

# Completely remove including data
sudo ./deploy-timescaledb.sh uninstall
sudo rm -rf /var/lib/qsafe/timescaledb-data
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

1. **Default Password**: Change `changeme` in production!
2. **Network**: By default, listens on all interfaces. Restrict in production.
3. **User Isolation**: Runs as dedicated `qsafe` user for security.
4. **Capabilities**: Drops unnecessary capabilities, only keeps required ones.

## Performance Tuning

The deployment configures TimescaleDB with reasonable defaults:
- Max connections: 100
- Shared buffers: 256MB
- Effective cache size: 1GB
- Work memory: 4MB

For production, adjust based on your hardware:
- Edit the quadlet file at `/var/lib/qsafe/.config/containers/systemd/qsafe-timescaledb.container`
- Restart the service: `sudo -u qsafe systemctl --user restart qsafe-timescaledb`

## Support

For issues:
1. Check the troubleshooting section
2. Review logs with `./deploy-timescaledb.sh logs`
3. Ensure all dependencies are installed
4. Verify network connectivity to blockchain node