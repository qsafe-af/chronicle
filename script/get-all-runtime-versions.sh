#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
metadata_dir=${script_dir}/../metadata

# Install dependencies if needed
if ! command -v subxt &> /dev/null; then
    echo "Installing subxt-cli..."
    cargo install subxt-cli
fi
if ! command -v yq &> /dev/null; then
    echo "Installing yq..."
    pip install yq
fi
if ! command -v jq &> /dev/null; then
    echo "Error: jq is required but not installed"
    exit 1
fi

# Function to make JSON-RPC calls
rpc_call() {
    local endpoint=$1
    local method=$2
    local params=$3

    curl -s -X POST \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"${method}\",\"params\":${params},\"id\":1}" \
        "${endpoint}" | jq -r '.result'
}

# Function to get runtime version at a specific block
get_runtime_version_at_block() {
    local endpoint=$1
    local block_hash=$2

    if [ -z "$block_hash" ] || [ "$block_hash" = "null" ]; then
        rpc_call "$endpoint" "state_getRuntimeVersion" "[]"
    else
        rpc_call "$endpoint" "state_getRuntimeVersion" "[\"${block_hash}\"]"
    fi
}

# Function to get block hash at height
get_block_hash() {
    local endpoint=$1
    local height=$2

    if [ -z "$height" ]; then
        rpc_call "$endpoint" "chain_getBlockHash" "[]"
    else
        rpc_call "$endpoint" "chain_getBlockHash" "[${height}]"
    fi
}

# Function to scan for runtime upgrades
find_runtime_versions() {
    local endpoint=$1
    local chain_base58=$2

    echo "Scanning for runtime versions on ${chain_base58}..."

    # Get current block height
    local latest_hash=$(get_block_hash "$endpoint" "")
    local latest_header=$(rpc_call "$endpoint" "chain_getHeader" "[\"${latest_hash}\"]")
    local latest_height=$(echo "$latest_header" | jq -r '.number' | xargs printf "%d\n")

    echo "Latest block height: ${latest_height}"

    # Get genesis runtime version
    local genesis_hash=$(get_block_hash "$endpoint" "0")
    local genesis_version=$(get_runtime_version_at_block "$endpoint" "$genesis_hash")
    local genesis_spec_version=$(echo "$genesis_version" | jq -r '.specVersion')

    echo "Genesis runtime version: ${genesis_spec_version}"

    # Array to store unique runtime versions
    declare -A runtime_versions
    runtime_versions["${genesis_spec_version}"]="0"

    # Get current runtime version
    local current_version=$(get_runtime_version_at_block "$endpoint" "")
    local current_spec_version=$(echo "$current_version" | jq -r '.specVersion')
    runtime_versions["${current_spec_version}"]="${latest_height}"

    echo "Current runtime version: ${current_spec_version}"

    # Binary search approach to find runtime upgrades
    # This is more efficient than checking every block
    find_version_changes() {
        local start=$1
        local end=$2
        local start_version=$3
        local end_version=$4

        if [ "$start_version" = "$end_version" ] || [ $((end - start)) -le 1 ]; then
            return
        fi

        local mid=$(( (start + end) / 2 ))
        local mid_hash=$(get_block_hash "$endpoint" "$mid")
        local mid_version_json=$(get_runtime_version_at_block "$endpoint" "$mid_hash")
        local mid_version=$(echo "$mid_version_json" | jq -r '.specVersion')

        if [ "$mid_version" != "$start_version" ]; then
            runtime_versions["${mid_version}"]="$mid"
            echo "Found runtime version ${mid_version} at block ${mid}"
            find_version_changes "$start" "$mid" "$start_version" "$mid_version"
        fi

        if [ "$mid_version" != "$end_version" ]; then
            find_version_changes "$mid" "$end" "$mid_version" "$end_version"
        fi
    }

    # Search for version changes between genesis and current
    if [ "$genesis_spec_version" != "$current_spec_version" ]; then
        echo "Searching for runtime upgrades between genesis and current block..."
        find_version_changes 0 "$latest_height" "$genesis_spec_version" "$current_spec_version"
    fi

    # Return sorted list of versions
    echo "Found runtime versions: ${!runtime_versions[@]}"
    for version in $(echo "${!runtime_versions[@]}" | tr ' ' '\n' | sort -n); do
        echo "${version}:${runtime_versions[$version]}"
    done
}

# Function to download metadata for a specific runtime version
download_runtime_metadata() {
    local endpoint=$1
    local chain_base58=$2
    local version=$3
    local block_height=$4

    echo "Downloading metadata for runtime v${version} (block ${block_height})..."

    local version_dir="${metadata_dir}/${chain_base58}/runtime-v${version}"
    mkdir -p "${version_dir}"

    # Get block hash at the height where this version was active
    local block_hash=""
    if [ "$block_height" != "latest" ]; then
        block_hash=$(get_block_hash "$endpoint" "$block_height")
    fi

    # Download metadata in different formats
    for format in hex json bytes; do
        case ${format} in
            hex)
                ext=hex
                ;;
            json)
                ext=json
                ;;
            bytes)
                ext=scale
                ;;
        esac

        local output_file="${version_dir}/metadata.${ext}"

        # Build subxt command
        local subxt_cmd="subxt metadata --format ${format} --url ${endpoint}"

        # Add version flag if not latest
        if [ "$block_height" != "latest" ] && [ -n "$block_hash" ]; then
            subxt_cmd="${subxt_cmd} --version ${version}"
        fi

        subxt_cmd="${subxt_cmd} --output-file ${output_file}"

        if eval "${subxt_cmd}" 2>/dev/null; then
            echo "  ✓ Downloaded ${format} format to runtime-v${version}/metadata.${ext}"
        else
            # Fallback: try getting metadata at specific block
            echo "  ⚠ Direct version download failed, trying at block ${block_height}..."
            if [ -n "$block_hash" ]; then
                # Use RPC to get metadata at specific block
                local metadata=$(rpc_call "$endpoint" "state_getMetadata" "[\"${block_hash}\"]")
                if [ -n "$metadata" ] && [ "$metadata" != "null" ]; then
                    case ${format} in
                        hex)
                            echo "$metadata" > "$output_file"
                            ;;
                        bytes)
                            echo "$metadata" | sed 's/^0x//' | xxd -r -p > "$output_file"
                            ;;
                        json)
                            # For JSON, we'd need to decode the metadata
                            # This is complex, so we'll skip if direct download fails
                            echo "  ✗ JSON format requires subxt for decoding"
                            continue
                            ;;
                    esac
                    echo "  ✓ Downloaded ${format} format via RPC to runtime-v${version}/metadata.${ext}"
                else
                    echo "  ✗ Failed to download ${format} format for v${version}"
                fi
            fi
        fi
    done

    # Save version info
    cat > "${version_dir}/version.json" <<EOF
{
  "specVersion": ${version},
  "blockHeight": ${block_height},
  "blockHash": "${block_hash:-null}"
}
EOF
}

# Main processing loop
echo "Chronicle Runtime Metadata Collector"
echo "===================================="
echo

for chain_as_base64 in $(yq -r '.chains[] | @base64' ${script_dir}/../orchestration/config/chains.yml); do
    chain_id=$(echo "$chain_as_base64" | base64 -d | jq -r '.id')
    chain_endpoint=$(echo "$chain_as_base64" | base64 -d | jq -r '.endpoint')
    chain_genesis_hash=$(echo "$chain_as_base64" | base64 -d | jq -r '.genesis.hash')
    chain_genesis_base58=$(echo "$chain_as_base64" | base64 -d | jq -r '.genesis.base58')

    echo "Processing chain: ${chain_id}"
    echo "  Endpoint: ${chain_endpoint}"
    echo "  Genesis hash: ${chain_genesis_hash}"
    echo "  Genesis base58: ${chain_genesis_base58}"
    echo

    # Create metadata directory
    mkdir -p "${metadata_dir}/${chain_genesis_base58}"

    # Find all runtime versions
    version_list=$(mktemp)
    find_runtime_versions "$chain_endpoint" "$chain_genesis_base58" | grep "^[0-9]" > "$version_list" || true

    if [ ! -s "$version_list" ]; then
        echo "Warning: Could not find runtime versions, downloading current metadata only"
        download_runtime_metadata "$chain_endpoint" "$chain_genesis_base58" "current" "latest"
    else
        # Download metadata for each version
        while IFS=: read -r version block_height; do
            download_runtime_metadata "$chain_endpoint" "$chain_genesis_base58" "$version" "$block_height"
        done < "$version_list"
    fi

    rm -f "$version_list"

    # Create summary file
    summary_file="${metadata_dir}/${chain_genesis_base58}/versions.json"
    echo "Creating version summary at ${summary_file}"

    # Build summary JSON
    echo "{" > "$summary_file"
    echo "  \"chain_id\": \"${chain_id}\"," >> "$summary_file"
    echo "  \"genesis_hash\": \"${chain_genesis_hash}\"," >> "$summary_file"
    echo "  \"genesis_base58\": \"${chain_genesis_base58}\"," >> "$summary_file"
    echo "  \"versions\": [" >> "$summary_file"

    first=true
    for version_dir in "${metadata_dir}/${chain_genesis_base58}"/runtime-v*/; do
        if [ -d "$version_dir" ] && [ -f "${version_dir}/version.json" ]; then
            if [ "$first" = false ]; then
                echo "," >> "$summary_file"
            fi
            cat "${version_dir}/version.json" | sed 's/^/    /' | head -n -1 >> "$summary_file"
            echo -n "    }" >> "$summary_file"
            first=false
        fi
    done

    echo "" >> "$summary_file"
    echo "  ]" >> "$summary_file"
    echo "}" >> "$summary_file"

    echo
    echo "----------------------------------------"
    echo
done

echo "Runtime metadata collection complete!"
echo "Metadata saved to: ${metadata_dir}"
