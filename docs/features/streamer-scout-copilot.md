# Streamer-Scout (Copilot-Modell) — Spec

> Stand: 2026-07-14 · Beschlossen im Planungs-Interview (EarlySalty). Datengrundlage:
> Stil-Analyse von 3.359 EarlySalty- und 4.111 Kubi-Nachrichten aus `twitch_chat_messages`
> (Jan–Jul 2026), davon 159 echte Pitch-Momente.

## Ziel

Neue deutsche Deadlock-Streamer für die Community/Partnerschaft gewinnen — so wie
EarlySalty es heute manuell macht: in fremden Streams entspannt mitchatten, bei
passendem Anlass den **Streamer** auf die Website pitchen
(`deutsche-deadlock-community.de/twitch/`), Discord optional erwähnen.

**Kernentscheidung (Copilot-Modell):** Der Bot sendet **nie selbst** in fremde
(Nicht-Partner-)Channels. Er beobachtet, erkennt Momente, formuliert Vorschläge —
gesendet wird ausschließlich manuell von EarlySalty (Copy-Paste). Das ist zugleich
die Shadow-Phase: Autonomie wird erst diskutiert, wenn die Vorschläge über Wochen
nachweislich gut sind. (Chatter-Engagement in Partner-Channels mit Auth läuft
unverändert über die bestehende Engagement-Pipeline.)

## Warum kein verdeckter Auto-Pitch

1. **ToS:** In fremden Channels ist ein als Mensch getarnter Bot mit Werbeabsicht
   Spam/Deception; Twitch-AI-Anfrage läuft parallel.
2. **Ruf:** Fliegt ein getarnter Pitch-Account auf, ist der Ruf der Community
   beschädigt, nicht nur der Account.
3. **Pitch-Qualität:** EarlySaltys stärkstes Argument ist Ownership („bin viel am
   entwickeln für die Community") — das kann nur er selbst glaubwürdig sagen.

## Erfolgsrezept aus der Stil-Analyse (Vier-Schritt-Muster)

1. **Anlass abwarten** — nie kalt pitchen, immer auf Trigger reagieren.
2. **Nutzen als Mechanismus** — nie Feature-Liste, immer Wirkkette
   („mit den Leuten in Discord zocken → die kommen in deinen Stream → mehr Zuschauer").
3. **Consent vor Link** — „wenn du willst ich kann dir nen link schicken"; Link erst nach Ja.
4. **Anleitung statt Versprechen** — operativ erklären, was der Streamer gleich sieht/klickt.

Stil-Fingerabdruck (gemessen): Median 17–25 Zeichen, p90 ≈ 50, **0 % Emojis,
0 % Ausrufezeichen**, Tippfehler bleiben stehen, keine Gedankenstriche, lieber zwei
kurze Nachrichten als eine lange. Anti-Beispiel: perfektes Marketing-Deutsch mit
Gedankenstrich leuchtet zwischen echten Chat-Zeilen als Fremdkörper.

## Welle 1 — Stil-Härtung (bestehende Engagement-Pipeline)

- **Ein** Sanitizer-Vertrag auf allen Sende-Pfaden des Engagement-Layers
  (`tb-engagement`): Emojis strippen, Ausrufezeichen am Wort-/Satzende strippen
  (Command-Syntax wie `!clip` bleibt), Gedankenstriche (`—`/`–`) durch Komma/Space
  ersetzen, typografische Anführungszeichen begradigen, Nachrichten > 120 Zeichen
  **verwerfen** (nicht kürzen — abgeschnittene Sätze wirken kaputt).
- Off-profile Seed-Beispiele ersetzen (Emoji-Seeds raus, echte Zeilen rein).
- Vertragstests auf den Sanitizer + Anker-Test: kein Gold-/Seed-Beispiel enthält
  Emoji, `!` oder Gedankenstrich.

## Welle 2 — Scout

- **Kandidaten:** deutsche Deadlock-Streams (Quelle: Raid-Target-Resolution,
  tb-raid) minus Partner-Liste. Beobachtung über den anonymen IRC-Lurker
  (Wiring prüfen — „ported but never wired" ist im Repo eine bekannte Bug-Klasse).
- **Trigger v1** (jeder Trigger = ein Discord-Vorschlag):
  1. **Problem-Moment:** Streamer ärgert sich über Spam-/Scam-Bots, fehlende Raids,
     Mitspieler-Suche (LLM-Judge über Chat-Fenster).
  2. **Offline-/Raid-Moment:** Stream endet ohne Raid-Ziel → Auto-Raid-Feature-Pitch.
  3. **Neuer-Streamer-Radar:** erstmals gesehener deutscher DL-Streamer →
     nur Info-Eintrag, **kein Pitch-Text** (Kaltkontakt ist nie das Muster).
  - Bewusst NICHT drin: „Direkte Frage nach Community/Tools" — kommt in fremden
    Streams praktisch nicht vor.
- **Zustellung:** Post in den bestehenden Staff-/Radar-Discord-Channel:
  Streamer + Viewer-Zahl + letzte ~5 Chat-Zeilen + Trigger-Grund + fertiger
  Pitch-Text (Copy-Paste) + 👍/👎-Buttons.
- **Frequenz:** mehrere Vorschläge pro Stream erlaubt, wenn *verschiedene* Trigger
  feuern; pro Trigger-Typ max. 1× pro Stream. Erkanntes „Nein" des Streamers →
  permanente Blacklist (nur manuell aufhebbar).
- **Ledger (Pflicht):** JEDE Entscheidung wird geloggt — auch „Trigger erkannt,
  unterdrückt (Cooldown/Blacklist)" und Judge-Fehler/Timeouts. Nie nur Treffer melden.
  Log-Zeile: Eingabe (gekürzt), Urteil, Confidence, Trigger-Grund.
- **Pitch-Formulierung:** Few-Shot aus den 159 echten Pitches (kuratiertes
  Pitch-Register), Consent-Leiter-Regel im Prompt (erste Nachricht = Angebot ohne
  Link), Website als Primärziel, Discord optional. Finale Prompt-/Beispieltexte
  schreibt Claude/EarlySalty, nicht der Implementierungs-Worker.

## v1.1 — Feedback-Sync (Cross-Repo)

Der 10-Minuten-Sync in `scout_pitch_wiring.rs` konsumiert
`GET /internal/master/v1/discord/message-reactions` des Master-Brokers.
Diese Route ist im **Deadlock-Bots-Repo** implementiert und deployt
(Rust-Serving-Pfad `dl-broker`/`dl-discord`, main-Merge `7384d7bf`;
eine Verhaltens-Referenz liegt zusätzlich in `service/master_broker.py`).
Kontrakt: 200 `{"found": bool, "reactions": [{"emoji", "count"}]}`,
404→`found:false`, Discord-Fehler→502; IDs immer Strings.
`found=false`/Fehler lassen bestehendes Feedback unangetastet
(COALESCE im Ledger-Update).

## Welle 3 — Review-Betrieb

1–2 Wochen 👍/👎 sammeln; Shadow-Phase misst das echte Melde-Volumen (Judge-Regel:
Volumen messen, nicht schätzen). Danach Auswertung: Trefferquote, Volumen,
Streamer-Conversions → Entscheidung über v2 (z. B. One-Click-Send via
earlysalty-OAuth).

## Rohdaten

Analyse-Exporte (Session-Scratchpad, nicht im Repo): `chat_export.tsv`,
`pitch_earlysalty.txt`, `pitch_kubi_kubi_kubi.txt`, `sample_*.txt`.
