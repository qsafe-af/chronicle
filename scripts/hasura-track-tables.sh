#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
HASURA_URL="${HASURA_URL:-http://localhost:8080}"
HASURA_ADMIN_SECRET="${HASURA_ADMIN_SECRET:?HASURA_ADMIN_SECRET must be set}"
DB_NAME="${DB_NAME:-chronicle}"

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

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

check_dependencies() {
    local missing_deps=()

    for cmd in curl jq psql; do
        if ! command -v $cmd &> /dev/null; then
            missing_deps+=($cmd)
        fi
    done

    if [ ${#missing_deps[@]} -ne 0 ]; then
        log_error "Missing required dependencies: ${missing_deps[*]}"
        log_info "Install with:"
        log_info "  dnf install curl jq postgresql"
        exit 1
    fi
}

check_hasura() {
    if ! curl -f -s "${HASURA_URL}/healthz" &>/dev/null; then
        log_error "Hasura is not running at ${HASURA_URL}"
        log_info "Start Hasura with: sudo ./deploy-hasura-podman.sh start"
        exit 1
    fi
}

list_chain_schemas() {
    log_info "Querying database for indexed chains..."

    # Query for all schemas that look like Chronicle chain schemas
    # They should have the standard Chronicle tables
    local sql="
        SELECT DISTINCT s.schema_name
        FROM information_schema.schemata s
        WHERE s.schema_name NOT IN ('public', 'pg_catalog', 'information_schema', 'timescaledb_information', 'chronicle_meta', '_timescaledb_internal')
        AND EXISTS (
            SELECT 1 FROM information_schema.tables t
            WHERE t.table_schema = s.schema_name
            AND t.table_name IN ('blocks', 'balance_changes', 'index_progress')
        )
        ORDER BY s.schema_name;
    "

    psql -d "${DB_NAME}" -t -A -c "${sql}" 2>/dev/null
}

get_chain_info() {
    local schema=$1

    # Get chain indexing progress info
    local sql="
        SELECT
            chain_id,
            latest_block,
            blocks_indexed,
            balance_changes_recorded,
            to_char(started_at, 'YYYY-MM-DD HH24:MI:SS') as started,
            to_char(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated
        FROM \"${schema}\".index_progress
        LIMIT 1;
    "

    local info=$(psql -d "${DB_NAME}" -t -A -F'|' -c "${sql}" 2>/dev/null)

    if [ -n "$info" ]; then
        IFS='|' read -r chain_id latest_block blocks_indexed changes_recorded started updated <<< "$info"
        echo "  Chain ID: ${chain_id}"
        echo "  Latest Block: ${latest_block}"
        echo "  Blocks Indexed: ${blocks_indexed}"
        echo "  Balance Changes: ${changes_recorded}"
        echo "  Started: ${started}"
        echo "  Last Updated: ${updated}"
    fi
}

track_table() {
    local schema=$1
    local table=$2

    log_info "Tracking table: ${schema}.${table}"

    # Create the GraphQL mutation to track the table
    local mutation=$(cat <<EOF
{
  "type": "pg_track_table",
  "args": {
    "source": "default",
    "schema": "${schema}",
    "name": "${table}"
  }
}
EOF
)

    # Send the mutation to Hasura
    local response=$(curl -s -X POST "${HASURA_URL}/v1/metadata" \
        -H "X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}" \
        -H "Content-Type: application/json" \
        -d "${mutation}" 2>/dev/null)

    # Check if successful or already tracked
    if echo "$response" | jq -e '.message' | grep -q "already tracked" 2>/dev/null; then
        log_warn "  Table ${schema}.${table} is already tracked"
    elif echo "$response" | jq -e '.error' >/dev/null 2>/dev/null; then
        log_error "  Failed to track ${schema}.${table}: $(echo "$response" | jq -r '.error')"
    else
        log_success "  Tracked ${schema}.${table}"
    fi
}

track_chain_tables() {
    local schema=$1

    log_info "Tracking tables for chain schema: ${schema}"

    # List of Chronicle tables to track
    local tables=(
        "blocks"
        "balance_changes"
        "index_progress"
        "account_stats"
        "metadata"
    )

    for table in "${tables[@]}"; do
        # Check if table exists before trying to track it
        local check_sql="
            SELECT EXISTS (
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = '${schema}'
                AND table_name = '${table}'
            );
        "

        local exists=$(psql -d "${DB_NAME}" -t -A -c "${check_sql}" 2>/dev/null)

        if [ "$exists" = "t" ]; then
            track_table "${schema}" "${table}"
        else
            log_warn "  Table ${schema}.${table} does not exist, skipping"
        fi
    done
}

create_relationships() {
    local schema=$1

    log_info "Creating relationships for schema: ${schema}"

    # Create foreign key relationship from balance_changes to blocks
    local rel_mutation=$(cat <<EOF
{
  "type": "pg_create_object_relationship",
  "args": {
    "source": "default",
    "table": {
      "schema": "${schema}",
      "name": "balance_changes"
    },
    "name": "block",
    "using": {
      "manual_configuration": {
        "remote_table": {
          "schema": "${schema}",
          "name": "blocks"
        },
        "column_mapping": {
          "block_number": "number"
        }
      }
    }
  }
}
EOF
)

    local response=$(curl -s -X POST "${HASURA_URL}/v1/metadata" \
        -H "X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}" \
        -H "Content-Type: application/json" \
        -d "${rel_mutation}" 2>/dev/null)

    if echo "$response" | jq -e '.message' | grep -q "already exists" 2>/dev/null; then
        log_warn "  Relationship balance_changes->blocks already exists"
    elif echo "$response" | jq -e '.error' >/dev/null 2>/dev/null; then
        log_error "  Failed to create relationship: $(echo "$response" | jq -r '.error')"
    else
        log_success "  Created relationship: balance_changes -> blocks"
    fi

    # Create array relationship from blocks to balance_changes
    local array_rel_mutation=$(cat <<EOF
{
  "type": "pg_create_array_relationship",
  "args": {
    "source": "default",
    "table": {
      "schema": "${schema}",
      "name": "blocks"
    },
    "name": "balance_changes",
    "using": {
      "manual_configuration": {
        "remote_table": {
          "schema": "${schema}",
          "name": "balance_changes"
        },
        "column_mapping": {
          "number": "block_number"
        }
      }
    }
  }
}
EOF
)

    response=$(curl -s -X POST "${HASURA_URL}/v1/metadata" \
        -H "X-Hasura-Admin-Secret: ${HASURA_ADMIN_SECRET}" \
        -H "Content-Type: application/json" \
        -d "${array_rel_mutation}" 2>/dev/null)

    if echo "$response" | jq -e '.message' | grep -q "already exists" 2>/dev/null; then
        log_warn "  Relationship blocks->balance_changes already exists"
    elif echo "$response" | jq -e '.error' >/dev/null 2>/dev/null; then
        log_error "  Failed to create relationship: $(echo "$response" | jq -r '.error')"
    else
        log_success "  Created relationship: blocks -> balance_changes[]"
    fi
}

show_example_queries() {
    local schema=$1

    echo ""
    echo "================================================================"
    echo "Example GraphQL Queries for chain: ${schema}"
    echo "================================================================"
    echo ""
    echo "# Get latest blocks:"
    echo "query {"
    echo "  ${schema}_blocks(limit: 10, order_by: {number: desc}) {"
    echo "    number"
    echo "    hash"
    echo "    timestamp"
    echo "    balance_changes_aggregate {"
    echo "      aggregate {"
    echo "        count"
    echo "      }"
    echo "    }"
    echo "  }"
    echo "}"
    echo ""
    echo "# Get balance changes for an account:"
    echo "query {"
    echo "  ${schema}_balance_changes("
    echo "    where: {account: {_eq: \"\\\\x...\"}}"
    echo "    order_by: {block_number: desc}"
    echo "    limit: 10"
    echo "  ) {"
    echo "    block_number"
    echo "    delta"
    echo "    reason"
    echo "    event_pallet"
    echo "    event_variant"
    echo "    block {"
    echo "      timestamp"
    echo "      hash"
    echo "    }"
    echo "  }"
    echo "}"
    echo ""
    echo "# Get account statistics:"
    echo "query {"
    echo "  ${schema}_account_stats(order_by: {balance: desc}, limit: 10) {"
    echo "    account"
    echo "    balance"
    echo "    total_changes"
    echo "    first_seen_block"
    echo "    last_activity_block"
    echo "  }"
    echo "}"
    echo ""
}

track_all_chains() {
    local chains=("$@")

    for chain in "${chains[@]}"; do
        echo ""
        echo "----------------------------------------"
        echo "Chain Schema: ${chain}"
        echo "----------------------------------------"
        get_chain_info "${chain}"
        echo ""
        track_chain_tables "${chain}"
        create_relationships "${chain}"
    done

    echo ""
    log_success "All chains tracked successfully!"

    # Show example queries for the first chain
    if [ ${#chains[@]} -gt 0 ]; then
        show_example_queries "${chains[0]}"
    fi
}

interactive_mode() {
    log_info "Fetching available chains..."

    # Get list of chain schemas
    mapfile -t chains < <(list_chain_schemas)

    if [ ${#chains[@]} -eq 0 ]; then
        log_warn "No Chronicle chain schemas found in database"
        log_info "Make sure Chronicle has indexed at least one chain"
        exit 0
    fi

    echo ""
    echo "Found ${#chains[@]} indexed chain(s):"
    echo "----------------------------------------"

    for i in "${!chains[@]}"; do
        echo "$((i+1)). ${chains[$i]}"
        get_chain_info "${chains[$i]}"
        echo ""
    done

    echo "----------------------------------------"
    echo "Select chains to track in Hasura:"
    echo "  a) All chains"
    echo "  s) Specific chain(s) - comma separated numbers"
    echo "  q) Quit"
    echo ""
    read -p "Your choice: " choice

    case $choice in
        a|A)
            track_all_chains "${chains[@]}"
            ;;
        s|S)
            read -p "Enter chain numbers (comma-separated): " selections
            IFS=',' read -ra selected <<< "$selections"

            selected_chains=()
            for sel in "${selected[@]}"; do
                # Remove spaces and validate number
                sel=$(echo "$sel" | tr -d ' ')
                if [[ "$sel" =~ ^[0-9]+$ ]] && [ "$sel" -ge 1 ] && [ "$sel" -le ${#chains[@]} ]; then
                    selected_chains+=("${chains[$((sel-1))]}")
                else
                    log_warn "Invalid selection: $sel"
                fi
            done

            if [ ${#selected_chains[@]} -gt 0 ]; then
                track_all_chains "${selected_chains[@]}"
            else
                log_error "No valid chains selected"
            fi
            ;;
        q|Q)
            log_info "Exiting..."
            exit 0
            ;;
        *)
            log_error "Invalid choice"
            exit 1
            ;;
    esac
}

main() {
    log_info "Chronicle Hasura Table Tracker"
    log_info "==============================="

    check_dependencies
    check_hasura

    # If chain ID is provided as argument, track just that chain
    if [ $# -eq 1 ]; then
        local chain_id="$1"
        log_info "Tracking tables for chain: ${chain_id}"

        # Verify the schema exists
        if ! list_chain_schemas | grep -q "^${chain_id}$"; then
            log_error "Chain schema '${chain_id}' not found in database"
            log_info "Available chains:"
            list_chain_schemas | sed 's/^/  - /'
            exit 1
        fi

        track_chain_tables "${chain_id}"
        create_relationships "${chain_id}"
        show_example_queries "${chain_id}"
    else
        # Interactive mode
        interactive_mode
    fi

    echo ""
    log_info "Access Hasura Console at: ${HASURA_URL}/console"
    log_info "Admin Secret is set via HASURA_ADMIN_SECRET"
}

# Handle script arguments
case "${1:-}" in
    --help|-h)
        echo "Usage: $0 [CHAIN_ID]"
        echo ""
        echo "Track Chronicle database tables in Hasura GraphQL Engine"
        echo ""
        echo "Options:"
        echo "  CHAIN_ID    Track tables for a specific chain (base58 genesis hash)"
        echo "              If not provided, enters interactive mode"
        echo ""
        echo "Environment variables:"
        echo "  HASURA_URL           Hasura endpoint (default: http://localhost:8080)"
        echo "  HASURA_ADMIN_SECRET  Hasura admin secret (required via environment)"
        echo "  DB_NAME              Database name (default: chronicle)"
        echo ""
        echo "Examples:"
        echo "  # Interactive mode - list all chains and select"
        echo "  $0"
        echo ""
        echo "  # Track specific chain"
        echo "  $0 FnX4ttSwm8kTZUvUkDbyPYS2txtcrW5pZ7kATWar2v1i"
        exit 0
        ;;
    *)
        main "$@"
        ;;
esac
