# Paketereignis M/1/004

Datum: 2026-10-08
Status: Ausgeliefert und geprüft, Ressourcenabschluss läuft

Fix über den eigenen sauberen Main-Clone mit HEAD:main veröffentlicht. Code- und Release-SHA: 3341098f3a953e231c0ec5731141996b10f1b9b6. Main unmittelbar vor Release-Build und Auslieferung frisch geholt und auf identischen SHA geprüft. Release-Build über cargo-slot mit drei Jobs: Exit 0, 21 Minuten 24 Sekunden. Acht erforderliche ELF-Dateien tragen exakt diesen SHA ohne dirty-Zusatz; die ausgelieferten Dateien stimmen per SHA-256 mit dem eigenen Build überein.

Integrierte Kontaktabnahme: Exit 0, 7 passed, 0 failed, 0 ignored. Die 18 Fälle im echten Pipeline-Aufrufer sowie zusätzliche Vertrauens- und Einstellungsausnahmen liefen erfolgreich. Unbekanntes Kontoalter ergibt Hint, zehn Tage ergibt PublicWarn, 200 Tage ergibt Hint. Keine Löschung, kein Timeout und kein Ban aus diesen Pipeline-Replays. Paket-Clippy: Exit 0 mit dem vorhandenen Borrow-Hinweis. Format: Exit 0. Die vorbestehenden 42 roten Baseline-Tests bleiben separat dokumentiert; keine grüne Gesamtsuite behauptet.

Lokaler Gate am endgültigen Veröffentlichungsstand mit gpt-6.1-sol: Exit 0, „ALLOW: No merge-blocking defect found in the supplied diff and revision snapshots.“ Die frühere BLOCK-Korrektur wurde durch den vorgeschriebenen frischen Fixer und denselben Reviewer abgeschlossen. Keine zusätzlichen Reviewer und kein Hook umgangen.

Deploy-Wrapper: Exit 0; vier Dienste neu gestartet. Bot-PID 1005470 zu 1830251, Dashboard 1005184 zu 1830151, Coaching-Watch 1007302 zu 1830765, Collector 1007515 zu 1830776. Lesender Wrapper bestätigt vier aktive Prozesse auf dem Release-SHA, NRestarts jeweils 0 und keine gelöschte exe. Journal seit 2026-10-08T06:01:54+02:00 mit -p err für diese Dienste leer. Der neue ASCII-Anker combo:praise_support_unsolicited_contact fehlt im vorherigen Bot-Binary und ist im ausgelieferten Binary vorhanden.

Funktionsnachweis: isolierte Replays des ausgelieferten Quellstands mit echten Rust-Erkennungsfunktionen und echtem Pipeline-Aufrufer. KI-Transport mit Wiremock, dessen Urteil ist ein Stub. Keine Live-Moderation gegen echte Konten, kein reales Modellurteil behauptet und keine Community-Daten für Tests an externe Modelle gesendet. Ort: Rust-Moderationspfad des laufenden Twitch-Bots; keine neue UI, Route oder Browserprüfung.

Frontend-Quellen einschließlich bot/dashboard_v2 sind gegenüber dem laufenden Vorrelease 51c8a674 unverändert. Die daraus übernommenen drei Bundles stimmen bytegenau mit dem Vorrelease überein. Herkunft basiert auf Commit-Vergleich, nicht Datei-Alter. Staging-Clone wurde vom Deploy-Wrapper nach /opt/deadlock/twitch/builds/3341098f3a953e231c0ec5731141996b10f1b9b6 eingefroren.

Baseline-Worktree und Baseline-Testcontainer sind entfernt. Eigener Fix-Worktree und verbleibender Testcontainer werden nach Sicherung dieser Belege entfernt. AUFTRAG.md und REGISTER.md bleiben unter Eigentum der Hauptsession. Der schmutzige geteilte Checkout wurde nicht verändert.

TESTNACHWEIS[TW-1]: 7 passed, 0 ignored | Baseline: 42 rot
LIVEBEWEIS[DV-1]: PID 1005470->1830251 | exe ohne (deleted) | journal -p err leer | Anker "combo:praise_support_unsolicited_contact" in Binary | Funktion: isolierter Pipeline-Replay erfolgreich, keine Live-Moderation | Ort: Twitch-Bot, Rust-Moderationspfad
WIRKUNGSPRUEFUNG[WP-1]: 1 Befund behoben | Zwillingssuche: Kontaktnamen aus Kontext-, Anfrage- und Pitch-Prüfungen ausgenommen und Replay geprüft | Fremddienst-Pfade: 1/1 geprüft, bestehender KI-Transport per Wiremock
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Task-Abschlussbelege

Die folgenden Abschlussänderungen sichern Dokumentation und Belege, nicht neuen Anwendungscode. Der nachgewiesene produktive Code bleibt Release 3341098f.
