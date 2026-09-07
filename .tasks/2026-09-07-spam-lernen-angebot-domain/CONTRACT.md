# Contract: Spam-Lernen für Angebot-plus-Domain und generische Einzelwörter

status: aktiv
datum: 2026-09-07
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Zwei Fehlverhalten des Spam-Filters aus dem Live-Betrieb vom 05. bis 07.09.2026:

1. Der Scam-Richter hat am 05.09. das Einzelwort `discord` als Spam-Fragment gelernt. Seitdem löscht der Regelpfad jede Nachricht mit "discord" als hartes Signal, auch den Link zum eigenen Community-Discord und den Satz "bei Discord erlauben links zu öffnen" des Betreibers in einem Partnerkanal. Ein generisches Einzelwort darf nie allein eine Löschung auslösen.
2. Das Angebot "Ai viewers twitch .ad (no space)" fällt sechsmal in 30 Tagen an, wird jedes Mal nur vom KI-Richter erkannt (24h Timeout) und nie gelernt, weil das Distinktivitäts-Gate "twitch" als generisch und "ad" als zu kurz verwirft. Solche Angebot-plus-Domain-Muster müssen gelernt werden und danach wie "streamboo . com" über den Regelpfad laufen: gelerntes Muster plus Kontoalter plus Erstnachricht ergibt den Ban, ohne KI-Aufruf.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Angebot-plus-Domain wird gelernt: Ein vom Richter oder Menschen vorgeschlagenes Muster gilt als distinktiv, wenn es mindestens drei Tokens hat, eines davon domain-förmig ist (Label plus TLD, auch mit Trennversuch wie "twitch .ad" oder "streamboo . com", Zusätze wie "(no space)" oder "(remove the space)" werden vor der Prüfung entfernt) und ein weiteres Token ein Angebotswort ist (mindestens: viewer, viewers, follower, followers, sub, subs, promotion, boost, growth). Das Muster wird als `phrase` gespeichert, die Domain darin in der Schreibweise ohne Leerzeichen vor dem Punkt.
- REQ-02 Ganzphrasen-Treffer: Ein nach REQ-01 gelerntes Muster trifft eine Nachricht nur, wenn die ganze Phrase enthalten ist (Klartext oder Kompaktform). "die twitch ads nerven", "twitch ad break" und "twitch.ad" allein ergeben mit dem gelernten Muster "ai viewers twitch.ad" Score 0.
- REQ-03 Regelpfad statt KI: Die Nachricht "Ai viewers twitch .ad (no space)" von einem Konto unter 90 Tagen als Erstnachricht erreicht mit dem gelernten Muster `SPAM_MIN_MATCHES` und wird per Regelpfad gebannt (`SpamAction::Ban`), ohne Richter-Aufruf. Ohne Kontoalter und Erstnachricht bleibt es beim gelernten Muster als hartem Signal (Löschung, Richter entscheidet Ban).
- REQ-04 Plattformnamen sind generisch: `discord`, `telegram`, `instagram`, `youtube`, `tiktok`, `whatsapp`, `steam`, `kick`, `snapchat`, `twitter` stehen in `GENERIC_PATTERN_TOKENS`. Ein solches Einzelwort wird weder vom Richter noch per Mod-Korrektur gelernt, und ein Altbestand damit wird beim Laden verworfen (Warn-Log), so dass eine Nachricht wie "bei Discord erlauben links zu öffnen" oder ein `discord.com/channels/...`-Link keinen `Learned-`-Grund mehr erzeugt.
- REQ-05 Review-Karte: Bei einem nach REQ-01 gelernten Muster zeigt die Discord-Karte die Zeile "Gelernt" mit dem gespeicherten Muster; die Zeile "Nicht gelernt ... (zu generisch, Distinktivitäts-Gate)" erscheint dafür nicht mehr.

## Invarianten (darf sich nicht ändern)

- INV-01: `SPAM_MIN_MATCHES`, die Eskalatoren (Kontoalter unter 90 Tagen, Erstnachricht) und ihre Bedingung "nur unter der Schwelle und nur bei hartem Signal" bleiben wie sie sind.
- INV-02: Domain-Muster mit distinktivem Label ("eballo.com", "streamboo.com", "clicknex.online") und Dienstwörter ("streamboo", "peakpy") werden weiterhin gelernt; "best viewers", "hello viewers", "viewer.com", "view.ers", "ai viewers" (ohne Domain) und "cheap viewers and followers available" weiterhin nicht.
- INV-03: Die Phrasen-Ausnahme für Menschen (`is_distinctive_spam_pattern_vom_menschen`, ab 5 Wörtern und 30 Zeichen) bleibt unverändert und gilt weiterhin nicht für den Richter.
- INV-04: Broadcaster, Mods und `safe_list` bleiben von jeder Auto-Moderation ausgenommen; das Safe-Wording-Gate und `LEARN_MIN_CONFIDENCE` bleiben unverändert.
- INV-05: Kein neues LLM, kein neuer Richter-Aufruf, keine Änderung am Prompt des Richters.
- INV-06: Bestehende Tests nicht löschen oder abschwächen; die Regressionstests aus der Testphase ändert der Implementierer nicht.
- INV-07: Keine Migration, keine neue Tabelle, keine ENV-Config.

## Nicht-Ziele

- Kein globaler Vertrauenswert je Twitch-User-ID (eigener Contract).
- Keine Änderung an Timeout-Dauer oder Ban-Logik des Richter-Pfads.
- Kein Erkennen des eigenen Community-Discord-Links als Sonderfall (mit REQ-04 ist er kein Signal mehr).
- Keine Änderung an `sus_invite.rs` oder am Global-Ban-Sweeper.

## Erlaubter Änderungsbereich

- rust/crates/tb-chat/src/spam_filter.rs
- rust/crates/tb-chat/src/scam_pitch.rs
- rust/crates/tb-chat/src/pipeline.rs
- rust/crates/tb-chat/tests/
- rust/crates/tb-internal-api/src/handlers/spam_learning.rs
- .tasks/2026-09-07-spam-lernen-angebot-domain/

## Amendments

- 2026-09-07 A1 zu REQ-03: Erreicht die Nachricht die Schwelle `SPAM_MIN_MATCHES` auch ohne Eskalatoren (zum Beispiel gelernte Phrase +2 plus Viewer-Muster +1), ist der Regel-Ban gewollt. Das Viewer-Muster darf nicht unterdrückt werden, wenn eine gelernte Phrase greift; die Scoring-Reihenfolge und die Additivität der Regeln bleiben wie vor diesem Contract. "Ohne Kontoalter und Erstnachricht nur Löschung" gilt nur, wenn die Summe der Regeln unter der Schwelle bleibt.
