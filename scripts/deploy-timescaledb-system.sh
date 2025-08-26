#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
QSAFE_USER="qsafe"
QSAFE_HOME="/var/lib/qsafe"
QSAFE_UID=9001
QSAFE_GID=9001
QUADLET_NAME="qsafe-timescaledb"
CONTAINER_IMAGE="docker.io/timescale/timescaledb:latest-pg16"
POSTGRES_PASSWORD="${POSTGRES_PASSWORD:-changeme}"
DB_DATA_DIR="${QSAFE_HOME}/timescaledb-data"

# Functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root"
        exit 1
    fi
}

check_dependencies() {
    local missing_deps=()

    # Check for required commands
    for cmd in podman systemctl; do
        if ! command -v $cmd &> /dev/null; then
            missing_deps+=($cmd)
        fi
    done

    if [ ${#missing_deps[@]} -ne 0 ]; then
        log_error "Missing required dependencies: ${missing_deps[*]}"
        log_info "Install with: dnf install podman systemd"
        exit 1
    fi

    # Check for quadlet support
    if ! podman --version | grep -q "podman version 4\|podman version 5"; then
        log_error "Podman 4.0+ is required for quadlet support"
        exit 1
    fi

    # Check if podman has quadlet support
    if ! rpm -qa | grep -q podman-quadlet && ! command -v /usr/libexec/podman/quadlet &> /dev/null; then
        log_warn "Quadlet support might not be installed"
        log_info "Try: dnf install podman-quadlet or update podman to 4.4+"
    fi
}

create_qsafe_user() {
    if id "${QSAFE_USER}" &>/dev/null; then
        log_info "User ${QSAFE_USER} already exists"
    else
        log_info "Creating system user ${QSAFE_USER}"
        useradd \
            --system \
            --uid ${QSAFE_UID} \
            --home-dir ${QSAFE_HOME} \
            --create-home \
            --shell /sbin/nologin \
            --comment "QSafe service user" \
            ${QSAFE_USER}
    fi

    # Ensure home directory exists with correct permissions
    if [ ! -d "${QSAFE_HOME}" ]; then
        mkdir -p "${QSAFE_HOME}"
    fi
    chown ${QSAFE_USER}:${QSAFE_USER} "${QSAFE_HOME}"
    chmod 755 "${QSAFE_HOME}"
}

setup_directories() {
    log_info "Setting up directories"

    # Create data directory for TimescaleDB
    if [ ! -d "${DB_DATA_DIR}" ]; then
        log_info "Creating database data directory: ${DB_DATA_DIR}"
        mkdir -p "${DB_DATA_DIR}"
        chown ${QSAFE_USER}:${QSAFE_USER} "${DB_DATA_DIR}"
        chmod 700 "${DB_DATA_DIR}"
    else
        log_info "Database data directory already exists: ${DB_DATA_DIR}"
    fi

    # Create system containers directory for quadlet files
    if [ ! -d "/etc/containers/systemd" ]; then
        log_info "Creating system containers directory"
        mkdir -p "/etc/containers/systemd"
    fi
}

create_quadlet_file() {
    local quadlet_file="/etc/containers/systemd/${QUADLET_NAME}.container"

    log_info "Creating system quadlet file: ${quadlet_file}"

    cat > "${quadlet_file}" <<EOF
[Unit]
Description=QSafe TimescaleDB Database
After=network-online.target
Wants=network-online.target

[Container]
Image=${CONTAINER_IMAGE}
ContainerName=${QUADLET_NAME}
AutoUpdate=registry

# Run as qsafe user
User=${QSAFE_UID}
Group=${QSAFE_GID}

# Database configuration
Environment=POSTGRES_USER=qsafe
Environment=POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
Environment=POSTGRES_DB=res_index
Environment=POSTGRES_INITDB_ARGS=--encoding=UTF8 --lc-collate=C --lc-ctype=C

# TimescaleDB configuration
Environment=TIMESCALEDB_TELEMETRY=off
Environment=TS_TUNE_MAX_CONNS=100
Environment=TS_TUNE_MAX_BG_WORKERS=8

# PostgreSQL configuration via environment
Environment=POSTGRES_MAX_CONNECTIONS=100
Environment=POSTGRES_SHARED_BUFFERS=256MB
Environment=POSTGRES_EFFECTIVE_CACHE_SIZE=1GB
Environment=POSTGRES_MAINTENANCE_WORK_MEM=128MB
Environment=POSTGRES_CHECKPOINT_COMPLETION_TARGET=0.9
Environment=POSTGRES_WAL_BUFFERS=16MB
Environment=POSTGRES_DEFAULT_STATISTICS_TARGET=100
Environment=POSTGRES_RANDOM_PAGE_COST=1.1
Environment=POSTGRES_EFFECTIVE_IO_CONCURRENCY=200
Environment=POSTGRES_WORK_MEM=4MB
Environment=POSTGRES_MIN_WAL_SIZE=1GB
Environment=POSTGRES_MAX_WAL_SIZE=4GB

# Volume mount for persistent data
Volume=${DB_DATA_DIR}:/var/lib/postgresql/data:Z

# Network configuration
PublishPort=5432:5432
Network=bridge

# Health check
HealthCmd=pg_isready -U qsafe -d res_index
HealthInterval=30s
HealthTimeout=5s
HealthRetries=3
HealthStartPeriod=60s

# Resource limits
PodmanArgs=--memory=2g
PodmanArgs=--memory-reservation=1g
PodmanArgs=--cpus=2
PodmanArgs=--userns=keep-id

# Security
PodmanArgs=--cap-drop=ALL
PodmanArgs=--cap-add=CHOWN
PodmanArgs=--cap-add=DAC_OVERRIDE
PodmanArgs=--cap-add=FOWNER
PodmanArgs=--cap-add=SETUID
PodmanArgs=--cap-add=SETGID
PodmanArgs=--security-opt=no-new-privileges

# Restart policy
Restart=always
RestartSec=30

[Service]
TimeoutStartSec=300

[Install]
WantedBy=multi-user.target
EOF

    chmod 644 "${quadlet_file}"
}

create_init_script() {
    local init_script="${QSAFE_HOME}/init-timescaledb.sql"

    log_info "Creating initialization script: ${init_script}"

    cat > "${init_script}" <<'EOF'
-- Enable TimescaleDB extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Create chronicle user if it doesn't exist
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_user WHERE usename = 'chronicle') THEN
        CREATE USER chronicle WITH PASSWORD 'chronicle_password';
    END IF;
END
$$;

-- Grant necessary permissions
GRANT CREATE ON DATABASE res_index TO chronicle;
GRANT ALL ON SCHEMA public TO chronicle;

-- Create a schema for chronicle metadata
CREATE SCHEMA IF NOT EXISTS chronicle_meta;
GRANT ALL ON SCHEMA chronicle_meta TO chronicle;

-- Performance tuning for TimescaleDB
ALTER SYSTEM SET max_worker_processes = 32;
ALTER SYSTEM SET max_parallel_workers_per_gather = 4;
ALTER SYSTEM SET max_parallel_workers = 8;
ALTER SYSTEM SET timescaledb.max_background_workers = 8;

-- Logging configuration for debugging
ALTER SYSTEM SET log_statement = 'all';
ALTER SYSTEM SET log_duration = on;
ALTER SYSTEM SET log_line_prefix = '%t [%p]: [%l-1] user=%u,db=%d,app=%a,client=%h ';

-- Apply configuration changes
SELECT pg_reload_conf();

-- Show current configuration
\echo 'TimescaleDB version:'
SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';

\echo 'Current configuration:'
SELECT name, setting, unit FROM pg_settings
WHERE name IN ('max_connections', 'shared_buffers', 'effective_cache_size', 'work_mem')
ORDER BY name;
EOF

    chown ${QSAFE_USER}:${QSAFE_USER} "${init_script}"
    chmod 644 "${init_script}"
}

reload_systemd() {
    log_info "Reloading systemd configuration"

    # First, ensure the quadlet generator is available
    if ! command -v /usr/lib/systemd/system-generators/podman-system-generator &> /dev/null; then
        if ! command -v /usr/libexec/podman/quadlet &> /dev/null; then
            log_warn "Podman quadlet generator not found - trying manual generation"
        fi
    fi

    # Reload systemd
    systemctl daemon-reload

    # Force regeneration of units
    systemctl daemon-reexec

    # Give systemd time to process the quadlet
    sleep 2

    # Check if service was generated
    if ! systemctl list-unit-files | grep -q "${QUADLET_NAME}.service"; then
        log_warn "Service not generated yet, trying to trigger generation"

        # Try to manually run the generator if available
        if [ -x /usr/lib/systemd/system-generators/podman-system-generator ]; then
            /usr/lib/systemd/system-generators/podman-system-generator /run/systemd/generator
        elif [ -x /usr/libexec/podman/quadlet ]; then
            /usr/libexec/podman/quadlet -dryrun
        fi

        systemctl daemon-reload
        sleep 2
    fi

    # Verify the service exists
    if systemctl list-unit-files | grep -q "${QUADLET_NAME}.service"; then
        log_info "Service ${QUADLET_NAME}.service successfully generated"
    else
        log_error "Failed to generate service from quadlet"
        log_info "Checking quadlet file syntax..."
        if [ -x /usr/libexec/podman/quadlet ]; then
            /usr/libexec/podman/quadlet -dryrun -unit-dir /etc/containers/systemd 2>&1 | head -20
        fi
    fi
}

manage_service() {
    local action=$1
    local service_name="${QUADLET_NAME}.service"

    case $action in
        start)
            log_info "Starting ${service_name}"
            if systemctl list-unit-files | grep -q "${service_name}"; then
                systemctl start ${service_name}
            else
                log_error "Service ${service_name} not found in systemd"
                log_info "Checking for quadlet file..."
                if [ -f "/etc/containers/systemd/${QUADLET_NAME}.container" ]; then
                    log_info "Quadlet file exists, attempting reload"
                    reload_systemd
                    systemctl start ${service_name}
                else
                    log_error "Quadlet file not found"
                fi
            fi
            ;;
        stop)
            log_info "Stopping ${service_name}"
            systemctl stop ${service_name} || true
            ;;
        enable)
            log_info "Enabling ${service_name}"
            systemctl enable ${service_name}
            ;;
        status)
            systemctl status ${service_name}
            ;;
        restart)
            log_info "Restarting ${service_name}"
            systemctl restart ${service_name}
            ;;
    esac
}

wait_for_database() {
    log_info "Waiting for database to be ready..."

    local max_attempts=30
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        # Check if container is running first
        if ! podman ps | grep -q ${QUADLET_NAME}; then
            log_warn "Container not running, checking..."
            podman ps -a | grep ${QUADLET_NAME} || log_error "Container not found"
        fi

        if podman exec ${QUADLET_NAME} pg_isready -U qsafe -d res_index &>/dev/null; then
            log_info "Database is ready!"
            return 0
        fi

        log_info "Waiting for database... (attempt $attempt/$max_attempts)"
        sleep 2
        ((attempt++))
    done

    log_error "Database failed to start after $max_attempts attempts"
    log_info "Container status:"
    podman ps -a | grep ${QUADLET_NAME} || echo "Container not found"
    return 1
}

run_init_script() {
    local init_script="${QSAFE_HOME}/init-timescaledb.sql"

    if [ -f "${init_script}" ]; then
        log_info "Running initialization script"
        podman exec -i ${QUADLET_NAME} psql -U qsafe -d res_index < "${init_script}" || {
            log_warn "Some initialization commands may have failed (this is normal if re-running)"
        }
    fi
}

show_connection_info() {
    log_info "================================================================"
    log_info "QSafe TimescaleDB has been deployed successfully!"
    log_info "================================================================"
    log_info ""
    log_info "Connection Information:"
    log_info "  Host: localhost"
    log_info "  Port: 5432"
    log_info "  Database: res_index"
    log_info "  Username: qsafe"
    log_info "  Password: ${POSTGRES_PASSWORD}"
    log_info ""
    log_info "Chronicle Connection String:"
    log_info "  PG_DSN=\"postgres://qsafe:${POSTGRES_PASSWORD}@localhost:5432/res_index\""
    log_info ""
    log_info "Service Management:"
    log_info "  Status:  systemctl status ${QUADLET_NAME}"
    log_info "  Stop:    systemctl stop ${QUADLET_NAME}"
    log_info "  Start:   systemctl start ${QUADLET_NAME}"
    log_info "  Restart: systemctl restart ${QUADLET_NAME}"
    log_info "  Logs:    journalctl -u ${QUADLET_NAME} -f"
    log_info ""
    log_info "Database Access:"
    log_info "  psql -h localhost -U qsafe -d res_index"
    log_info "  podman exec -it ${QUADLET_NAME} psql -U qsafe -d res_index"
    log_info ""
}

# Main execution
main() {
    log_info "Starting QSafe TimescaleDB deployment (system-wide)"

    # Checks
    check_root
    check_dependencies

    # Setup
    create_qsafe_user
    setup_directories
    create_quadlet_file
    create_init_script

    # Service management
    reload_systemd

    # Check if service was generated
    if ! systemctl list-unit-files | grep -q "${QUADLET_NAME}.service"; then
        log_error "Service generation failed"
        log_info "Attempting fallback to direct podman run..."

        # Try running container directly as fallback
        log_info "Starting container directly with podman"
        podman run -d \
            --name ${QUADLET_NAME} \
            --user ${QSAFE_UID}:${QSAFE_GID} \
            -e POSTGRES_USER=qsafe \
            -e POSTGRES_PASSWORD=${POSTGRES_PASSWORD} \
            -e POSTGRES_DB=res_index \
            -e POSTGRES_INITDB_ARGS="--encoding=UTF8 --lc-collate=C --lc-ctype=C" \
            -v ${DB_DATA_DIR}:/var/lib/postgresql/data:Z \
            -p 5432:5432 \
            ${CONTAINER_IMAGE}

        if [ $? -eq 0 ]; then
            log_info "Container started directly with podman"
            log_warn "Note: This container will not auto-start on boot"
            log_info "To manage: podman stop/start ${QUADLET_NAME}"
        else
            log_error "Failed to start container"
            exit 1
        fi
    else
        manage_service stop
        manage_service start
        manage_service enable
    fi

    # Wait and initialize
    if wait_for_database; then
        run_init_script
        show_connection_info
    else
        log_error "Failed to start database service"
        log_info "Check logs with: journalctl -u ${QUADLET_NAME} -xe"
        exit 1
    fi
}

# Handle script arguments
case "${1:-deploy}" in
    deploy)
        main
        ;;
    status)
        manage_service status
        ;;
    stop)
        manage_service stop
        ;;
    start)
        manage_service start
        wait_for_database
        ;;
    restart)
        manage_service restart
        wait_for_database
        ;;
    logs)
        journalctl -u ${QUADLET_NAME}.service -f
        ;;
    uninstall)
        log_warn "This will remove the QSafe TimescaleDB service and data!"
        read -p "Are you sure? (yes/no): " confirm
        if [ "$confirm" = "yes" ]; then
            manage_service stop
            systemctl disable ${QUADLET_NAME}.service || true
            rm -f "/etc/containers/systemd/${QUADLET_NAME}.container"
            log_info "Service removed. Data directory preserved at: ${DB_DATA_DIR}"
            log_info "To completely remove data: rm -rf ${DB_DATA_DIR}"
        fi
        ;;
    *)
        echo "Usage: $0 {deploy|status|stop|start|restart|logs|uninstall}"
        echo ""
        echo "  deploy    - Deploy and start the TimescaleDB service (default)"
        echo "  status    - Show service status"
        echo "  stop      - Stop the service"
        echo "  start     - Start the service"
        echo "  restart   - Restart the service"
        echo "  logs      - Follow service logs"
        echo "  uninstall - Remove the service (preserves data)"
        exit 1
        ;;
esac
