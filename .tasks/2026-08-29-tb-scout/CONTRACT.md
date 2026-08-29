# Contract: tb-scout — Kleinkanal-Erkennung mit Admin-Freigabe

status: aktiv
datum: 2026-08-29
klasse: hoch

## Ziel

Neues Modul `tb-scout` im Twitch-Bot-Repo: findet kleine, erstmalig gesehene
Deadlock-Twitch-Kanäle (belegtes Erfolgsfmuster des Nutzers: 177 von 182
Recruitment-Raids an Kanäle unter 10 Zuschauern), legt sie im bestehenden
Admin-Dashboard (Research-Seite) zur Freigabe vor, und nur freigegebene
Kandidaten laufen über die BESTEHENDE Outreach-Kette (`twitch_partner_outreach`
plus Raid-Arrival-Recruitment) in die Ansprache. Der Nutzer kuratiert im
Dashboard, wen der Bot ansprechen darf.

## Anforderungen (user-sichtbar, prüfbar)

- REQ-01: Auf der Research-Seite gibt es einen Bereich "Scout-Freigaben" mit
  Kandidaten, die (a) im Lookback höchstens 5 Sessions haben, (b) deren
  erste Sichtuing (`MIN(ts_utc)` aus `twitch_stats_category`) höchstens 60 Tage
  zurückliegt, (c) deren mittlere Zuschauerzahl höchstens 10 beträgt und (d)
  die nicht Partner sind. Gezeigt werden: Login, Sessionzahl, Ø Zuschauer,
  first_seen, last_seen, Sprache, Deadlock-Anteil.
- REQ-02: Je Kandidat genau eine Aktion: Freigeben, Überspringen, Pausieren —
  mit optionalem Grund (z. B. "Einzelgänger", "hat kein Bock"). Der Zustand
  bleibt in der DB gespeichert und überlebt Neustarts.
- REQ-03: Harte Filter laufen automatisch und ohne Nutzeraktion: bereits
  Partner, `twitch_raid_blacklist`, `twitch_partner_signup_denylist`,
  `twitch_scout_pitch_blacklist`, globaler Ban (`is_hard_banned`), aktive
  Outbound-Suppression `source='recruitment'`, aktiver
  Outreach-Eintrag/Cooldown in `twitch_partner_outreach`. Gefilterte Kandidaten
  erscheinen nicht in der Freigabeliste.
- REQ-04: Ein freigegebener Kandidat wird vom Bot ausschließlich über den
  bestehenden Weg angesprochen: Einreihung in `twitch_partner_outreach`
  (bestehende Limits 8/Tag, 3/Tick, 30 Tage Cooldown, Gates in
  `partner_recruit.rs` und `raid_arrival_wiring.rs` bleiben unangetastet).
  Keine neue Nachrichtenart, kein kalter Erstkontakt jenseits des Bestands.
- REQ-05: Freigaben und Überspringungen des Nutzers werden nie automatisch
  überschrieben. Ein pausierter oder übersprungener Kandidat taucht nicht
  erneut auf.

## Invarianten (was sich nicht ändern darf)

- INV-01: Das Coaching-Modul (`tb-stream-audit`, Binärprogramm und Crate) wird
  nicht angerückt: keine Code-Änderungen, keine Verhaltensänderung.
- INV-02: Die bestehende automatische Erkennung in `partner_recruit.rs`
  (häufige Streamer, 28-Tage-Lookback) bleibt unverändert bestehen und läuft
  weiter wie heute.
- INV-03: Keine Identitätsmerkmale (Herkunft, Weltanschauung, sexuelle
  Orientierung, Geschlecht) als Filter, Score oder Prompt-Bestandteil. Filter
  wirken nur auf Verhalten, Listenbestand und Sendedaten.
- INV-04: Kein Live-Mitschnitt fremder Kanäle, kein Whisper-Zwang in diesem
  Slice. Kein LLM hart verdrahtet (falls ein Judge kommt, nur über tb-llm).
- INV-05: Bestehende Routen, Handler und Dashboard-Flächen ändern ihr
  Verhalten nicht; der Scout-Bereich kommt als Zusatz auf die bestehende
  Research-Seite, keine Standalone-Route.
- INV-06: Neue Schreibpfade müssen idempotent sein und Neustarts überleben
  (Zustand nur in der zentralen PG, nie im Speicher allein).

## Nicht-Ziele

- Keine Whisper/Clip-Inhaltsanalyse in diesem Slice (möglicher Nachfolgeslice).
- Kein automatischer Raid-Trigger, kein Massen-DM, kein zweites Dashboard.
- Keine Änderung an Blacklist-Pflege, Denylist-Verwaltung oder Partneraufnahme.
- Kein Modellwechsel und kein neuer KI-Anbieter.

## Änderungsbereich

- Neu: Crate `rust/crates/tb-scout`, Migration(en) unter `rust/migrations/`
  (Tabelle `twitch_scout_candidates` inkl. Status, Grund, Entscheider,
  Zeitstempel), Handler in `tb-dashboard-api` (2 Routen, admin-gated, CSRF),
  Ergänzungen an `bot/admin_dashboard` (Research-Seite, API-Client,
  Testanbindung), ein Tick-Anschluß in `rust/bin/tb-bot` (dispatch approved →
  bestehendes `enqueue_partner_outreach`).
- Erlaubt: minimal nötige Erweiterungen dieser Dateien.
- Verboten: alles außerhalb (insbesondere `tb-stream-audit`, `tb-raid`
  Gate-Logik, OAuth, Rechnungs-/Billing-Pfade).

## Offene Produktfragen

- Keine. Ansprache-Texte bleiben exakt die bestehenden der Recruitment-Kette.
