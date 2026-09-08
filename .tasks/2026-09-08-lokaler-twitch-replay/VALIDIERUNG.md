status: abgeschlossen
datum: 2026-09-08

# Prüfung des lokalen Replay

Die Tests verwenden ausschließlich synthetische Nachrichten. Echte Datensatzinhalte stehen weder in Git noch in Terminal- oder Reviewausgaben.

- Vor Implementierung war der Feature-Test rot: fehlender Bin und fehlendes `tb_llm::local_eval`.
- Vollständiger Feature-Test: 30 Unit-Tests, sieben Replay-Integrationstests und ein bestehender Identifier-Guard erfolgreich. Das bestehende ignorierte Doc-Beispiel bleibt ignoriert. Die Postgres-Fixtures überspringen sich ohne Test-DSN intern; das ist kein Live-DB-Nachweis.
- Abgedeckt sind lokale URL-/Modell-/Scopegrenzen, keine Authheader und Redirects, Referenz-/Metadatenunabhängigkeit, Zeit-/Wissens-/Autorenprüfung, private Dateirechte und Escaping, gültige Telemetrie bei verworfenem Text, vollständiger Fehlernenner, Vergleichsparameter und Abbruchbericht.
- Bestehender SQLx-0.9-Testfixture-Compilerfehler: innerhalb von ledger.rs mit geprüftem Testschemanamen und QueryBuilder behoben. Keine Produktionsabfrage geändert. Bestehender Clippyfehler im Identifier-Guard: unbenutzten rekursiven Parameter entfernt, Inhalt des Guards unverändert.
- Neue Dateien bestehen gezieltes rustfmt und `git diff --check`. Unbeteiligte historische Formatierungshunks in Hub und Guard bleiben unverändert.
- Finale Logdateien: `/tmp/twitch-replay-final-tests.log`, `/tmp/twitch-replay-final-clippy.log`, `/tmp/twitch-replay-final-default-check.log`.
- Verwendete Toolchain: `/home/nathanael/.cargo/bin/cargo`; zwei Buildjobs, ausschließlich `/home/nathanael/.local/share/twitch-local-eval/rust-target`.

Unabhängige Rust-/Sicherheitsreviews werden vom Root koordiniert. Eigen-Gate folgt gegen den eigenen Commit. Die echte Messung steht noch aus; technische Smoke-Tests aus der vorherigen Recherche sind keine Deutsch- oder Gesprächsfreigabe.

## Dauerhafter Abschlussnachweis für den Code

Geprüfter Code-Commit: `70e55a1e1a3c1edae6f171d03657cc296e504584`, nachgezogen auf `d1e35213`. Alle 38 integrierten Tests, all-targets Clippy und Default-aus-Check grün. Rust- und Sicherheitsreview geben diesen Stand frei. Die vorangegangenen offenen Formulierungen sind durch diesen datierten Nachweis ersetzt.

Alle endgültigen Reviewerberichte, integrierten Cargo-Logs und der Eigen-Gate-Log liegen dauerhaft mit 0600 unter `/home/nathanael/.local/share/twitch-local-eval/runtime/reviews/`; das Verzeichnis hat 0700. Dateinamen: `twitch-replay-rust-review.md`, `twitch-replay-security-review.md`, `twitch-replay-integrated-tests.log`, `twitch-replay-integrated-clippy.log`, `twitch-replay-integrated-default.log`, `twitch-replay-eigen-gate.log`.

Der normale Eigen-Gate liefert ALLOW, nennt aber ausdrücklich fehlende Repository-Werkzeuge. Ursache ist sein bestehender Sandbox-Aufruf, der diese Werkzeuge deaktiviert. Keine Gate-Regel wurde geändert. Die beiden unabhängigen vollständigen Codereviews und selbst geprüften Cargo-Nachweise decken diese Einschränkung ab. Das Gate allein wird nicht als vollständiger Nachweis bezeichnet.

Ausführbares Bin außerhalb des Worktrees: `/home/nathanael/.local/share/twitch-local-eval/rust-target/debug/tb-local-replay`, SHA-256 `63910dc2bae18cdbcb8a0d8eb63470e4033ec972c07637fdcc607dd5715133e6`. Die private Vergleichsansicht wird dauerhaft unter `results/qwen3.5-9b-20260908/comparison.html` derselben Versuchsablage erstellt. Bin, Modelle, Configs, Antworten und Ansichten bleiben nach Worktree-Bereinigung nutzbar.

Nach beiden vollständigen Läufen zusätzlich unverändert gesichert: `/home/nathanael/.local/share/twitch-local-eval/runtime/bin/tb-local-replay-63910dc2bae18cdbcb8a0d8eb63470e4033ec972c07637fdcc607dd5715133e6`, Modus 0500. Der SHA wurde erneut bestätigt. Dieser Inhaltspfad ersetzt den überschreibbaren Debug-Pfad als dauerhafte Referenz. Beide Manifeste nennen denselben Binärhash.

Finale Artefaktprüfung: jeweils 60 JSONL-Ergebnisse, alle 60 Prompt-Prüfsummen modellübergreifend identisch, 50 Fälle und 170 Karten im Gesamt-HTML, keine Script-Tags oder externen Assets, null Fehler. Private Ergebnisdateien sind 0600, Runordner 0700. Nachweis dauerhaft unter `runtime/artifact-validation.json`.

## Grenze der Laufzeitmessung

Die JSON-Serialisierung stellt den veränderlichen `previous_context` vor Wissen und Stilbeispiele. Deren Auswahl und Reihenfolge wechseln außerdem je Fall. Damit bleibt in dieser Anordnung im Wesentlichen die Systemkarte als gemeinsamer Präfix; beobachtet wurden etwa 228 Cachetokens. Die gemessene CPU-Latenz gilt für genau diese Vorgabe und Runtime, nicht als allgemeine Untergrenze lokaler Modelle. Statische Inhalte vor den veränderlichen Kontext zu stellen wäre eine spätere Optimierung; ihre Wirkung wurde hier nicht getestet. Beide laufenden Modellvergleiche behalten unveränderte Prompts und Parameter.

## Grenze der Stilauswahl

Die rein lokale zusätzliche Zählung unter `runtime/style-pool-metrics.json` zeigt für die 16 Stilvorlagen einen Median von 46 Zeichen und 8,5 durch Leerraum getrennten Wortgruppen. Die 50 historischen Referenzen haben 26 Zeichen und vier Wortgruppen im Median. Beide Gruppen enthalten keine Mehrzeiler. Die Verteilungen überlappen deutlich; das 75. Perzentil liegt bei 65 beziehungsweise 64,75 Zeichen. Das ist ein Hinweis auf eine mögliche Längenverzerrung des kleinen Vorlagenpools, kein Kausalnachweis. Die Häufigkeit, mit der einzelne Beispiele tatsächlich ausgewählt wurden, ist dabei nicht gewichtet. Ein späterer Vergleich sollte die Repräsentativität des Stilpools verbessern; die laufenden Daten bleiben eingefroren.
