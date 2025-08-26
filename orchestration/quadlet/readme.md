# orchestration/quadlet

Podman **Quadlet** unit files (systemd user units).

## Files
- `qsafe-timescaledb.container` / `.volume` — Postgres/TimescaleDB
- `qsafe-hasura.container` — Hasura GraphQL Engine (single instance)
- `qsafe-chronicled@.container` / `.volume` — templated chronicled daemon (one instance per chain)

## Install & start
```bash
mkdir -p ~/.config/containers/systemd
cp -v *.container *.volume ~/.config/containers/systemd/
systemctl --user daemon-reload

# DB + GraphQL
systemctl --user enable --now qsafe-timescaledb.service qsafe-hasura.service

# Indexer (instance name = base58 chain id)
systemctl --user enable --now qsafe-chronicled@<base58-genesis-id>.service
```

## Env wiring
- `qsafe-chronicled@.container` reads: `orchestration/config/chronicled-%i.env`
  where `%i` = the instance name (we recommend the base58 genesis id).

## Logs
```bash
journalctl --user -u qsafe-chronicled@<id>.service -f
journalctl --user -u qsafe-timescaledb.service -f
journalctl --user -u qsafe-hasura.service -f
```
