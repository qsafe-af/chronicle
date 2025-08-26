#!/usr/bin/env bash

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
metadata_dir=${script_dir}/../metadata


if ! command -v subxt &> /dev/null; then
    cargo install subxt-cli
fi
if ! command -v yq &> /dev/null; then
    pip install yq
fi

for chain_as_base64 in $(yq -r '.chains[] | @base64' ${script_dir}/../orchestration/config/chains.yml); do
    chain_id=$(echo "$chain_as_base64" | base64 -d | jq -r '.id')
    chain_endpoint=$(echo "$chain_as_base64" | base64 -d | jq -r '.endpoint')
    chain_genesis_hash=$(echo "$chain_as_base64" | base64 -d | jq -r '.genesis.hash')
    chain_genesis_base58=$(echo "$chain_as_base64" | base64 -d | jq -r '.genesis.base58')
    echo "id: ${chain_id}"
    echo "endpoint: ${chain_endpoint}"
    echo "hash: ${chain_genesis_hash}"
    echo "base58: ${chain_genesis_base58}"
    mkdir -p ${metadata_dir}/${chain_genesis_base58}
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
        if subxt metadata \
            --format ${format} \
            --url ${chain_endpoint} \
            --output-file ${metadata_dir}/${chain_genesis_base58}/metadata.${ext}; then
            echo "metadata (${format}): ${chain_genesis_base58}/metadata.${ext}"
        else
            echo "failed to obtain metadata (${format}) for ${chain_genesis_base58}"
        fi
    done
    echo
done
