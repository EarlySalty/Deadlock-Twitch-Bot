# Affiliate Claim-Zeitfenster (Reservierung) — Design/Spec

Datum: 2026-07-03 · Branch: `feat/affiliate-claim-window` · Repo: Deadlock-Twitch-Bot (Rust, live)

## Ziel

Affiliates werben **neue** Streamer an. Der Claim soll nicht mehr „jeder claimt jeden
ungeclaimten Streamer" sein, sondern ein **Reservierungs-Zeitfenster** relativ zur
Aktivierung des Streamers. Zweck: nur echte Neu-Anwerbungen belohnen, keine
Alt-Partner-Abgriffe; toter Reservierung nach Ablauf den Slot wieder freigeben.

## Anker A = Aktivierung des geworbenen Streamers

**A = `twitch_partners.partnered_at`** — der Moment, in dem der Streamer aktiver Partner
wird („wir haben die Zugangsdaten, der Bot kann aktiv werden"). In der bestehenden View
`twitch_streamers_partner_state` als Spalte **`created_at`** sichtbar; `is_partner_active`
wird aus `twitch_partners.status='active'` (+ nicht opt-out/pausiert/archiviert) berechnet.
Kein neuer Zeitstempel, keine neue Tabelle.

Datentypen: `affiliate_streamer_claims.claimed_at` ist **TEXT (ISO-8601)**,
`partnered_at` (View `created_at`) ist **TEXT**. Fensterrechnung daher in SQL mit
`::timestamptz` + `INTERVAL`, nie String-Vergleich.

## Regel (eine Invariante)

Ein Claim ist **provisionsberechtigt** genau dann, wenn
`claimed_at ∈ [A − RESERVATION_TTL, A + POST_ACTIVATION_GRACE]`.

Konstanten (bewusst zentral + tweakbar, User kann später revidieren):
- `RESERVATION_TTL = 4 Tage`
- `POST_ACTIVATION_GRACE = 24 Stunden`

Interpretation der beiden Grenzen:
- **−4 Tage (Voraus/Reservierung):** Affiliate darf einen Login reservieren, **bevor** der
  Streamer aktiv ist. Wird der Streamer binnen 4 Tagen aktiv → Claim greift. Sonst
  **läuft die Reservierung ab** und der Slot ist wieder frei (kein harter Fehler, nur
  überschreibbar — „nach 4 Tagen kann man neu claimen").
- **+24h (Nachfrist):** Ist der Streamer gerade erst (≤24h) aktiver Partner geworden,
  darf man ihn noch claimen.

## Enforcement — zweistufig

Ein einziger geteilter Ort für Konstanten + Fenster-Prädikat (DRY, sonst driften die zwei
Stellen auseinander = klassischer Bug). Implementierer: Konstanten + Helper in die
**tiefste gemeinsame Crate** legen (voraussichtlich `tb-analytics`, die `tb-dashboard-api`
ohnehin nutzt). Falls keine gemeinsame Crate existiert: duplizieren **mit** einem Test, der
Gleichheit der Konstanten erzwingt.

### 1. Claim-Handler — `tb-dashboard-api/src/handlers/affiliate.rs::claim_streamer`

Ersetzt die heutige Logik (heute: Partner→reject, existing claim→reject). Neu, in **einer
Transaktion mit Zeilensperre/Advisory-Lock** auf `LOWER(claimed_streamer_login)` (Race-fest):

1. Partner-State des Streamers laden: `is_partner_active`, `created_at` (=`partnered_at`).
2. Bestehenden Claim laden (falls vorhanden): `affiliate_twitch_login`, `claimed_at`.
3. **Bestehender Claim blockiert**, wenn:
   - Streamer **aktuell aktiver Partner** ist (Claim ist „konvertiert"/verdient → dauerhaft), **oder**
   - Reservierung **noch frisch**: `now ≤ claimed_at + RESERVATION_TTL`.
   → `AlreadyClaimed` / `StreamerAlreadyRegistered` (409, wie heute).
4. **Bestehender Claim abgelaufen** (nicht-Partner **und** `now > claimed_at + RESERVATION_TTL`):
   überschreibbar → alten Claim löschen und neu einfügen (Slot-Reclaim).
5. Für einen **neuen** Claim (kein blockierender Bestand):
   - Nicht-Partner → **erlauben** (Pre-Claim/Reservierung).
   - Aktiver Partner, `partnered_at` **≤24h** → erlauben (Nachfrist).
   - Aktiver Partner, `partnered_at` **>24h** → ablehnen (`StreamerAlreadyRegistered`; etablierter Partner).
6. INSERT `claimed_at = now` (rfc3339 micros, wie bisher). Unique-Verletzung im Race → `AlreadyClaimed`.

Der `POLICY(offen)`-Kommentar in `claim_handler` (aktuell affiliate.rs:418) wird durch die
neue Policy-Beschreibung ersetzt.

### 2. Geld-Choke-Point — `tb-analytics/src/affiliate_commission.rs`

Der Resolver (heute ~affiliate_commission.rs:126–135) löst bei `invoice.payment_succeeded`
Streamer→Claim→Affiliate. **Zusätzlich**: das volle Fenster gegen `partnered_at` prüfen.
Provision **nur**, wenn `claimed_at ∈ [partnered_at − 4T, partnered_at + 24h]`.

- Claim vor dem Fenster (veraltete Reservierung, Streamer erst später aktiv) → **keine
  Provision** (`CommissionOutcome::NoAffiliate`), damit ein toter Pre-Claim, der zufällig
  noch im Slot liegt, kein Geld zieht.
- Claim nach dem Fenster (sollte durch das Claim-Gate nicht vorkommen) → defensiv ebenfalls keine Provision.
- **Edge — `partnered_at` fehlt** (kein aktiver-Partner-Datensatz, Datenanomalie):
  **Default = keine Provision + laute `tracing::warn!`** (Geld-Sicherheit vor unbewiesener
  Berechtigung; sichtbar für manuelle Klärung). → **User-Entscheid im Spec-Review** (alternativ:
  zahlen + warnen).

## Nicht-Ziele (bewusst außen vor)

- Kein Referral-Token / Consent / Admin-Freigabe (separater Folge-Task).
- Keine neue Tabelle/Migration — `affiliate_streamer_claims` + View `twitch_streamers_partner_state` reichen.
- Kein Grandfathering (es gibt noch keine Claims — bestätigt).
- Konstanten bleiben vorerst 4T/24h (später tweakbar).

## Vertrag = Tests (adversarial verifizierbar, gegen echte DB)

Claim-Handler (`affiliate.rs` Tests):
1. Nicht-Partner → Pre-Claim erfolgreich (`Ok`).
2. Etablierter aktiver Partner (`partnered_at` > 24h) → abgelehnt.
3. Frischer aktiver Partner (`partnered_at` ≤ 24h) → Claim erfolgreich (Nachfrist).
4. Bereits geclaimt, Reservierung frisch (≤4T) → abgelehnt.
5. Bereits geclaimt, Reservierung abgelaufen (>4T, Streamer weiter Nicht-Partner), anderer
   Affiliate → Claim erfolgreich (Slot-Reclaim; neuer `affiliate_twitch_login` + `claimed_at`).
6. Konvertierter Claim (Streamer wurde aktiver Partner) → bleibt blockiert, auch bei altem `claimed_at`.
7. Race: zwei parallele Claims auf denselben frischen Streamer → genau einer gewinnt, anderer `AlreadyClaimed`.

Commission (`affiliate_commission.rs` Tests):
8. Claim im Fenster → Provision attribuiert.
9. Claim vor Fenster (`claimed_at < partnered_at − 4T`) → keine Provision (`NoAffiliate`).
10. Claim nach Fenster (`claimed_at > partnered_at + 24h`) → keine Provision.
11. `partnered_at` fehlt → gemäß Review-Entscheid (Default: keine Provision + warn).

Konstanten-Sync (falls dupliziert): Test erzwingt gleiche `RESERVATION_TTL`/`GRACE` an beiden Stellen.

## Abschluss (CLAUDE.md-Workflow)

Umsetzung an Codex delegiert (Kritiker→Rework-Loop pro Stelle). Danach: `CHANGELOG.md`
(#N, user-sichtbar, Problem→Änderung→Verhalten) → Commit → Push → Merge nach `main` →
Binary-Swap-Deploy tb-dashboard → Live-Beweis. **Deploy ist User-Checkpoint** (Geld-Pfad),
nicht autonom.
