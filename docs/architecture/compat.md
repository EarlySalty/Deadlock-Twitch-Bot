# compat/ — Architektur & Funktionsreferenz

> Pfad: `bot/compat/` · Stand: 2026-06-08 · 3 Dateien, ~236 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [raid.md](raid.md) (verschlüsselt Tokens mit `FieldCrypto`), [storage.md](storage.md), [core.md](core.md) (HTTP).

## 1. Zweck & Abgrenzung

`compat/` ist eine kleine Sammlung **Infrastruktur-Helfer für den Standalone-Betrieb** — Dinge, die der Bot braucht, wenn er nicht im großen Master-Bot-Prozess, sondern eigenständig läuft. Zwei Bausteine tragen das Gewicht:

1. **`field_crypto.py`** — **Feld-Level-Verschlüsselung** (AES-256-GCM) für sensible DB-Felder, allen voran die Raid-OAuth-Tokens in `twitch_raid_auth`.
2. **`http_client.py`** — ein **DNS-resilienter** aiohttp-Connector (gegen DNS-Aussetzer auf dem Host).

Abgrenzung: Trotz des Namens ist das **kein** „toter Compat-Code“ — `FieldCrypto` ist sicherheitskritisch und aktiv im Raid-Auth-Pfad.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `raid/auth.py` (Token-Ver-/Entschlüsselung über `FieldCrypto`), diverse Pfade mit eigenem HTTP (resilienter Connector). |
| **Nutzt** | `cryptography` (AES-GCM), `aiohttp`, keyring/ENV für den Schlüssel. |
| **DB** | keine eigene; verschlüsselt/entschlüsselt nur die Werte, die andere Module speichern. |
| **Secret-Namen** | der Feld-Verschlüsselungs-Schlüssel (keyring/ENV), optional DNS-Server-Liste (ENV). |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `field_crypto.py` | 160 | `FieldCrypto` — AES-256-GCM-Feldverschlüsselung. |
| `http_client.py` | 64 | `build_resilient_connector(...)` — DNS-resilienter aiohttp-Connector. |
| `__init__.py` | 12 | Paket-Helfer. |

## 4. Datenfluss / Lebenszyklus

**Verschlüsselung:** Beim Speichern eines sensiblen Felds (z. B. Raid-Token) ruft der Aufrufer `get_crypto().encrypt_field(plaintext, aad, kid="v1")` → erhält ein Blob (`bytes`) mit Key-Version (`kid`) und AAD-Bindung. Beim Lesen `decrypt_field(blob, aad)`; passt die AAD nicht (anderer Kontext), schlägt die Entschlüsselung fehl (`DecryptFailed`). Der Schlüssel wird einmalig geladen (`_load_keys`); `get_crypto()` cached die Instanz, `reset_crypto()` setzt sie zurück (Tests/Rotation).

**HTTP-Resilienz:** Wer eine robuste aiohttp-Session braucht, baut den Connector mit `build_resilient_connector(...)` — mit eigenen DNS-Servern (aus ENV via `_parse_env_dns`), DNS-Cache-TTL und Verbindungslimits.

## 5. Funktionsreferenz pro Datei

### field_crypto.py
Fehlerklassen: `CryptoError` (Basis), `KeyMissing`, `DecryptFailed`, `InvalidPayload`.
- `FieldCrypto` — `__init__`, `_load_keys()` (Schlüssel aus keyring/ENV, versioniert), `encrypt_field(plaintext, aad, kid="v1") -> bytes`, `decrypt_field(blob, aad) -> str`.
- `get_crypto() -> FieldCrypto` — gecachte Singleton-Instanz. `reset_crypto()` — Cache zurücksetzen.

### http_client.py
- `build_resilient_connector(*, dns_servers=None, ttl_dns_cache=300, family=AF_INET, limit=500, limit_per_host=0) -> aiohttp.TCPConnector` — Connector mit eigenem DNS-Resolver/Cache; `_parse_env_dns()` liest DNS-Server aus der Umgebung.

## 6. Datenbank & externe Schnittstellen

- **Keine eigene DB.** `FieldCrypto` verschlüsselt Werte, die andere Module (v. a. `raid/auth`) in `twitch_raid_auth` ablegen.
- **DNS:** optionaler eigener Resolver für den resilienten Connector.

## 7. Stolperfallen / Besonderheiten

- **AAD-Bindung ist Pflicht:** Ein mit AAD „X“ verschlüsseltes Blob lässt sich nur mit AAD „X“ entschlüsseln — vertauschte Kontexte → `DecryptFailed`. Beim Lesen exakt dieselbe AAD verwenden wie beim Schreiben.
- **Schlüssel-Versionierung (`kid`):** Blobs tragen die Key-Version; für eine Rotation muss der alte Schlüssel zum Entschlüsseln verfügbar bleiben, bis alles re-encryptet ist.
- **Kein Schlüssel → kein Zugriff:** Fehlt der Feld-Schlüssel (`KeyMissing`), sind verschlüsselte Felder unlesbar — Funktionsverlust, kein Datenleck. Vor `drop_legacy_tokens` (siehe [migrations.md](migrations.md)) sicherstellen, dass die Verschlüsselung läuft.
- **`compat/` ist nicht löschbar:** Trotz des Namens aktiver, sicherheitsrelevanter Code — nicht als veraltet behandeln.
