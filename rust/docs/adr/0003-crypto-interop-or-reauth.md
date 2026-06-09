# ADR 0003 — Krypto-Interop nachweisen oder bewusst Re-Auth erzwingen

- **Status:** vorgeschlagen (2026-06-08) — Ausgang hängt am Phase-0-Interop-Test
- **Kontext-Doku:** [`02-db-contract.md`](../02-db-contract.md), [`06-open-questions.md`](../06-open-questions.md)

## Kontext

Mehrere Spalten sind verschlüsselt und werden im Parallelbetrieb von Python **und** Rust gelesen:

- `twitch_raid_auth`, `social_media_platform_auth` — **AES-256-GCM** (Python `cryptography`).
- `dashboard_sessions` — **Fernet** (AES-128-CBC + HMAC-SHA256).

Damit Rust bestehende Tokens/Sessions weiterverwenden kann, muss es die Python-Blobs **byte-genau**
entschlüsseln (und kompatibel neu verschlüsseln). Risikopunkte: Nonce-Länge, AAD-Format, Key-Ableitung,
Base64-/Encoding-Konventionen.

## Entscheidung (zweistufig)

1. **AES-256-GCM:** In Phase 0 ein Interop-Test gegen echte (kopierte) Prod-Blobs. Besteht er, nutzt
   `tb-crypto` `aes-gcm` und übernimmt die bestehenden Tokens nahtlos. Besteht er **nicht** und ist
   die Differenz nicht überbrückbar, wird für `twitch_raid_auth`/`social_media_platform_auth` ein
   **Re-Auth** der betroffenen Streamer eingeplant (vor Phase 6/8).
2. **Fernet (`dashboard_sessions`):** Default = **Sessions invalidieren** (alle User loggen sich neu
   ein) und auf AES-256-GCM gehen, statt Fernet in Rust nachzubauen — geringeres Risiko, einmaliger
   sichtbarer Effekt (Re-Login). Nur falls Re-Login unerwünscht ist, wird die `fernet`-Crate auf
   Byte-Kompatibilität geprüft. Entscheidung spätestens vor der Dashboard-Phase.

## Konsequenzen

- AES-Interop ist ein **harter Gate** vor dem Raid-/Social-Cutover — kein Cutover ohne grünen Test
  oder bewusst akzeptierten Re-Auth.
- Fernet-Default kostet einen einmaligen Re-Login aller Dashboard-Nutzer, hält dafür `tb-crypto`
  schlank (ein AEAD-Verfahren statt zwei).
- Der Interop-Test wird zum dauerhaften Regressionstest in `tb-crypto`.

## Offen

- Ausgang des AES-256-GCM-Interop-Tests (Phase 0).
- Endgültige Fernet-Entscheidung (Nachbau vs. Re-Login) vor Dashboard-Phase.
