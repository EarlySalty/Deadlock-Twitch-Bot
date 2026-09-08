status: aktiv
datum: 2026-09-08

# Übergabe vor Implementierung

## Abgeschlossener Versuch

Beide echten Läufe sind vollständig beendet: je 60 von 60 Antworten, keine Fehler, keine leeren oder abgeschnittenen Antworten, Exit 0. Der direkte private Gesamtvergleich und die Kennzahlen stehen in ERGEBNIS.md. Der nachfolgende Abschnitt hält frühere Zwischenstände fest und ist keine offene Arbeitsanweisung. Runtime-Abschluss und finaler Git-Merge werden mit Root koordiniert; es gibt keine offene Modell-/Coding-Freigaberückfrage.

## Aktueller Stand nach dem erneuten Nutzer-Go

Der folgende historische Übergabetext wird durch diesen Abschnitt und die letzten CONTRACT-Amendments ersetzt. Die begrenzte Codex-Umsetzung wurde vom Root freigegeben und ist gebaut. Es gibt keinen offenen Nutzer-Freigabeblocker. In dieser Runde werden ausschließlich lokale Qwen3.5-4B/9B-Modelle verglichen. Ein neuer Infisical-/Cloudpfad wurde nicht gebaut, DeepSeek bleibt unverändert produktiv.

Der standardmäßig ausgeschaltete Feature-Einstieg `tb-local-replay` liegt in `tb-llm`. Er prüft den eingefrorenen Datensatz vollständig vor Aufrufen, erstellt private JSONL-/HTML-/Markdown-Berichte und atomare Zwischenstände mit `complete=false`. Ungültige Modelltexte werden verworfen, bereits gelieferte Tokens und Abschlussgrund bleiben als Messwerte erhalten. Die zweite Runde kann die erste anhand Datensatz, Binärhash, Systemkarte, Parametern und einzelnen Prompt-Prüfsummen verifiziert direkt daneben darstellen. Kein Sender, Concierge oder produktiver Dienst ist angebunden.

Die beiden im unabhängigen Rustreview gefundenen Punkte (Telemetrieverlust und fehlender Abbruchstand) sind behoben und synthetisch getestet. Sicherheitsreview kontrolliert zusätzlich echten Vorlauf-Binärhash und lokale Vorlaufadresse. Alle neuen Rust-Dateien sind formatiert; zwei vorbestehende Testfixture-/Clippyfehler wurden minimal innerhalb von `tb-llm` behoben. Die genauen finalen Cargo-Logs werden in VALIDIERUNG.md benannt. Echte Modellläufe starten erst nach dem Root-Run-Go. Parameter: 50 Fälle plus zehn Baselines je Modell, 160 Tokens, Temperatur 0,4, 600 Sekunden Frist. Kein fester Sampling-Seed, deshalb keine bitgenau identischen Antworttexte bei Wiederholung versprechen.

## Historische Übergabe vor dem erneuten Go

ORCHESTRIERUNG[OR-1]: Klasse mittel | Phase plan | Artefakt: .tasks/2026-09-08-lokaler-twitch-replay

## Bereit

- Isolierter Worktree /home/nathanael/.worktrees/tb-lokaler-replay, Branch feat/lokaler-twitch-replay, Basis d55880ab804086a39271fbd0885c7e0c50fe4c96. Hauptcheckout unverändert.
- CONTRACT, RESEARCH, EVIDENCE, PLAN und ausführlicher synthetischer Implementierungsauftrag vorhanden. Kein Quellcode verändert, kein Build und kein Replay ausgeführt.
- Private Daten unter /home/nathanael/.local/share/twitch-local-eval/data/dataset.json. Schema-Aggregate: 50 Fälle, 24 mit Audio, alle mit mindestens zwei Stilbeispielen. Keine Rohdaten im Terminal oder Codingprompt.
- Runtimeagent bestätigt 4B an http://127.0.0.1:18789/v1/chat/completions, Alias qwen3.5-4b-local, Health OK. Nach 4B-Vergleich wechselt er sequenziell auf qwen3.5-9b-local. Server ohne Authsecret, Denken aus, 8192 Kontext, ein Slot. Smoke ist nur Transportnachweis, keine Sprachfreigabe.

## Belegter Blocker

- Claude-CLI mit exakt claude-opus-4-8, nur synthetischer „BEREIT“-Prompt: Exit 1, „You've hit your weekly limit · resets 10pm (Europe/Berlin)“.
- Bestehende Nutzer-Ausnahme für Antigravity bei einfacher/mittlerer Änderung an bestehendem Code vom Root geprüft. agy models erfolgreich, aber Ausführung gemini-3.8-flash-high: Exit 1, „Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 50h46m24s.“
- Zweite Googlefamilie gemini-3.1-pro-high mit reinem BEREIT-Prompt: gleicher Quota-Fehler, rund 50 Stunden Reset. Kein bloßer Fehler eines Modellnamens.
- Root hat gezielt um Nutzerfreigabe für Codex bei diesem Versuch gebeten. Diese Antwort steht aus. Keine abhängige Implementierung beginnen, bis sie vorliegt.

## Geplanter begrenzter Codeumfang

- rust/crates/tb-llm/Cargo.toml: default-aus Feature local-eval, erforderlicher Bin-Eintrag, ausschließlich notwendige schon im Workspace vorhandene Abhängigkeiten.
- rust/crates/tb-llm/src/hub.rs: bestehende Body-/Transport-/Parsinglogik so wiederverwenden, dass lokale Anfragen weder Authsecret, Proxy, Redirect noch Cloud-Fallback erhalten. Produktionsfreigabe unverändert.
- rust/crates/tb-llm/src/lib.rs: nur feature-gated Exporte.
- rust/crates/tb-llm/src/local_eval.rs: typisierter lokaler Ziel-/Modell-/Twitch-Scope und evalbezogene Datenvalidierung, Prompt und private Berichte, gegebenenfalls in klar getrennte Unterdateien.
- rust/crates/tb-llm/src/bin/tb-local-replay.rs: kleiner Config-/Ausführungseinstieg; kein zweiter HTTP-Client außerhalb des Hubs.
- rust/crates/tb-llm/tests/local_eval.rs oder Modul-Tests: synthetische Sicherheits-/Leckage-/Privatsphäretests. Keine Änderung bestehender Tests oder anderer Crates.
- rust/Cargo.lock nur falls notwendige Metadaten aktualisiert werden; .tasks-Dateien mit Ergebnis fortschreiben.

Der Aufwand ist ein kleiner Connector-Zusatz plus dateibasierter Runner und Berichterzeugung. Eine genaue Zeilenzahl ist vor dem Bauen kein belastbarer Messwert. Nicht produktive Onboarding-/Sender-/Concierge-Logik anbinden.

## Als Nächstes

Nach ausdrücklicher erlaubter Modellverfügbarkeit den Auftrag in IMPLEMENTIERUNG.md ausführen. Zuerst neue sinnvolle Tests, dann Implementierung, zielgenaue Cargo-Prüfungen. Danach Root um unabhängigen Rust-/Securityreview bitten; reales Dataset ausschließlich mit dem gebauten lokalen Bin lesen. Kein Rohchatinhalt an Reviewer oder Coding-CLI. Vollständige Messung, private Vergleichsansicht und dokumentierte Grenzen vor Abschluss. Eigenreview gate_hook.py --review gegen eigenen Commit erst nach tatsächlichem Codebau, nicht gegen reine Planung als Scheinfreigabe.

## Aktualisierung vom 08.09.2026, Stand nach 17:44 Uhr CEST

Die Angaben dieses Abschnitts ersetzen die früheren Zwischenstände zu Runtime und Datenumfang unter „Bereit“. Die historischen Angaben bleiben zur Nachvollziehbarkeit erhalten.

- Qwen3.5-4B und Qwen3.5-9B liegen als Q4_K_M-GGUF unter `/home/nathanael/.local/share/twitch-local-eval/models/`; die offizielle llama.cpp-Binärdistribution und Herkunftsbelege liegen unter `runtime/` derselben Wurzel.
- Beide Modelle wurden ausschließlich synthetisch geprüft. 4B: 6,20 Sekunden, 63 Eingabe- und 43 Ausgabetokens, 12,09 generierte Tokens/s, 4,32 GiB RSS. 9B: 13,04 Sekunden, 63 Eingabe- und 93 Ausgabetokens, 9,55 generierte Tokens/s, 8,35 GiB RSS. Wegen unterschiedlicher Antwortlängen sind die Gesamtzeiten kein direkter Geschwindigkeitsvergleich. Beide Einzelantworten waren inhaltlich fehlerhaft. Das belegt Ausführbarkeit, keine ausreichende Deutsch- oder Gesprächsqualität.
- Die temporären System-Units liefen als `nathanael`. CPU-Limit 400 %, Arbeitsspeicherlimit 12 GiB und Adressraumlimit 16 GiB wurden auf Kernel-Ebene nachgewiesen. Beide Test-Units sind seit 17:44 Uhr CEST gestoppt, Port 18789 ist frei. Produktionsbot und Dashboard sind aktiv und unverändert.
- Der eingefrorene Datensatz enthält 50 echte Fälle aus 18 Kanälen über 14 Tage: 49 mit Chatkontext, 29 mit Audio, acht mit zeitlich belegtem Botwissen. Stilmaterial: 16 ältere Beispiele aus drei getrennten Kanälen. Die frühere Angabe „24 mit Audio“ ist überholt. Alle 18 Datenprüfungen sowie der unabhängige SQL-Review bestanden.
- Wurzel und Datenverzeichnis sind auf 0700, private Dateien auf 0600 begrenzt. SHA-256 von `data/dataset.json`: `377a48fe79f1f65d7aed97f523b9fbb9a6be2dbd353ff37782c4650377dbb558`. Keine Rohdaten in Arbeitsdokumentation, Terminal oder Codingprompt übernehmen.
- Ein Replay mit echten Daten wurde noch nicht ausgeführt. Es wurde kein Rust-Code gebaut. Der Opus-4.8-Wochenreset ist für 22 Uhr angekündigt; beide geprüften Google-Modellfamilien melden etwa 50 Stunden bis zur Freigabe. Eine Antwort auf die gezielt angefragte Codex-Ausnahme liegt noch nicht vor.
- Der Nutzer hat anschließend kostenlose Modellrouter angesprochen. Die laufende Recherche ist keine Freigabe, den privaten Datensatz extern zu übertragen oder die offene Codingmodell-Freigabe zu übergehen. Ein möglicher Router darf auch nicht stillschweigend zum Produktionsbackend werden.
- Nanis Stil gilt ausschließlich für Twitch; der Concierge erhält weder sein Stilprofil noch seine Beispiele. Kein Training, kein Nachrichtenversand und kein Produktionsmodellwechsel fanden statt.

Belege: [Runtimebericht](/home/nathanael/.local/share/twitch-local-eval/runtime/README.md), [technische Aggregate](/home/nathanael/.local/share/twitch-local-eval/runtime/technical-summary.json), [4B-Herkunft](/home/nathanael/.local/share/twitch-local-eval/runtime/4b-source-manifest.json), [9B-Herkunft](/home/nathanael/.local/share/twitch-local-eval/runtime/9b-source-manifest.json), [Datenmanifest](/home/nathanael/.local/share/twitch-local-eval/data/manifest.json), [Datenvalidierung](/home/nathanael/.local/share/twitch-local-eval/data/validation.json), [privater unabhängiger Datenreview](/tmp/twitch-local-data-review.md).

Fortsetzung: Nach tatsächlich verfügbarer freigegebener Codingstrecke den vorhandenen Implementierungsauftrag ausführen, den isolierten Replay bauen und erst danach echte Fälle ausschließlich lokal messen. Dieser Worktree bleibt für die unfertige Arbeit erhalten; diese Dokumentationsaktualisierung wird nicht separat gemergt, gepusht oder deployt.
