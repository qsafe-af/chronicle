# orchestration/config

Per-chain and per-service environment files used by Quadlet units.

## Pattern
- chronicled: `chronicled-<base58-chain-id>.env`
- examples: `*.env.example` (safe defaults, **no secrets**)

## Example (chronicled-<base58>.env)
```env
# WebSocket endpoint for this chain
WS_URL=wss://a.t.res.fm

# Postgres DSN (same DB for all chains; per-chain schemas are created automatically)
PG_DSN=postgres://res:change-me@127.0.0.1:5432/chronicle

# Optional tuning
FINALITY_CONFIRMATIONS=200
RUST_LOG=info
```

> Keep real `.env` files **out of git** (see repo `.gitignore`). Use the provided `*.env.example` as templates.
