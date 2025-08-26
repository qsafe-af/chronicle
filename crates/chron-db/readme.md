# chron-db

Shared database helpers for **chronicled**.

## Responsibilities
- Create/upgrade the per-chain schema (named by base58 genesis).
- Provide SQL strings / helpers for:
  - `blocks`, `balance_changes`, `accounts`, `balance_snapshots`
  - optional Timescale hypertable creation on `balance_changes(block_ts)`
  - utility SQL (e.g., `balance_at(account, block)`)

## Usage
Add as a workspace dependency and call its DDL helpers at daemon startup.

## Future
Consider adding proper migrations (e.g., `refinery`/`sqlx migrate`) once schemas stabilize.
