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
CONTAINER_NAME="qsafe-hasura"
CONTAINER_IMAGE="docker.io/hasura/graphql-engine:v2.36.0"
HASURA_PORT="${HASURA_PORT:-8080}"
HASURA_ADMIN_SECRET="${HASURA_ADMIN_SECRET:-changeme}"
POSTGRES_PASSWORD="${POSTGRES_PASSWORD:-changeme}"
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_NAME="${DB_NAME:-res_index}"
DB_USER="${DB_USER:-qsafe}"
HASURA_DATA_DIR="${QSAFE_HOME}/hasura-data"
SERVICE_NAME="qsafe-hasura"

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

check_database() {
    log_info "Checking database connectivity..."

    # Check if we can connect to the database from the host
    if command -v pg_isready &> /dev/null; then
        if pg_isready -h ${DB_HOST} -p ${DB_PORT} -U ${DB_USER} -d ${DB_NAME} &>/dev/null; then
            log_info "✓ Database is reachable at ${DB_HOST}:${DB_PORT}"
            return 0
        else
            log_warn "Database is not reachable at ${DB_HOST}:${DB_PORT}"
            log_warn "Make sure TimescaleDB is running:"
            log_warn "  sudo ./deploy-timescaledb-podman.sh status"
            log_warn ""
            log_warn "Continuing anyway - Hasura will retry connection..."
            return 0
        fi
    else
        log_info "pg_isready not found, using alternative check..."

        # Try a simple TCP connection check
        if timeout 2 bash -c "cat < /dev/null > /dev/tcp/${DB_HOST}/${DB_PORT}" 2>/dev/null; then
            log_info "✓ Database port ${DB_PORT} is open at ${DB_HOST}"
            return 0
        else
            log_warn "Cannot verify database connectivity"
            log_warn "Make sure TimescaleDB is running:"
            log_warn "  sudo ./deploy-timescaledb-podman.sh status"
            log_warn ""
            log_warn "To install PostgreSQL client tools for better checks:"
            log_warn "  dnf install postgresql"
            log_warn ""
            log_warn "Continuing anyway - Hasura will retry connection..."
            return 0
        fi
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

    # Create data directory for Hasura metadata/migrations
    if [ ! -d "${HASURA_DATA_DIR}" ]; then
        log_info "Creating Hasura data directory: ${HASURA_DATA_DIR}"
        mkdir -p "${HASURA_DATA_DIR}"
        # Set permissions for container access (container runs as its own user)
        chmod 777 "${HASURA_DATA_DIR}"
    else
        log_info "Hasura data directory already exists: ${HASURA_DATA_DIR}"
        # Ensure permissions are correct
        chmod 777 "${HASURA_DATA_DIR}"
    fi

    # Create subdirectories for metadata and migrations
    for subdir in metadata migrations config; do
        if [ ! -d "${HASURA_DATA_DIR}/${subdir}" ]; then
            mkdir -p "${HASURA_DATA_DIR}/${subdir}"
            chmod 777 "${HASURA_DATA_DIR}/${subdir}"
        else
            # Ensure permissions are correct for existing directories
            chmod 777 "${HASURA_DATA_DIR}/${subdir}"
        fi
    done
}

start_container() {
    log_info "Starting Hasura GraphQL Engine container"

    # Stop and remove existing container if it exists
    if podman ps -a --format "{{.Names}}" | grep -q "^${CONTAINER_NAME}$"; then
        log_info "Stopping existing container"
        podman stop ${CONTAINER_NAME} 2>/dev/null || true
        podman rm ${CONTAINER_NAME} 2>/dev/null || true
    fi

    # Construct database URL - use host.containers.internal for localhost access
    if [ "${DB_HOST}" = "localhost" ] || [ "${DB_HOST}" = "127.0.0.1" ]; then
        DATABASE_URL="postgres://${DB_USER}:${POSTGRES_PASSWORD}@host.containers.internal:${DB_PORT}/${DB_NAME}"
    else
        DATABASE_URL="postgres://${DB_USER}:${POSTGRES_PASSWORD}@${DB_HOST}:${DB_PORT}/${DB_NAME}"
    fi

    # Run the container
    podman run -d \
        --name ${CONTAINER_NAME} \
        --restart always \
        -e HASURA_GRAPHQL_DATABASE_URL="${DATABASE_URL}" \
        -e HASURA_GRAPHQL_ADMIN_SECRET="${HASURA_ADMIN_SECRET}" \
        -e HASURA_GRAPHQL_ENABLE_CONSOLE=true \
        -e HASURA_GRAPHQL_DEV_MODE=false \
        -e HASURA_GRAPHQL_ENABLED_LOG_TYPES="startup,http-log,webhook-log,query-log" \
        -e HASURA_GRAPHQL_LOG_LEVEL=info \
        -e HASURA_GRAPHQL_ENABLE_TELEMETRY=false \
        -e HASURA_GRAPHQL_UNAUTHORIZED_ROLE=anonymous \
        -e HASURA_GRAPHQL_CORS_DOMAIN="*" \
        -e HASURA_GRAPHQL_STRINGIFY_NUMERIC_TYPES=false \
        -e HASURA_GRAPHQL_ENABLE_ALLOWLIST=false \
        -e HASURA_GRAPHQL_LIVE_QUERIES_MULTIPLEXED_REFETCH_INTERVAL=1000 \
        -e HASURA_GRAPHQL_CONNECTION_COMPRESSION=true \
        -e HASURA_GRAPHQL_WEBSOCKET_KEEPALIVE=30 \
        -e HASURA_GRAPHQL_WEBSOCKET_CONNECTION_INIT_TIMEOUT=180 \
        -v ${HASURA_DATA_DIR}/metadata:/hasura-metadata:Z \
        -v ${HASURA_DATA_DIR}/migrations:/hasura-migrations:Z \
        -p ${HASURA_PORT}:8080 \
        --memory=1g \
        --memory-reservation=512m \
        --cpus=1 \
        --health-cmd="curl -f http://localhost:8080/healthz || exit 1" \
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
Description=QSafe Hasura GraphQL Engine (Podman)
After=network-online.target qsafe-timescaledb.service
Wants=network-online.target
Requires=qsafe-timescaledb.service

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

wait_for_hasura() {
    log_info "Waiting for Hasura to be ready..."

    local max_attempts=30
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        if curl -f -s http://localhost:${HASURA_PORT}/healthz &>/dev/null; then
            log_info "Hasura is ready!"
            return 0
        fi

        log_info "Waiting for Hasura... (attempt $attempt/$max_attempts)"
        sleep 2
        ((attempt++))
    done

    log_error "Hasura failed to start after $max_attempts attempts"
    return 1
}

configure_hasura() {
    log_info "Configuring Hasura metadata"

    # Create a basic metadata configuration
    cat > ${HASURA_DATA_DIR}/config/config.yaml <<EOF
version: 3
endpoint: http://localhost:${HASURA_PORT}
admin_secret: ${HASURA_ADMIN_SECRET}
metadata_directory: ${HASURA_DATA_DIR}/metadata
migrations_directory: ${HASURA_DATA_DIR}/migrations
actions:
  kind: synchronous
  handler_webhook_baseurl: http://localhost:3000
EOF

    # Create initial metadata if it doesn't exist
    if [ ! -f "${HASURA_DATA_DIR}/metadata/version.yaml" ]; then
        cat > ${HASURA_DATA_DIR}/metadata/version.yaml <<EOF
version: 3
EOF

        cat > ${HASURA_DATA_DIR}/metadata/databases.yaml <<EOF
- name: default
  kind: postgres
  configuration:
    connection_info:
      database_url:
        from_env: HASURA_GRAPHQL_DATABASE_URL
      pool_settings:
        max_connections: 50
        idle_timeout: 180
        retries: 1
        pool_timeout: 360
        connection_lifetime: 600
      use_prepared_statements: true
      isolation_level: read-committed
  tables: []
EOF

        chmod -R 777 "${HASURA_DATA_DIR}/metadata"
    fi

    log_info "Hasura configuration complete"
}

track_tables() {
    log_info "Tracking Chronicle tables in Hasura"

    # This would track the tables created by Chronicle
    # For now, we'll just show how to do it manually
    log_info "To track tables, use Hasura Console or CLI:"
    log_info "  1. Open console: http://localhost:${HASURA_PORT}/console"
    log_info "  2. Navigate to Data tab"
    log_info "  3. Track the tables you want to expose via GraphQL"
}

show_status() {
    echo ""
    echo "Container Status:"
    echo "-----------------"
    podman ps -a --filter "name=${CONTAINER_NAME}" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"

    echo ""
    echo "Hasura Status:"
    echo "--------------"
    if curl -f -s http://localhost:${HASURA_PORT}/healthz &>/dev/null; then
        echo "Hasura is ready and accepting connections"
        echo "GraphQL Endpoint: http://localhost:${HASURA_PORT}/v1/graphql"
        echo "Console: http://localhost:${HASURA_PORT}/console"
    else
        echo "Hasura is not ready"
    fi

    echo ""
    echo "Systemd Service Status:"
    echo "-----------------------"
    systemctl status ${SERVICE_NAME} --no-pager | head -n 10 || echo "Service not installed"
}

show_connection_info() {
    log_info "================================================================"
    log_info "QSafe Hasura GraphQL Engine has been deployed successfully!"
    log_info "================================================================"
    log_info ""
    log_info "Connection Information:"
    log_info "  GraphQL Endpoint: http://localhost:${HASURA_PORT}/v1/graphql"
    log_info "  GraphQL WSS:      ws://localhost:${HASURA_PORT}/v1/graphql"
    log_info "  Console:          http://localhost:${HASURA_PORT}/console"
    log_info "  Admin Secret:     ${HASURA_ADMIN_SECRET}"
    log_info ""
    log_info "API Access Examples:"
    log_info "  # Query with curl:"
    log_info "  curl -X POST http://localhost:${HASURA_PORT}/v1/graphql \\"
    log_info "    -H \"X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}\" \\"
    log_info "    -H \"Content-Type: application/json\" \\"
    log_info "    -d '{\"query\": \"{ __typename }\"}'"
    log_info ""
    log_info "  # Health check:"
    log_info "  curl http://localhost:${HASURA_PORT}/healthz"
    log_info ""
    log_info "Container Management:"
    log_info "  Status:  podman ps -a --filter name=${CONTAINER_NAME}"
    log_info "  Logs:    podman logs -f ${CONTAINER_NAME}"
    log_info "  Stop:    podman stop ${CONTAINER_NAME}"
    log_info "  Start:   podman start ${CONTAINER_NAME}"
    log_info "  Restart: podman restart ${CONTAINER_NAME}"
    log_info "  Shell:   podman exec -it ${CONTAINER_NAME} sh"
    log_info ""
    log_info "Service Management (if installed):"
    log_info "  systemctl status ${SERVICE_NAME}"
    log_info "  systemctl restart ${SERVICE_NAME}"
    log_info ""
    log_info "Configuration Files:"
    log_info "  Metadata:   ${HASURA_DATA_DIR}/metadata/"
    log_info "  Migrations: ${HASURA_DATA_DIR}/migrations/"
    log_info "  Config:     ${HASURA_DATA_DIR}/config/config.yaml"
    log_info ""
}

# Main execution
main() {
    log_info "Starting QSafe Hasura GraphQL Engine deployment (Direct Podman)"

    # Checks
    check_root
    check_dependencies
    check_database

    # Setup
    create_qsafe_user
    setup_directories

    # Start container
    start_container

    # Wait for Hasura
    if wait_for_hasura; then
        configure_hasura
        track_tables

        # Optionally create systemd service
        read -p "Do you want to create a systemd service for automatic startup? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            create_systemd_service
        fi

        show_connection_info
        show_status
    else
        log_error "Failed to start Hasura"
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
        wait_for_hasura
        ;;
    restart)
        log_info "Restarting container"
        podman restart ${CONTAINER_NAME}
        wait_for_hasura
        ;;
    logs)
        podman logs -f ${CONTAINER_NAME}
        ;;
    shell)
        podman exec -it ${CONTAINER_NAME} sh
        ;;
    console)
        log_info "Opening Hasura Console in browser..."
        log_info "URL: http://localhost:${HASURA_PORT}/console"
        log_info "Admin Secret: ${HASURA_ADMIN_SECRET}"
        if command -v xdg-open &> /dev/null; then
            xdg-open "http://localhost:${HASURA_PORT}/console" 2>/dev/null
        elif command -v open &> /dev/null; then
            open "http://localhost:${HASURA_PORT}/console" 2>/dev/null
        else
            log_warn "Could not auto-open browser. Please navigate to the URL manually."
        fi
        ;;
    graphql)
        # Interactive GraphQL query
        log_info "Enter GraphQL query (end with Ctrl+D):"
        query=$(cat)
        curl -X POST http://localhost:${HASURA_PORT}/v1/graphql \
            -H "X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}" \
            -H "Content-Type: application/json" \
            -d "{\"query\": \"${query}\"}" | jq . 2>/dev/null || echo "$result"
        ;;
    remove)
        log_warn "This will stop and remove the container (metadata will be preserved)"
        read -p "Are you sure? (yes/no): " confirm
        if [ "$confirm" = "yes" ]; then
            podman stop ${CONTAINER_NAME} 2>/dev/null || true
            podman rm ${CONTAINER_NAME} 2>/dev/null || true
            systemctl disable ${SERVICE_NAME} 2>/dev/null || true
            rm -f /etc/systemd/system/${SERVICE_NAME}.service
            systemctl daemon-reload
            log_info "Container removed. Metadata preserved at: ${HASURA_DATA_DIR}"
        fi
        ;;
    purge)
        log_warn "This will remove the container AND all metadata/migrations!"
        read -p "Are you sure? (yes/no): " confirm
        if [ "$confirm" = "yes" ]; then
            podman stop ${CONTAINER_NAME} 2>/dev/null || true
            podman rm ${CONTAINER_NAME} 2>/dev/null || true
            systemctl disable ${SERVICE_NAME} 2>/dev/null || true
            rm -f /etc/systemd/system/${SERVICE_NAME}.service
            systemctl daemon-reload
            rm -rf ${HASURA_DATA_DIR}
            log_info "Container and metadata completely removed"
        fi
        ;;
    *)
        echo "Usage: $0 {deploy|status|stop|start|restart|logs|shell|console|graphql|remove|purge}"
        echo ""
        echo "  deploy  - Deploy and start Hasura GraphQL Engine (default)"
        echo "  status  - Show container and Hasura status"
        echo "  stop    - Stop the container"
        echo "  start   - Start the container"
        echo "  restart - Restart the container"
        echo "  logs    - Follow container logs"
        echo "  shell   - Open shell in container"
        echo "  console - Open Hasura Console in browser"
        echo "  graphql - Execute GraphQL query interactively"
        echo "  remove  - Remove container (preserves metadata)"
        echo "  purge   - Remove container and all metadata"
        exit 1
        ;;
esac
