#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[STEP]${NC} $1"
}

# Check if TimescaleDB is running
check_timescaledb() {
    log_step "Checking TimescaleDB status..."

    if command -v podman &> /dev/null; then
        if podman ps | grep -q qsafe-timescaledb; then
            log_info "TimescaleDB is running in container"
            return 0
        fi
    fi

    if pg_isready -h localhost -p 5432 &> /dev/null; then
        log_info "PostgreSQL is responding on localhost:5432"
        return 0
    fi

    log_warn "TimescaleDB doesn't appear to be running"
    log_info "Deploy it with: sudo ${SCRIPT_DIR}/deploy-timescaledb.sh"

    read -p "Do you want to deploy TimescaleDB now? (y/n): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        if [ -f "${SCRIPT_DIR}/deploy-timescaledb.sh" ]; then
            sudo "${SCRIPT_DIR}/deploy-timescaledb.sh"
        else
            log_error "deploy-timescaledb.sh not found"
            exit 1
        fi
    else
        log_warn "Continuing without TimescaleDB (may fail)"
    fi
}

# Check for test blockchain node
check_node() {
    log_step "Checking blockchain node..."

    # Try to detect what's configured
    if [ -f "${SCRIPT_DIR}/test-local.env" ]; then
        source "${SCRIPT_DIR}/test-local.env" > /dev/null 2>&1
    fi

    WS_URL="${WS_URL:-ws://localhost:9944}"

    # Convert ws/wss to http/https for curl test
    TEST_URL="${WS_URL/ws:/http:}"
    TEST_URL="${TEST_URL/wss:/https:}"
    TEST_URL="${TEST_URL%/}"

    log_info "Testing connection to: ${WS_URL}"

    # Basic connectivity test
    if curl -s -o /dev/null -w "%{http_code}" "${TEST_URL}" | grep -q "405\|200\|302"; then
        log_info "Node appears to be reachable"
    else
        log_warn "Cannot reach node at ${WS_URL}"
        log_info "Make sure a Substrate node is running"
        log_info "For testing, you can use:"
        log_info "  - A local development node"
        log_info "  - Public testnet: wss://westend-rpc.polkadot.io"
        log_info "  - Public testnet: wss://rococo-rpc.polkadot.io"
    fi
}

# Build chronicle
build_chronicle() {
    log_step "Building Chronicle..."

    cd "${PROJECT_ROOT}"

    if cargo build --release 2>&1 | tee /tmp/chronicle_build.log | grep -E "Compiling|Finished"; then
        log_info "Build successful"
    else
        log_error "Build failed. Check /tmp/chronicle_build.log for details"
        exit 1
    fi
}

# Run chronicle with test configuration
run_chronicle() {
    log_step "Starting Chronicle indexer..."

    cd "${PROJECT_ROOT}"

    # Load test environment
    if [ -f "${SCRIPT_DIR}/test-local.env" ]; then
        log_info "Loading test configuration from test-local.env"
        source "${SCRIPT_DIR}/test-local.env"
    else
        log_warn "test-local.env not found, using defaults"
        export PG_DSN="postgres://qsafe:changeme@localhost:5432/res_index"
        export WS_URL="ws://localhost:9944"
        export RUST_LOG="info"
        export ENABLE_TIMESCALE="true"
    fi

    # Show configuration
    echo
    log_info "Configuration:"
    echo "  Database: ${PG_DSN}"
    echo "  Node: ${WS_URL}"
    echo "  TimescaleDB: ${ENABLE_TIMESCALE}"
    echo "  Log Level: ${RUST_LOG}"
    echo

    # Run chronicled
    log_info "Starting indexer (Ctrl+C to stop)..."
    echo "----------------------------------------"

    exec cargo run --release
}

# Test database connection
test_database() {
    log_step "Testing database connection..."

    # Extract connection details from PG_DSN
    if [[ "${PG_DSN:-}" =~ postgres://([^:]+):([^@]+)@([^:]+):([^/]+)/(.+) ]]; then
        DB_USER="${BASH_REMATCH[1]}"
        DB_PASS="${BASH_REMATCH[2]}"
        DB_HOST="${BASH_REMATCH[3]}"
        DB_PORT="${BASH_REMATCH[4]}"
        DB_NAME="${BASH_REMATCH[5]}"

        export PGPASSWORD="${DB_PASS}"

        if psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d "${DB_NAME}" -c "SELECT version();" &> /dev/null; then
            log_info "Database connection successful"

            # Check for TimescaleDB
            if psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d "${DB_NAME}" -c "SELECT extversion FROM pg_extension WHERE extname = 'timescaledb';" 2>/dev/null | grep -q "[0-9]"; then
                log_info "TimescaleDB extension is installed"
            else
                log_warn "TimescaleDB extension not found"
            fi
        else
            log_error "Failed to connect to database"
            log_info "Check your PG_DSN configuration"
        fi

        unset PGPASSWORD
    else
        log_warn "Could not parse PG_DSN for testing"
    fi
}

# Show usage
usage() {
    echo "Chronicle Local Test Script"
    echo ""
    echo "Usage: $0 [COMMAND]"
    echo ""
    echo "Commands:"
    echo "  run      - Build and run Chronicle (default)"
    echo "  check    - Check dependencies only"
    echo "  build    - Build Chronicle only"
    echo "  test-db  - Test database connection"
    echo "  clean    - Clean build artifacts"
    echo "  help     - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0              # Build and run"
    echo "  $0 check        # Check if everything is ready"
    echo "  $0 test-db      # Test database connection"
    echo ""
}

# Main execution
main() {
    cd "${PROJECT_ROOT}"

    case "${1:-run}" in
        run)
            log_info "Chronicle Local Test Runner"
            echo "============================"
            check_timescaledb
            check_node
            test_database
            build_chronicle
            run_chronicle
            ;;
        check)
            check_timescaledb
            check_node
            test_database
            log_info "All checks complete"
            ;;
        build)
            build_chronicle
            ;;
        test-db)
            if [ -f "${SCRIPT_DIR}/test-local.env" ]; then
                source "${SCRIPT_DIR}/test-local.env" > /dev/null 2>&1
            fi
            test_database
            ;;
        clean)
            log_info "Cleaning build artifacts..."
            cd "${PROJECT_ROOT}"
            cargo clean
            log_info "Clean complete"
            ;;
        help|--help|-h)
            usage
            ;;
        *)
            log_error "Unknown command: $1"
            usage
            exit 1
            ;;
    esac
}

# Run main function
main "$@"
