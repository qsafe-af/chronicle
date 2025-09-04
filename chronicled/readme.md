# chronicled

Rust daemon that indexes PQ Substrate chains and writes balance-change facts.

## What it does
- Detects chain **genesis hash**, derives **base58 chain id**, ensures a per-chain schema.
- Scans **genesis endowments** and **all balance-affecting events**.
- Handles reorgs (by block), and supports periodic balance snapshots (optional).
- Targets Postgres/Timescale; pairs with Hasura for GraphQL/aggregations.

## Config (env)
- `WS_URL` — WebSocket endpoint (e.g., `wss://a.t.res.fm`)
- `PG_DSN` — Postgres connection string (e.g., `postgresql:///chronicle`)
- `FINALITY_CONFIRMATIONS` — (optional) reorg buffer (e.g., `200`)
- `RUST_LOG` — e.g., `info` / `debug`

## Run locally
```bash
PG_DSN=postgresql:///chronicle WS_URL=wss://... cargo run -p chronicled
```

## Notes
- The per-chain schema name is the **base58** of the **raw 32-byte** genesis hash (Bitcoin alphabet, no checksum).
- Timescale hypertables/materialized views can be enabled later for performance.
