# Sponsoring über den Twitch-Bot: Overlay-Werbung mit Umsatzbeteiligung

Stand: 2026-09-05, Konzept vor Contract. Klasse hoch (Geldpfad, externe Partner, neue Tabellen).

## TLDR

Sponsoren buchen bei uns eine Kampagne mit festem Budget. Partner-Streamer schalten
Sponsoring im Dashboard frei und binden eine OBS-Browser-Adresse ein. Der Bot blendet
die Kampagne nur ein, wenn der Streamer live ist, misst Sichtkontakte aus unseren
Zuschauerdaten und verteilt den Streamer-Anteil monatlich nach Zuschauerminuten per
Stripe Connect. Abrechnung, Auszahlung und Gutschrift-PDF gibt es schon (Affiliate-Pfad),
das Overlay gibt es schon (Stats-Overlay). Neu sind Kampagnen, Sichtkontakt-Messung,
Verteilschlüssel und die Dashboard-Seite.

## Bestand (EVIDENCE)

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-dashboard-api/src/handlers/overlay.rs:1, rust/crates/tb-analytics/src/affiliate_commission.rs:31, rust/crates/tb-analytics/src/stripe/client.rs:290 | Anknüpfung: Overlay-Render-Pfad, Stripe-Connect-Onboarding, Transfer und Gutschrift-PDF werden wiederverwendet

| Baustein | Stand | Fundstelle |
|---|---|---|
| OBS-Overlay (transparentes HTML, Poll alle 20 s, Builder-Seite) | live | `rust/crates/tb-dashboard-api/src/handlers/overlay.rs`, `bot/dashboard_v2/src/pages/OverlayBuilder.tsx`, Doku `rust/docs/stats-overlay.md` |
| Overlay-Route öffentlich per `?streamer=<login>`, ohne Geheimnis | live | `rust/crates/tb-dashboard-api/src/lib.rs:140` |
| Geheime OBS-Adressen je Streamer mit Token und Neu-Erzeugen | live (Uplink-Dock) | `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:485` (`dock_token_rotate_handler`) |
| Stripe-Client: Checkout, Produkte, Preise, Connect-OAuth, Transfer | live | `rust/crates/tb-analytics/src/stripe/client.rs:148,209,273,290` |
| Provisionsrechnung mit Prozentsatz je Konto, Pending-Deckel, Replay nach Connect | live (Affiliate) | `rust/crates/tb-analytics/src/affiliate_commission.rs:31,121,232,450` |
| Stripe-Connect-Onboarding per OAuth mit State-Token | live (Affiliate) | `rust/crates/tb-dashboard-api/src/handlers/affiliate.rs:309,360` |
| Gutschriften (Selbstabrechnung) mit USt-Status, PDF, Nummernkreis, Admin-Erzeugung | live (Affiliate) | `rust/migrations/20260617030000_baseline_missing_tables.sql:89`, `rust/crates/tb-dashboard-api/src/handlers/admin_affiliate.rs` |
| PII verschlüsselt (Name, Adresse, Steuernummer) | live (Affiliate) | `affiliate_pii` in derselben Migration |
| Twitch-Werbemanager (Twitch-eigene Commercials, Snooze, Helix-Scope) | live, anderes Thema | `rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs`, `tb_analytics::ad_manager` |
| Monetization-Seite (Ad-Break-Analyse, Ad-Zeitplan) | live | `bot/dashboard_v2/src/pages/Monetization.tsx`, `handlers/monetization.rs` |
| Werbefrei-Plan (schaltet Bot-eigene Chat-Werbung ab) | live | `rust/crates/tb-analytics/src/plan.rs:41`, `tb-chat/src/promos.rs:2253` |
| Zuschauerzahlen je Session und Zeitpunkt (Tracker) | live | `twitch_stream_sessions`, Poll-Samples (tb-monitoring) |

Nicht vorhanden: Kampagnen-Tabellen, Sichtkontakt-Messung, Verteilschlüssel über mehrere
Streamer, Sponsor-Overlay-Pfad, Streamer-Opt-in, Admin-Kampagnenpflege.

Abweichung im Bestand: Affiliate-Tabellen schlüsseln nach `twitch_login`. Neue
Sponsoring-Tabellen schlüsseln nach `twitch_user_id` (Nutzerregel). Beim Wiederverwenden
der Gutschrift-Logik wird die Kontosuche auf die ID umgestellt, nicht der Login übernommen.

## Zielbild

1. **Sponsor** (Firma aus dem Deadlock- oder Gaming-Umfeld) bucht eine Kampagne:
   Zeitraum, Budget netto, Werbemittel (Bild oder kurzes Video, Textzeile, Ziel-Link),
   Kategorie (für Ausschlüsse). Zahlung vorab per Stripe Checkout, Kampagne wird erst
   nach Zahlungseingang aktiv. v1 ohne Sponsor-Selbstbedienung: wir legen die Kampagne
   im Admin an, der Sponsor bekommt nur den Zahlungslink.
2. **Streamer** schaltet im Partner-Dashboard "Sponsoring" ein, wählt Position und
   ausgeschlossene Kategorien, verbindet Stripe (bestehender Connect-Weg) und bindet die
   OBS-Adresse ein. Ohne Stripe-Verbindung läuft das Overlay trotzdem, der Anteil bleibt
   ausstehend und wird nach Verbindung nachgezahlt (Replay-Mechanik vorhanden).
3. **Bot** liefert über die OBS-Adresse die aktive Kampagne, nur bei Live-Status. Das
   Overlay meldet sich alle 30 s beim Bot ("ich bin sichtbar"). Sichtkontakte rechnet der
   Bot serverseitig aus Heartbeat mal Zuschauerzahl aus dem Tracker, nicht aus dem Browser.
4. **Abrechnung** monatlich: Streamer-Topf je Kampagne wird nach Zuschauerminuten mit
   sichtbarem Overlay verteilt, Auszahlung per Stripe Transfer, Gutschrift-PDF je Streamer.

## Umsatzbeteiligung (Empfehlung)

- **Split 70/30**: 70 % des Netto-Budgets in den Streamer-Topf, 30 % bleiben bei uns
  (deckt Vertrieb, Stripe-Gebühren, Betrieb). Satz als Konstante mit Override je Kampagne.
- **Verteilschlüssel**: Anteil eines Streamers = seine Zuschauerminuten mit eingeblendetem
  Overlay geteilt durch die Summe aller Streamer in der Kampagne. Zuschauerminute =
  Zuschauerzahl aus dem Tracker mal Anzeigedauer. Kein Bonus für Stream-Stunden ohne
  Zuschauer, kein Deckel pro Streamer (größere Kanäle liefern mehr Kontakte und
  bekommen mehr).
- **Mindestauszahlung 10 €** netto, Rest wird in den Folgemonat vorgetragen (Muster
  Pending-Deckel im Affiliate-Pfad).
- **Gutschrift-Verfahren** wie bei Affiliates: Streamer gibt einmal Name, Adresse und
  USt-Status an, wir stellen monatlich die Gutschrift aus. Kleinunternehmer ohne USt,
  Regelbesteuerte mit 19 %. Ohne diese Angaben keine Auszahlung, aber Sichtkontakte
  laufen weiter auf.
- **Transparenz im Dashboard**: je Kampagne Sichtkontakte, Zuschauerminuten, eigener
  Anteil, Auszahlungsstatus, Gutschrift-Download.

Alternativen, nur falls gewünscht:
- Fester Betrag je Stream-Stunde mit Overlay (einfacher, belohnt aber leere Streams).
- CPM-Abrechnung mit dem Sponsor nach Sichtkontakten (Sponsor zahlt hinterher,
  Inkassorisiko bei uns, für v1 zu viel).

## Overlay

- Neue Adresse `GET /twitch/overlay/sponsor?token=<streamer-token>` im bestehenden
  Overlay-Handler. Token je Streamer wie beim Uplink-Dock (dauerhaft gespeichert,
  Neu-Erzeugen per Knopf), damit niemand fremde Heartbeats erzeugen kann. Die
  öffentliche `?streamer=login`-Form des Stats-Overlays bleibt unangetastet.
- Daten: `GET /twitch/api/v2/public/sponsor-overlay?token=` liefert Werbemittel, Slot
  und Anzeigedauer, oder leer (offline, kein Opt-in, keine aktive Kampagne, Kategorie
  ausgeschlossen). Heartbeat `POST .../sponsor-overlay/heartbeat` alle 30 s.
- Format v1: Eck-Banner bis 320 mal 100 Pixel, Einblendung 20 s alle 10 Minuten,
  dazwischen unsichtbar. Sichtbare Kennzeichnung "Anzeige" im Banner (Medienstaatsvertrag,
  UWG). Ziel-Link als Kurz-URL im Banner, weil OBS-Overlays nicht klickbar sind.
- Begleitende Chat-Nachricht zur Einblendung (ein Satz plus Link) über die bestehende
  Promo-Engine, mit eigenem Deckel (höchstens eine pro 30 Minuten je Kanal). Der
  Werbefrei-Plan blockt weiterhin nur die Bot-eigene Discord-Werbung; Sponsoring hat der
  Streamer selbst eingeschaltet und kann es jederzeit wieder ausschalten.
- Twitch-Regel: Sponsor-Overlays sind erlaubt, Twitch verlangt die Kennzeichnung
  "Branded Content" in den Stream-Infos. Der Bot setzt sie per Helix
  (`PATCH /helix/channels`, Feld `is_branded_content`, Scope `channel:manage:broadcast`)
  automatisch, solange eine Kampagne im Kanal läuft, und nimmt sie danach zurück.
  Vor dem Bau gegen die aktuelle Twitch-Richtlinie prüfen, ob die Helix-Flagge reicht.

## Datenmodell (neu, alles in `twitch_analytics`, Rolle postgres)

- `sponsor_campaigns`: id, sponsor_name, kategorie, zeitraum, budget_netto_cents,
  streamer_anteil_pct (Default 70), werbemittel (Bild-Bytes oder Pfad, Text, Link),
  slot_sekunden, intervall_minuten, stripe_checkout_id, bezahlt_at, status.
- `sponsor_streamer_settings`: twitch_user_id PK, aktiv, position, ausgeschlossene
  Kategorien, overlay_token, stripe_account_id, stripe_connect_status, created/updated.
- `sponsor_impressions`: id, campaign_id, twitch_user_id, stream_session_id, von, bis,
  zuschauer_avg, zuschauerminuten. Eine Zeile je Einblendung, aus Heartbeats gefaltet.
- `sponsor_settlements`: id, campaign_id, twitch_user_id, monat, zuschauerminuten,
  anteil_cents, status (pending, transferred, carried), stripe_transfer_id,
  gutschrift_id.
- Gutschriften: `affiliate_gutschriften` um `quelle` (affiliate, sponsoring) und
  `twitch_user_id` erweitern statt eine zweite Gutschrift-Tabelle zu bauen.

## Oberfläche

- Partner-Dashboard: neuer Tab "Sponsoring" auf der bestehenden Monetization-Seite
  (kein neues Dashboard). Inhalt: Schalter, Position, Kategorie-Ausschlüsse, OBS-Adresse
  mit Kopieren und Neu-Erzeugen, Stripe-Verbinden, Verdienstübersicht, Gutschriften.
- Admin-SPA: Kampagnen anlegen und bearbeiten, Zahlungslink erzeugen, Status, Vorschau
  des Werbemittels, Monatsabschluss auslösen (Gutschriften erzeugen wie bei Affiliates).
- Streamer-Landing `/streamer`: erst bewerben, wenn die erste Kampagne live war.

## Nicht-Ziele v1

- Kein Sponsor-Selbstbedienungsportal, kein Werbemittel-Upload durch Sponsoren.
- Keine Video-Werbemittel, kein Ton.
- Kein Targeting nach Zuschauerzahl oder Uhrzeit, nur Kategorie-Ausschluss.
- Keine Klick-Messung, nur Sichtkontakte.

## Entscheidungen, die offen sind

1. Split 70/30 und Verteilung nach Zuschauerminuten (Empfehlung) oder anderer Satz.
2. Sponsor zahlt Budget vorab per Checkout (Empfehlung) oder Rechnung mit Zahlungsziel.
3. Banner-Rhythmus 20 s alle 10 Minuten (Empfehlung) oder dauerhaft kleines Logo.
4. Begleitende Chat-Nachricht ja (Empfehlung) oder nur Overlay.
5. Mindestauszahlung 10 € (Empfehlung).

## Aufwand

Etwa 6 bis 8 Agententage: Backend (Tabellen, Overlay-Pfad, Heartbeat, Faltung,
Monatsabschluss, Transfer) 3 bis 4 Tage, Dashboard-Tab und Admin 2 Tage, Rechtstexte
(AGB-Zusatz Sponsoring, Gutschrift-Vorlage, Kennzeichnung) 1 Tag, Live-Beweis mit
Wegwerf-Konto und Test-Stripe 1 Tag.

## Nächster Schritt

Nach den fünf Entscheidungen: Contract anlegen (`CONTRACT.md` mit REQ, INV,
Nicht-Zielen, Scope inklusive `rust/.sqlx` und Schema-Snapshot), dann Plan, dann Bau
über Opus 4.8.
