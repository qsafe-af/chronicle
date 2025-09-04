# orchestration/quadlet

Podman **Quadlet** unit files (systemd user units).

## Files
- `chronicle-timescaledb.container` / `.volume` — Postgres/TimescaleDB
- `chronicle-hasura.container` — Hasura GraphQL Engine (single instance)
- `chronicle-chronicled@.container` / `.volume` — templated chronicled daemon (one instance per chain)

## Install & start
```bash
mkdir -p ~/.config/containers/systemd
cp -v *.container *.volume ~/.config/containers/systemd/
systemctl --user daemon-reload

# DB + GraphQL
systemctl --user enable --now chronicle-timescaledb.service chronicle-hasura.service

# Indexer (instance name = base58 chain id)
systemctl --user enable --now chronicle-chronicled@<base58-genesis-id>.service
```

## Env wiring
- `chronicle-chronicled@.container` reads: `orchestration/config/chronicled-%i.env`
  where `%i` = the instance name (we recommend the base58 genesis id).

## Logs
```bash
journalctl --user -u chronicle-chronicled@<id>.service -f
journalctl --user -u chronicle-timescaledb.service -f
journalctl --user -u chronicle-hasura.service -f
```
