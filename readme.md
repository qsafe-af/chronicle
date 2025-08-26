# Chronicle

Multi-chain index + GraphQL backend for **quantum-safe (PQ) Substrate chains**.  
It walks from **genesis → head**, records per-account balance deltas (incl. miner rewards),
and serves flexible queries via Postgres/Timescale + Hasura.

## Components
- **chronicled** — Rust daemon that ingests blocks/events and writes facts.
- **chron-db** — shared DB helpers (schema DDL, utilities).
- **orchestration/** — Podman **Quadlet** units for TimescaleDB, Hasura and chronicled.
- **script/** — base58/hex helpers for chain IDs.

## Quick start
```bash
# 1) Install Quadlets (user scope)
mkdir -p ~/.config/containers/systemd
cp -v orchestration/quadlet/*.container orchestration/quadlet/*.volume ~/.config/containers/systemd/
systemctl --user daemon-reload

# 2) Configure containers
#   - copy/edit env examples as needed (keep real secrets out of git)
cp orchestration/config/chronicled-my-chain-id.env.example orchestration/config/chronicled-<base58>.env

# 3) Start DB + Hasura
systemctl --user enable --now qsafe-timescaledb.service qsafe-hasura.service

# 4) Start an indexer instance (instance name = base58 genesis-id)
systemctl --user enable --now qsafe-chronicled@<base58-genesis-id>.service
```

GraphQL will be available at `http://127.0.0.1:8080/` (single Hasura instance; query by schema = base58 chain id).

## Conventions
- **Schema per chain**: named exactly the **base58** of the chain’s **genesis hash**.
- **Drop a chain**: `DROP SCHEMA "<base58>" CASCADE;`
- **Base58 helpers**: see `script/`.

## Dev
```bash
cargo run -p chronicled
```

License: Apache-2.0
