# script

Small helpers for chain ID conversions.

## Files
- `b58hex.py` — shared Python lib (hex ↔ base58, Bitcoin alphabet, no checksum)
- `hex2b58.sh` — **Hex → Base58** (accepts `0x` prefix)
- `b582hex.sh` — **Base58 → Hex** (prints `0x...`)

## Examples
```bash
# Hex → Base58 (compute chain id from genesis hash)
./script/hex2b58.sh 0xd939e389d83c1bdd...   # -> 7wwWHpnm...

# Base58 → Hex
./script/b582hex.sh 7wwWHpnmxRf1Cnvri1cQ6LUiwvDw1czPjL7ytnoxNNNg
# -> 0xd939e389d83c1bdd...

# Make them executable once
chmod +x script/*.sh script/b58hex.py
```

Use the base58 string as:
- per-chain **schema name** in Postgres
- the **instance** for `qsafe-chronicled@<base58>.service`
- the filename in `orchestration/config/chronicled-<base58>.env`
