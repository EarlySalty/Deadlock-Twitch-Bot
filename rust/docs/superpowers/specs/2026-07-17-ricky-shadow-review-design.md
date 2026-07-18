# Ricky-Shadow-Review — Design

**Stand:** 17.07.2026
**Status:** vom Betreiber freigegeben
**Scope:** ausschließlich Schattenbetrieb; keine Twitch-Nachricht wird gesendet

## Ziel

Wenn der bekannte Twitch-Account mit der stabilen User-ID `147713656` in
einem überwachten Kanal schreibt, startet der Rust-Bot automatisch eine
zeitlich begrenzte Prüfsitzung. Während dieser Sitzung werden Rickys
Chatnachrichten, die Sprache des Streamers als Text, alle Modellentscheidungen
und die erzeugten Antwortentwürfe nachvollziehbar gespeichert und zusätzlich
in den internen Discord-Review-Kanal `1374364800817303632` gespiegelt.

Der Schattenbetrieb soll zeigen, ob ein späterer, ausdrücklich separat
freizugebender Twitch-Dialog natürlich, sachlich und ausschließlich auf
belegten Informationen basiert.

## Nicht-Ziele

- Kein automatisches oder manuelles Senden an Twitch in dieser Phase.
- Kein Erkennen anhand von Anzeigenamen, Schreibstil oder vermuteten Alt-Accounts.
- Keine Persönlichkeitsdiagnosen, politischen Etiketten oder erfundenen Motive.
- Keine Behauptung, der sendende Account sei persönlicher Augenzeuge.
- Kein Speichern des Roh-Audios.
- Kein Training oder Fine-Tuning mit den Review-Daten.
- Kein neuer allgemeiner LLM-Provider-Layer; vorhandene OpenAI-kompatible
  HTTP-Strukturen werden wiederverwendet.

## Freigegebene Faktenbasis

Das Modell erhält nur Fakten mit stabilen IDs. Eine Antwort muss die verwendeten
IDs zurückgeben; Text mit nicht freigegebenen Tatsachenbehauptungen wird
verworfen.

| Fakten-ID | Freigegebener Inhalt | Belegbasis |
|---|---|---|
| `community_ban_2026_05_29` | Ricky wurde aus der Deutschen Deadlock Community entfernt. | Discord-Mitteilung des Betreibers vom 29.05.2026 |
| `racist_greeting_report` | Als Bann-Grund wurde unter anderem eine rassistische Begrüßung mit dem N-Wort genannt. | dieselbe Discord-Mitteilung |
| `cs2_cheat_stream` | Als weiterer Grund wurde genannt, dass er CS2-Cheating selbst gestreamt und gerechtfertigt habe. | Discord-Mitteilung und Betreiberbeobachtung |
| `post_ban_discord_recruitment` | Nach dem Bann entstand ein eigener Discord; anschließend wurden Personen aus der Community und weitere Kontakte dafür angeworben. | Discord-Mitteilung und dokumentierte Kontakte |
| `twitch_pitch_history` | In der Twitch-Datenbank liegen kanalübergreifende Nachrichten vor, in denen der Account einen Deadlock-Community-Discord anbietet oder nach Interesse fragt. | `twitch_chat_messages`, exakte Twitch-User-ID |

Die Faktenbasis beschreibt beobachtete Ereignisse, nicht Charakter, Absicht oder
psychischen Zustand. Die zeitliche Folge „Bann, eigener Discord, Abwerbung" darf
genannt werden; daraus darf keine innere Motivation abgeleitet werden.

## Sprachvertrag

- Deutsch, natürlich, kurz bis mittellang, maximal 450 Zeichen pro Entwurf.
- Direkte Warnung aus dritter Person; kein amtlicher oder juristischer Ton.
- Formulierungen wie „nach dem, was ich dazu mitbekommen habe" sind erlaubt,
  weil sie den Informationsstand beschreiben.
- Verboten sind Formulierungen, die persönliche Anwesenheit, eigene Beobachtung
  vor Ort oder eine menschliche Identität behaupten.
- Das N-Wort wird ausschließlich als „N-Wort" bezeichnet.
- Das Modell beantwortet konkrete Rückfragen mit den dazu passenden Fakten,
  statt bei jeder Antwort die ganze Vorgeschichte zu wiederholen.
- Gibt der Kontext keine belegte passende Antwort her, lautet die Aktion
  `silent`; es wird nichts ergänzt oder geraten.
- Beispiel für späteren Twitch-Text: `Platzhalter`.

## Trigger und Sitzungszustand

1. Nur eine eingehende Chatnachricht mit `chatter_id = 147713656` startet eine
   Sitzung. Login und Anzeigename sind keine Identitätsbelege.
2. Pro Twitch-Kanal kann höchstens eine aktive Sitzung existieren.
3. Eine Sitzung läuft zehn Minuten ab dem letzten Ricky-Bezug.
4. Folgende Ereignisse setzen die zehn Minuten zurück:
   - eine weitere Nachricht der exakten Twitch-ID,
   - eine eindeutige Ricky-Erwähnung im Streamer-Transkript,
   - eine Modellentscheidung `topic_active = true` auf Basis des neuen Kontexts.
5. Streamende, Prozess-Shutdown oder der Kill-Switch beenden die Sitzung sofort.
6. Schreibt Ricky nach Ablauf erneut, beginnt eine neue Sitzung.

## Audio und Transkription

- Der Bot öffnet den öffentlichen Stream-Audiopfad erst nach dem ID-Trigger.
- Audio wird in kurzen Segmenten verarbeitet und nur im Arbeitsspeicher gehalten.
- Das vorhandene `yt-dlp` löst den öffentlichen HLS-Pfad auf; `ffmpeg` schreibt
  das kurze 16-kHz-Mono-Segment ausschließlich nach stdout in einen
  Arbeitsspeicher-Puffer. Weder Quellsegment noch WAV-Datei landen im
  Dateisystem.
- Die Transkription erfolgt über `POST /v1/audio/transcriptions` mit
  `model=whisper-1` und deutscher Sprachvorgabe.
- Ein Segment wird nach erfolgreicher oder fehlgeschlagener Übertragung sofort
  aus dem Arbeitsspeicher verworfen.
- Leere beziehungsweise reine Musik-/Geräuschsegmente erzeugen keinen
  Transkript-Datensatz.
- Es gibt keine Realtime-API-Verbindung und keine lokale Audiodatei.

## Textmodell

- Provider: Fireworks AI.
- API: OpenAI-kompatible Chat Completions unter
  `https://api.fireworks.ai/inference/v1/chat/completions`.
- Modell: `accounts/fireworks/models/deepseek-v4-flash`.
- Fireworks wird ohne Daten-Logging-Opt-in verwendet. Die Responses API wird
  nicht benutzt.
- Eingabe: freigegebene Faktenbasis, neue Ricky-Nachrichten, neue
  Streamer-Transkripte, bisherige Modellantworten derselben Sitzung und
  Sitzungsstatus.
- Ausgabe ist validierbares JSON mit:
  - `action`: `silent`, `initial_warning` oder `reply`,
  - `topic_active`: Boolean,
  - `confidence`: Zahl von `0.0` bis `1.0`,
  - `used_fact_ids`: Liste ausschließlich freigegebener Fakten-IDs,
  - `reason`: kurze interne Begründung,
  - `draft`: Antwortentwurf oder `null`.
- Ein Parserfehler, eine unbekannte Fakten-ID, fehlende Pflichtfelder oder ein
  unzulässiger Text führen zu `provider_error` und niemals zu einem Entwurf.
- Es gibt keinen Provider-Fallback. Bei OpenAI- oder Fireworks-Fehler bleibt das
  System still und dokumentiert den Fehler.

## Review-Datenbank

Eine neue, eigenständige Tabelle `twitch_crew_review_events` enthält alle Daten
dieses Features. Bestehende Chat-, Radar- oder Transkripttabellen werden weder
als Review-Speicher missbraucht noch in ihrer Aufbewahrung verändert.

Kernfelder:

- `id BIGSERIAL PRIMARY KEY`
- `review_session_id UUID NOT NULL`
- `channel_login TEXT NOT NULL`
- `subject_twitch_user_id TEXT NOT NULL`
- `event_kind TEXT NOT NULL` mit Check-Constraint für
  `session_started`, `ricky_message`, `streamer_transcript`, `ai_decision`,
  `ai_draft`, `provider_error`, `session_ended`
- `source_message_id TEXT NULL`
- `occurred_at TIMESTAMPTZ NOT NULL`
- `content TEXT NULL`
- `metadata JSONB NOT NULL DEFAULT '{}'`
- `provider TEXT NULL`
- `model TEXT NULL`
- `confidence DOUBLE PRECISION NULL`
- `discord_channel_id BIGINT NULL`
- `discord_message_id TEXT NULL`
- `discord_deleted_at TIMESTAMPTZ NULL`
- `last_delete_error TEXT NULL`
- `tombstoned_at TIMESTAMPTZ NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`
- `expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '6 months')`

Die Discord-Kanal-ID kommt über eine additive Folgemigration. Bereits vorhandene
Review-Zeilen mit Discord-Nachrichten-ID werden dabei auf den festgelegten
Review-Kanal zurückgefüllt; anschließend erzwingen Constraints, dass Kanal- und
Nachrichten-ID nur gemeinsam gesetzt sind und die Kanal-ID positiv ist.

Indizes liegen auf `(review_session_id, occurred_at)`,
`(channel_login, occurred_at DESC)` und `expires_at`.
Eine partielle Unique-Constraint auf
`(subject_twitch_user_id, source_message_id)` für nichtleere Nachrichten-IDs
verhindert, dass dieselbe Twitch-Nachricht über EventSub und Scout-IRC zwei
Sitzungen beziehungsweise zwei Review-Zyklen auslöst.

`metadata` enthält nur strukturierte Review-Daten wie Fakten-IDs, Aktion,
Latenz, Tokenzählung und Fehlercode. Secrets, Header, Rohantworten des Providers
und Roh-Audio sind verboten.

## Sechsmonatige Löschung

- Cleanup läuft beim Botstart und danach einmal täglich.
- Für abgelaufene Datensätze werden zuerst alle unterschiedlichen
  Paare aus `discord_channel_id` und `discord_message_id` einzeln aus Discord
  gelöscht. Der Broker übersetzt eine auf Discord fehlende Nachricht in einen
  erfolgreichen idempotenten Antwortstatus; eine HTTP-`404` des Broker-Endpunkts
  bleibt dagegen ein Löschfehler mit Retry. Die beim Versand gespeicherte
  Kanal-ID bleibt maßgeblich, auch wenn sich die Runtime-Konfiguration später
  ändert.
- Der Twitch-Bot erhält dafür keinen Discord-Token. Er ruft den authentifizierten
  Master-Broker-Endpunkt
  `/internal/master/v1/discord/delete-message` auf; der Broker führt die
  eigentliche Discord-Löschung aus und behandelt eine fehlende Nachricht
  idempotent.
- Danach wird der DB-Datensatz gelöscht.
- Ist Discord vorübergehend nicht erreichbar, werden `content`, `metadata`,
  Provider-/Modellfelder und Confidence fristgerecht geleert. Nur technische
  Identifikatoren, Ereignisart und -zeit, Ablaufzeit sowie der letzte
  Löschfehler bleiben als Tombstone für den nächsten Retry erhalten.
- Nach erfolgreichem Retry wird auch der Tombstone gelöscht.
- Der Cleanup löscht niemals Einträge aus `twitch_chat_messages` oder anderen
  bestehenden Tabellen.

## Discord-Review

- Zielkanal: `1374364800817303632` im Guild `1289721245281292288`.
- Versand und spätere Einzellöschung laufen ausschließlich über den bestehenden
  `BrokerRelay` zum Master-Broker. Dadurch bleibt die Discord-Berechtigung an
  einer Prozessgrenze und wird nicht in den Twitch-Bot kopiert.
- Pro Modellzyklus wird eine kompakte Components-V2-Karte mit Gold-Akzent
  `0xC8A86B` erzeugt.
- Die Karte enthält Session-ID, Kanal, Zeit, neue Ricky-Nachrichten, neue
  Streamer-Transkripte, Modellentscheidung, Fakten-IDs, Confidence, Entwurf
  beziehungsweise Fehler.
- `allowed_mentions.parse` ist leer; gespeicherter Text darf keine Erwähnung
  auslösen.
- Lange Inhalte werden deterministisch in mehrere Karten geteilt. Jede erzeugte
  Discord-Nachrichten-ID und ihr Ursprungskanal werden den enthaltenen
  Review-Ereignissen zugeordnet.
- Ein einzelner langer Transkripttext wird bereits beim Speichern an
  Wortgrenzen in fortlaufend nummerierte `streamer_transcript`-Ereignisse
  geteilt. Dadurch gehört jedes Ereignis genau zu einer Discord-Karte und die
  einzelne `discord_message_id` bleibt eindeutig.
- Fehlgeschlagene Posts bleiben mit `discord_message_id = NULL` in der DB und
  werden durch einen begrenzten Hintergrund-Retry erneut versucht. Seltene
  Duplikate nach einem Crash zwischen Discord-POST und DB-Update sind im
  internen Review-Kanal akzeptabel; Datenverlust ist es nicht.
- Im Systemjournal stehen nur Session-ID, Event-IDs, Status und Fehlerklasse,
  niemals die Inhaltsfelder.

## Schatten-Ausgabe und spätere Live-Phase

Der implementierte Modus erzeugt und prüft Entwürfe, sendet aber keine
Twitch-Nachricht. Das vorhandene dedizierte Twitch-Senderkonto wird in dieser
Phase nicht aufgerufen.

Eine spätere Live-Freigabe ist ein eigener Arbeitsschritt. Dafür gelten bereits
die freigegebenen Produktregeln:

- höchstens eine unaufgeforderte Warnung pro Kanal und laufendem Stream,
- danach nur thematisch passende Antworten auf Streamer-Aussagen oder -Fragen,
- derselbe Fakten-, Sprach- und Validierungsvertrag,
- ein sofortiger Kill-Switch.

## Konfiguration und Secrets

- `RICKY_SHADOW_REVIEW_ENABLED` ist standardmäßig `false` bis zum kontrollierten
  Deploy; danach wird nur der Schattenmodus aktiviert.
- Twitch-User-ID, Discord-Kanal-ID, Modellnamen, zehn Minuten Inaktivität und
  sechs Monate Aufbewahrung sind feste Produktkonstanten, keine unnötige
  Laufzeitkonfiguration.
- Secrets werden ausschließlich über Infisical in die Service-Umgebung geladen:
  `OPENAI_API_KEY`, `FIREWORKS_API_KEY` beziehungsweise der vorhandene
  kompatible Vault-Name und `DISCORD_TOKEN`.
- Secretwerte erscheinen weder in Dateien noch Logs noch Test-Fixtures.

## Fehlerverhalten

- Audioquelle nicht verfügbar: `provider_error`, Sitzung bleibt bis zum Timeout
  aktiv und versucht das nächste Segment erneut.
- OpenAI-Timeout/Fehler: Fehlerereignis, kein Fireworks-Aufruf für dieses Segment.
- Fireworks-Timeout/Fehler/ungültiges JSON: Fehlerereignis, kein Entwurf.
- DB-Fehler: kein Discord-Post für den betroffenen Zyklus; der Bot läuft weiter.
- Discord-Fehler: DB bleibt Source of Truth und der Post wird begrenzt erneut
  versucht.
- Jeder Pfad, auch `silent`, Parserfehler und Timeout, erzeugt eine
  nachvollziehbare Modellentscheidung oder ein Fehlerereignis.

## Testvertrag

Mindestens folgende TDD-Verträge werden automatisiert belegt:

1. Nur die exakte Twitch-ID startet eine Sitzung.
2. Pro Kanal entsteht nur eine aktive Sitzung; andere Kanäle sind unabhängig.
3. Ricky-Bezug verlängert, zehn Minuten Inaktivität und Streamende beenden.
4. Roh-Audio wird nach jedem Segment verworfen und nie persistiert.
5. Whisper-Request enthält Endpunkt, Modell und Sprache ohne Secret-Leak.
6. Fireworks-Request nutzt Chat Completions und exakt V4 Flash.
7. Gültige strukturierte Modellantworten werden akzeptiert.
8. Unbekannte Fakten-IDs, ungültiges JSON und unzulässige Behauptungen werden
   verworfen.
9. Alle Ereignisarten landen geordnet in der neuen Tabelle.
10. `expires_at` liegt sechs Kalendermonate nach Erstellung.
11. Cleanup löscht Discord zuerst und anschließend die DB-Zeilen.
12. Discord-Ausfall schwärzt abgelaufene Inhalte und lässt einen retrybaren
    Tombstone zurück.
13. Discord-Karten deaktivieren Mentions und werden bei Grenzwerten geteilt.
14. Kein Codepfad ruft im Schattenmodus den Twitch-Sender auf.
15. Providerfehler lösen keinen Fallback aus.

Zusätzlich wird der vorhandene redigierte Ricky-Evaluationskorpus genutzt, um
die Faktenbindung und das Schweigen bei irrelevanten Eingaben zu prüfen.

## Betriebsnachweis

Nach Merge und Release werden belegt:

1. Migration und Tabellenindizes existieren live.
2. Der Twitch-Bot startet mit neuer PID aus der frisch gebauten Release-Binary.
3. Journal seit Neustart enthält kein `error|panic|fatal` und keine Review-Inhalte.
4. Ein kontrollierter Schatten-Test mit der exakten ID erzeugt DB-Ereignisse und
   eine gerenderte Components-V2-Karte im Zielkanal.
5. Im Test wird keine Twitch-Nachricht durch das dedizierte Konto gesendet.
6. Ein künstlich abgelaufener Testdatensatz beweist Cleanup und Discord-Löschung,
   ohne Produktionsdaten zu verändern.
