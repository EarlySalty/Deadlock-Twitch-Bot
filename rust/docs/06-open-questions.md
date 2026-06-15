# 06 — Offene Fragen / Risiken

Punkte, die **vor** der jeweiligen Phase entschieden sein müssen. Geordnet nach frühestem
Blocker. Erledigte Entscheidungen stehen unter „Geklärt".

## Geklärt

- **Discord-Gateway-Identität.** → Kein eigener Gateway. Discord läuft über die interne Bridge
  (Master-Broker 8770), jeder Bot besitzt nur seinen Teil. Siehe
  [`adr/0001-discord-via-bridge.md`](adr/0001-discord-via-bridge.md).
- **DB-Strategie.** → sqlx (runtime-`query`) + refinery-Migrations, bestehendes Prod-Schema als
  unveränderte Baseline. Siehe [`adr/0002`](adr/0002-db-sqlx-refinery-shared-schema.md).

## Vor Phase 0 (Foundation)

1. **AES-256-GCM-Interop** (`twitch_raid_auth`, `social_media_platform_auth`). Byte-Kompatibilität
   mit den Python-`cryptography`-Blobs (Nonce-Länge, AAD-Format, Encoding) muss durch einen
   Interop-Test bewiesen sein. Wenn inkompatibel: Re-Auth aller Streamer nötig. → Test ist Teil
   von Phase 0; Ausgang entscheidet Phase 6/8. Siehe
   [`adr/0003-crypto-interop-or-reauth.md`](adr/0003-crypto-interop-or-reauth.md).
2. **Fernet-Migration** (`dashboard_sessions`). Fernet (AES-128-CBC + HMAC-SHA256) hat kein
   RustCrypto-1:1-Äquivalent. Optionen: Fernet in Rust nachbauen (`fernet`-Crate prüfen) **oder**
   bestehende Sessions invalidieren (alle User neu einloggen) und auf AES-256-GCM gehen. Blockt
   `tb-crypto`-Session-Teil, aber nicht den Rest der Foundation (Sessions werden erst in der
   Dashboard-Phase live).
3. **TimescaleDB / Hypertables.** sqlx kennt keine Timescale-Typen; `compress`/`hypertable`-DDL nur
   via raw SQL. ADR 0002 legt fest: reines `query()` für Timescale-Tabellen, refinery-Migration mit
   raw SQL. Zu bestätigen, sobald die betroffenen Tabellen identifiziert sind.

## Vor Monitoring (Phase 4)

4. **EventSub-Subscription-Ownership beim Cutover.** Subscriptions leben bei Twitch, referenziert
   über DB-State. Klären: bestehende Subs übernehmen oder neu anlegen? Capacity-Limit beachten
   (`twitch_eventsub_capacity_snapshot`). Doppel-Subscription = doppelte Events.

## Vor Raid (Phase 6) / Social-Media (Phase 8)

5. **`oauth_state_tokens` von raid UND social-media geteilt.** Beide schreiben dieselbe Tabelle.
   Beim getrennten Cutover muss der `platform`-Discriminator stabil bleiben, sonst stören sich
   Rust-raid und Python-social-media. → Discriminator-Spalte vor Phase 6 verifizieren.

## Vor Billing (Phase 7)

6. **`fpdf2`-Ersatz** für Gutschrift-PDFs. `printpdf`/`genpdf` mit manuellem Layout oder
   Python-Sidecar. Entscheid vor Billing-Implementierung.

## Vor Social-Media (Phase 8) / Community (Coaching)

7. **Whisper-Transkription.** Keine vollwertige Rust-Lib. Entscheid: `whisper-rs` (whisper.cpp-FFI)
   vs. faster-whisper-Python-Sidecar. Betrifft social-media + community-coaching.

## Sicherheit (separat)

8. **`system/query` (Raw-SQL-Admin-Endpoint).** Soll Rust diesen Endpoint überhaupt 1:1 nachbauen?
   Empfehlung: Read-only-Guard + Statement-Whitelist statt freier SQL-Ausführung. Sicherheits-Review
   vor Phase 9.

## LLM-Layer (tb-llm, Phase 0)

9. **Anthropic-Tokens im MiniMax-Usage-Ledger?** Das Python-Orakel verbucht **nur MiniMax**-Tokens
   ins geteilte Ledger; der Anthropic-/Opus-Pfad schreibt dort nichts. `tb-llm` verbucht aktuell
   auch den Anthropic-Verbrauch (mit Anthropic-Modellnamen unterscheidbar), damit der gesamte
   LLM-Verbrauch dieses Bots an einer Stelle messbar ist. Falls das Ledger strikt MiniMax-only
   bleiben soll (z. B. weil eine 5h-Budget-Logik nur MiniMax-Tokens meint), den Anthropic-`record`
   in `crates/tb-llm/src/anthropic.rs` entfernen. → User-Entscheid.
