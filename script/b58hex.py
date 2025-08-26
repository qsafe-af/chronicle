#!/usr/bin/env python3
"""
hex ↔ base58 (Bitcoin alphabet, no checksum)

CLI:
  b58hex.py hex2b58 <hex|0xhex>
  b58hex.py b582hex <base58>

Notes:
- Leading 00 bytes in hex become leading '1's in base58 (and vice-versa).
- Outputs lowercase hex (no 0x).
"""
import sys, re, binascii

ALPH = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
ALPH_MAP = {c: i for i, c in enumerate(ALPH)}

def hex_to_b58(h: str) -> str:
    if h.startswith(('0x','0X')):
        h = h[2:]
    h = h.strip().replace(' ', '').lower()
    if not h or len(h) % 2 != 0 or not re.fullmatch(r'[0-9a-f]+', h):
        raise ValueError('argument must be an even-length hex string (optionally with 0x)')
    b = binascii.unhexlify(h)
    n = int.from_bytes(b, 'big')

    s = ''
    while n:
        n, r = divmod(n, 58)
        s = ALPH[r] + s

    pad = len(b) - len(b.lstrip(b'\x00'))
    return ('1' * pad) + s if s else ('1' * pad)

def b58_to_hex(s: str) -> str:
    s = s.strip()
    if not s:
        raise ValueError('empty base58 string')
    bad = [c for c in s if c not in ALPH_MAP]
    if bad:
        raise ValueError(f'invalid base58 character(s): {"".join(sorted(set(bad)))}')

    n = 0
    for c in s:
        n = n * 58 + ALPH_MAP[c]

    b = b'' if n == 0 else n.to_bytes((n.bit_length() + 7) // 8, 'big')
    pad = len(s) - len(s.lstrip('1'))
    b = (b'\x00' * pad) + b
    return b.hex()

def usage(exit_code=1):
    prog = sys.argv[0].split('/')[-1]
    msg = f"Usage:\n  {prog} hex2b58 <hex|0xhex>\n  {prog} b582hex <base58>\n"
    sys.stderr.write(msg)
    raise SystemExit(exit_code)

def main():
    if len(sys.argv) != 3:
        usage()
    cmd, arg = sys.argv[1], sys.argv[2]
    try:
        if cmd == 'hex2b58':
            print(hex_to_b58(arg))
        elif cmd == 'b582hex':
            print(b58_to_hex(arg))
        else:
            usage()
    except ValueError as e:
        sys.stderr.write(f'error: {e}\n')
        raise SystemExit(2)

if __name__ == '__main__':
    main()
