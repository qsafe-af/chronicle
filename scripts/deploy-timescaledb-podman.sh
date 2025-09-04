#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
QSAFE_USER="chronicle"
QSAFE_HOME="/var/lib/chronicle"
QSAFE_UID=9001
QSAFE_GID=9001
CONTAINER_NAME="chronicle-timescaledb"
CONTAINER_IMAGE="docker.io/timescale/timescaledb:latest-pg16"
PG_PASSWORD="${PG_PASSWORD:?PG_PASSWORD must be set}"
DB_DATA_DIR="${QSAFE_HOME}/timescaledb-data"
SERVICE_NAME="chronicle-timescaledb"

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
            --comment "Chronicle service user" \
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
}

start_container() {
    log_info "Starting TimescaleDB container"

    # Stop and remove existing container if it exists
    if podman ps -a --format "{{.Names}}" | grep -q "^${CONTAINER_NAME}$"; then
        log_info "Stopping existing container"
        podman stop ${CONTAINER_NAME} 2>/dev/null || true
        podman rm ${CONTAINER_NAME} 2>/dev/null || true
    fi

    # Run the container
    podman run -d \
        --name ${CONTAINER_NAME} \
        --restart always \
        --user ${QSAFE_UID}:${QSAFE_GID} \
        --userns keep-id \
        -e POSTGRES_USER=qsafe \
        -e POSTGRES_PASSWORD=${PG_PASSWORD} \
        -e POSTGRES_DB=chronicle \
        -e POSTGRES_INITDB_ARGS="--encoding=UTF8 --lc-collate=C --lc-ctype=C" \
        -e TIMESCALEDB_TELEMETRY=off \
        -e TS_TUNE_MAX_CONNS=100 \
        -e TS_TUNE_MAX_BG_WORKERS=8 \
        -e POSTGRES_MAX_CONNECTIONS=100 \
        -e POSTGRES_SHARED_BUFFERS=256MB \
        -e POSTGRES_EFFECTIVE_CACHE_SIZE=1GB \
        -v ${DB_DATA_DIR}:/var/lib/postgresql/data:Z \
        -p 5432:5432 \
        --memory=2g \
        --memory-reservation=1g \
        --cpus=2 \
        --health-cmd="pg_isready -d chronicle" \
        --health-interval=30s \
        --health-timeout=5s \
        --health-retries=3 \
        --health-start-period=60s \
        ${CONTAINER_IMAGE}

    if [ $? -eq 0 ]; then
        log_info "Container started successfully"
    else
        log_error "Failed to start container"
        exit 1
    fi
}

create_systemd_service() {
    log_info "Creating systemd service"

    cat > /etc/systemd/system/${SERVICE_NAME}.service <<EOF
[Unit]
Description=Chronicle TimescaleDB Database (Podman)
After=network-online.target
Wants=network-online.target

[Service]
Type=forking
Restart=always
RestartSec=30
TimeoutStartSec=300
Environment="PODMAN_SYSTEMD_UNIT=%n"
ExecStartPre=/bin/rm -f %t/%n.ctr-id
ExecStart=/usr/bin/podman start ${CONTAINER_NAME}
ExecStop=/usr/bin/podman stop -t 30 ${CONTAINER_NAME}
ExecStopPost=/usr/bin/podman rm -f ${CONTAINER_NAME}
PIDFile=%t/%n.ctr-id

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable ${SERVICE_NAME}
    log_info "Systemd service created and enabled"
}

wait_for_database() {
    log_info "Waiting for database to be ready..."

    local max_attempts=30
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        if podman exec ${CONTAINER_NAME} pg_isready -d chronicle &>/dev/null; then
            log_info "Database is ready!"
            return 0
        fi

        log_info "Waiting for database... (attempt $attempt/$max_attempts)"
        sleep 2
        ((attempt++))
    done

    log_error "Database failed to start after $max_attempts attempts"
    return 1
}

run_init_script() {
    log_info "Initializing database"

    # Create initialization SQL
    cat <<EOF | podman exec -i ${CONTAINER_NAME} psql -d chronicle || log_warn "Some commands may have failed (normal if re-running)"
-- Enable TimescaleDB extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Create chronicle user if it doesn't exist
DO $$
DECLARE pw text := '${PG_PASSWORD}';
BEGIN
    IF NOT EXISTS (SELECT FROM pg_user WHERE usename = 'chronicle') THEN
        EXECUTE 'CREATE USER chronicle';
    END IF;
    IF pw IS NOT NULL AND length(pw) > 0 THEN
        EXECUTE 'ALTER USER chronicle WITH PASSWORD ' || quote_literal(pw);
    END IF;
END
$$;

-- Grant necessary permissions
GRANT CREATE ON DATABASE chronicle TO chronicle;
GRANT ALL ON SCHEMA public TO chronicle;

-- Create a schema for chronicle metadata
CREATE SCHEMA IF NOT EXISTS chronicle_meta;
GRANT ALL ON SCHEMA chronicle_meta TO chronicle;

-- Apply configuration changes
SELECT pg_reload_conf();
EOF

    log_info "Database initialization complete"
}

show_status() {
    echo ""
    echo "Container Status:"
    echo "-----------------"
    podman ps -a --filter "name=${CONTAINER_NAME}" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"

    echo ""
    echo "Database Status:"
    echo "-----------------"
    if podman exec ${CONTAINER_NAME} pg_isready -d chronicle &>/dev/null; then
        echo "Database is ready and accepting connections"
    else
        echo "Database is not ready"
    fi

    echo ""
    echo "Systemd Service Status:"
    echo "-----------------------"
    systemctl status ${SERVICE_NAME} --no-pager | head -n 10 || echo "Service not installed"
}

show_connection_info() {
    log_info "================================================================"
    log_info "Chronicle TimescaleDB has been deployed successfully!"
    log_info "================================================================"
    log_info ""
    log_info "Connection Information:"
    log_info "  Host: localhost"
    log_info "  Port: 5432"
    log_info "  Database: chronicle"
    log_info "  Username: qsafe"
    # Password intentionally not printed; set via PG_PASSWORD environment variable
    log_info ""
    log_info "Chronicle Connection String:"
    log_info "  PG_DSN=\"postgresql:///chronicle\""
    log_info ""
    log_info "Container Management:"
    log_info "  Status:  podman ps -a --filter name=${CONTAINER_NAME}"
    log_info "  Logs:    podman logs -f ${CONTAINER_NAME}"
    log_info "  Stop:    podman stop ${CONTAINER_NAME}"
    log_info "  Start:   podman start ${CONTAINER_NAME}"
    log_info "  Restart: podman restart ${CONTAINER_NAME}"
    log_info "  Shell:   podman exec -it ${CONTAINER_NAME} bash"
    log_info ""
    log_info "Database Access:"
    log_info "  psql -d chronicle"
    log_info "  podman exec -it ${CONTAINER_NAME} psql -d chronicle"
    log_info ""
    log_info "Service Management (if installed):"
    log_info "  systemctl status ${SERVICE_NAME}"
    log_info "  systemctl restart ${SERVICE_NAME}"
    log_info ""
}

# Main execution
main() {
    log_info "Starting Chronicle TimescaleDB deployment (Direct Podman)"

    # Checks
    check_root
    check_dependencies

    # Setup
    create_qsafe_user
    setup_directories

    # Start container
    start_container

    # Wait for database
    if wait_for_database; then
        run_init_script

        # Optionally create systemd service
        read -p "Do you want to create a systemd service for automatic startup? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            create_systemd_service
        fi

        show_connection_info
        show_status
    else
        log_error "Failed to start database"
        log_info "Check logs with: podman logs ${CONTAINER_NAME}"
        exit 1
    fi
}

# Handle script arguments
case "${1:-deploy}" in
    deploy)
        main
        ;;
    status)
        show_status
        ;;
    stop)
        log_info "Stopping container"
        podman stop ${CONTAINER_NAME}
        ;;
    start)
        log_info "Starting container"
        podman start ${CONTAINER_NAME}
        wait_for_database
        ;;
    restart)
        log_info "Restarting container"
        podman restart ${CONTAINER_NAME}
        wait_for_database
        ;;
    logs)
        podman logs -f ${CONTAINER_NAME}
        ;;
    shell)
        podman exec -it ${CONTAINER_NAME} bash
        ;;
    psql)
        podman exec -it ${CONTAINER_NAME} psql -d chronicle
        ;;
    remove)
        log_warn "This will stop and remove the container (data will be preserved)"
        read -p "Are you sure? (yes/no): " confirm
        if [ "$confirm" = "yes" ]; then
            podman stop ${CONTAINER_NAME} 2>/dev/null || true
            podman rm ${CONTAINER_NAME} 2>/dev/null || true
            systemctl disable ${SERVICE_NAME} 2>/dev/null || true
            rm -f /etc/systemd/system/${SERVICE_NAME}.service
            systemctl daemon-reload
            log_info "Container removed. Data preserved at: ${DB_DATA_DIR}"
        fi
        ;;
    purge)
        log_warn "This will remove the container AND all data!"
        read -p "Are you sure? (yes/no): " confirm
        if [ "$confirm" = "yes" ]; then
            podman stop ${CONTAINER_NAME} 2>/dev/null || true
            podman rm ${CONTAINER_NAME} 2>/dev/null || true
            systemctl disable ${SERVICE_NAME} 2>/dev/null || true
            rm -f /etc/systemd/system/${SERVICE_NAME}.service
            systemctl daemon-reload
            rm -rf ${DB_DATA_DIR}
            log_info "Container and data completely removed"
        fi
        ;;
    *)
        echo "Usage: $0 {deploy|status|stop|start|restart|logs|shell|psql|remove|purge}"
        echo ""
        echo "  deploy  - Deploy and start TimescaleDB (default)"
        echo "  status  - Show container and database status"
        echo "  stop    - Stop the container"
        echo "  start   - Start the container"
        echo "  restart - Restart the container"
        echo "  logs    - Follow container logs"
        echo "  shell   - Open bash shell in container"
        echo "  psql    - Open psql client in container"
        echo "  remove  - Remove container (preserves data)"
        echo "  purge   - Remove container and all data"
        exit 1
        ;;
esac
