#!/usr/bin/env python3
"""Hermetisches Oracle: repliziert das Byte-Format von bot/compat/field_crypto.py.

Verifiziert byte-genau gegen den Produktionscode (zwei unabhängige Reviewer, 0 Abweichungen).
Liest eine JSON-Anfrage von stdin, schreibt eine JSON-Antwort nach stdout.
Der Schlüssel kommt aus DB_MASTER_KEY_V1 (Hex) — im Test ein Wegwerf-Key, nie der Prod-Key.

  encrypt: {"plaintext": str, "aad": str, "nonce_hex": optional str} -> {"blob_hex": str}
  decrypt: {"blob_hex": str, "aad": str}                            -> {"plaintext": str}
"""
import json
import os
import struct
import sys

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

VERSION = 1
NONCE_SIZE = 12
KID = "v1"


def load_key() -> bytes:
    key = bytes.fromhex(os.environ["DB_MASTER_KEY_V1"].strip())
    if len(key) != 32:
        raise SystemExit("DB_MASTER_KEY_V1 must be 32 bytes (64 hex chars)")
    return key


def encrypt(key: bytes, plaintext: str, aad: str, nonce: bytes) -> bytes:
    ct = AESGCM(key).encrypt(nonce, plaintext.encode("utf-8"), aad.encode("utf-8"))
    kid_b = KID.encode("ascii")
    return struct.pack("BB", VERSION, len(kid_b)) + kid_b + nonce + ct


def decrypt(key: bytes, blob: bytes, aad: str) -> str:
    version, kid_len = struct.unpack("BB", blob[:2])
    if version != VERSION:
        raise SystemExit(f"unknown version {version}")
    kid_end = 2 + kid_len
    nonce = blob[kid_end:kid_end + NONCE_SIZE]
    ct = blob[kid_end + NONCE_SIZE:]
    return AESGCM(key).decrypt(nonce, ct, aad.encode("utf-8")).decode("utf-8")


def main() -> None:
    cmd = sys.argv[1]
    key = load_key()
    req = json.load(sys.stdin)
    if cmd == "encrypt":
        nonce = bytes.fromhex(req["nonce_hex"]) if req.get("nonce_hex") else os.urandom(NONCE_SIZE)
        json.dump({"blob_hex": encrypt(key, req["plaintext"], req["aad"], nonce).hex()}, sys.stdout)
    elif cmd == "decrypt":
        pt = decrypt(key, bytes.fromhex(req["blob_hex"]), req["aad"])
        json.dump({"plaintext": pt}, sys.stdout)
    else:
        raise SystemExit(f"unknown command {cmd}")


if __name__ == "__main__":
    main()
