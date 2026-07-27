# Smalltalk-Livetest — Design

Ziel ist nicht Vermarktung. Ziel ist herauszufinden, ob der Bot in einem
fremden deutschen Deadlock-Chat mitreden kann, ohne als Bot aufzufallen.
Erst wenn das trägt, hat ein Angebot überhaupt Wert.

Sprachlicher Rahmen: `2026-07-27-selbstvermarktung-stilvertrag.md`, Abschnitt
„Sprachliche Marker". Der Pitch-Teil des Stilvertrags gilt hier **nicht** —
in diesem Modus wird nie etwas angeboten.

## Ausgangslage

Die Smalltalk-Maschine existiert und ist in `chat_wiring.rs` verdrahtet:
`EngagementPipeline::handle()`, `OutputMode` (off/shadow/live) pro Kanal aus
`twitch_engagement_settings`, Versand über `StealthSender` mit dem separaten
Account aus `sender_auth::SENDER_LOGIN`. `persona`, `rhythm`, `style_examples`
und `conversation` sind vorhanden; Kanalprofile werden bereits laufend
gepflegt (`twitch_engagement_channel_profile`).

Alle Kanäle stehen derzeit auf `off`. Es fehlen: die Rotation, die
Auswertung und der Ausgabefilter.

## Was gebaut wird

### 1. Sitzungs-Loop

- Der Loop wählt **einen** fremden Kanal, der live Deadlock auf Deutsch
  streamt und kein Partner ist, und stellt ihn für 60 Minuten auf `live`.
- Global immer nur **eine** aktive Sitzung.
- Nach Ablauf: zurück auf `off`, Cooldown für diesen Kanal, nächster Kandidat.
- Streamende beendet die Sitzung sofort.
- Kill-Switch beendet sofort und setzt alle Kanäle auf `off`.
- Wird der Sender-Account in einem Kanal gebannt oder getimeoutet, endet die
  Sitzung sofort, der Kanal wird dauerhaft gesperrt und der Vorfall wird als
  Ergebnis festgehalten. Ein Bann ist ein Messergebnis, kein Fehler.

### 2. Ausgabefilter (vorhanden, wird verschärft)

`minimax_chat::sanitize_text` existiert bereits und entfernt Emojis, ersetzt
`—` durch „, ", `–` durch ein Leerzeichen, normalisiert typografische
Anführungszeichen und entfernt `!` außerhalb von Commands.

Zwei Ebenen kommen dazu, weil die Ersetzung nur das Zeichen trifft, nicht den
Satzbau:

Belegt an 4974 eigenen Nachrichten des Betreibers: **kein einziger**
Gedankenstrich. Über alle 127368 erfassten Chatnachrichten sind es 43, also
0,03 Prozent. Ein Gedankenstrich ist damit kein Stilfehler, sondern ein
Erkennungsmerkmal.

**Ebene 1 — der Prompt.** Der Engagement-Prompt in `minimax_chat.rs` enthält
selbst 28 Gedankenstriche („So tickst du — deine Persönlichkeit…"). Das Modell
lernt daraus den verschachtelten Satzbau; der Filter putzt danach nur das
Zeichen weg und lässt den Satz stehen. Für den Testmodus wird der Prompt
gedankenstrichfrei formuliert.

**Ebene 2 — der Filter.** Im Testmodus wird verworfen statt ersetzt:

- `—` und `–` führen zum **Verwerfen** der Nachricht. Ein Satzbau, der einen
  Gedankenstrich braucht, ist schon der falsche Satzbau für einen Twitch-Chat;
  ein Komma rettet den Rhythmus nicht.
- Ebenso verworfen: Anführungszeichen jeder Art, Aufzählungszeichen, mehr als
  ein Satzzeichen am Stück außer `...`, Nachrichten über 120 Zeichen.
- Ebenso verworfen: alles, was nach Angebot klingt (Links, `discord`,
  `community`, `partner`, `netzwerk`, `dashboard`, `website`). In diesem Modus
  gibt es keinen Pitch, auch nicht als Nebensatz.
- Die vorhandene Emoji- und `!`-Behandlung bleibt wie sie ist.
- Jede verworfene Nachricht wird mit Grund gespeichert. Verwerfen ist ein
  Ergebnis, kein stiller Ausfall.

### 2b. Redseligkeit

Der heutige Prompt schreibt „Dein Standard ist SCHWEIGEN, fast immer" und
schweigt ausdrücklich auch zu „Smalltalk und Geplänkel zwischen Zuschauern".
Das ist für den Partner-Betrieb richtig und bleibt dort unverändert.

Im Testmodus gilt stattdessen: mitreden wie ein normaler Zuschauer. Der Bot
darf auf Geplänkel reagieren, auf das Spielgeschehen und auf andere Chatter.
Unverändert bleiben die Regeln, die ihn nicht als Bot auffliegen lassen: er ist
Zuschauer und nicht Streamer, er erfindet keine Spielfakten, er kündigt keine
Wissenslücke an, und die Anti-Flood-/Anti-Burst-Grenzen aus `rhythm` gelten
weiter.

### 3. Sitzungsauswertung

Pro Sitzung wird gespeichert und im Review-Kanal `1374364800817303632`
ausgewiesen:

- Kanal, Dauer, Zuschauerzahl, ob der Bot angesprochen wurde.
- Jede erzeugte Nachricht: Text, Auslöser, Zeit, ob gesendet oder verworfen,
  bei Verwerfen der Grund.
- Jede Reaktion darauf: Antworten anderer Chatter innerhalb von zwei Minuten,
  Erwähnungen des Bot-Accounts, Timeouts und Banns.
- Am Ende eine Zusammenfassung: gesendet, verworfen je Grund, Antwortquote,
  Vorfälle.

Die Auswertung meldet **jede** Sitzung, auch die ohne eine einzige gesendete
Nachricht. Nur Treffer zu melden wäre ein blinder Fleck: Stille sähe dann wie
„kein Anlass" aus, wäre aber „Filter hat alles verworfen".

## Nicht-Ziele

- Kein Angebot, kein Link, keine Community-Erwähnung.
- Keine Änderung am Selbstvermarktungs-Shadow; der läuft getrennt weiter.
- Keine Änderung an den statischen Recruitment-Texten.
- Kein Senden über den Haupt-Bot-Account. Ausschließlich der Sender-Account.

## Konfiguration

- `SMALLTALK_LOOP_ENABLED` (Default aus) — Kill-Switch.
- Sitzungsdauer und Cooldown als Konstanten.

## Testvertrag

- Nie mehr als eine aktive Sitzung; Partner werden nie gewählt; Cooldown hält.
- Sitzungsende setzt den Kanal zuverlässig auf `off` zurück, auch bei Panik
  oder Prozessende.
- Der Filter verwirft Gedankenstrich, Halbgeviertstrich, Emoji, Anführungs-
  zeichen, Überlänge und jedes Angebotswort. Für jeden Fall ein Test.
- Ein Bann beendet die Sitzung und sperrt den Kanal dauerhaft.
- Jede erzeugte Nachricht erzeugt genau einen persistierten Datensatz,
  gesendet oder verworfen.
- Kein Codepfad sendet über den Haupt-Bot-Account.

## Betriebsnachweis

Nach Deploy: PID-Wechsel, `/proc/<pid>/exe` auf der neuen Binary, Journal ohne
`error|panic|fatal`, und eine vollständige Sitzungsauswertung im Review-Kanal
aus einer echten Stunde in einem fremden Kanal.
