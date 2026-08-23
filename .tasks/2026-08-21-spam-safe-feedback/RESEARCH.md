status: überholt
datum: 2026-08-21
klasse: mittel

## Ablösung

Der Nutzer hat den Auftrag nachträglich präzisiert: Der Button ist ein Gegen-Override
zur AI-Entscheidung. Die aktive Safe-Liste soll bei einer manuellen Korrektur auf
`harmlos` geschrieben und vor dem LLM exakt geprüft werden. Siehe den neuen Plan
`.tasks/2026-08-21-spam-safe-list-override/`.

## Auftrag

Ein AI-Urteil `harmlos` muss im Discord-Alert mit einer manuellen Safe-Feedback-Aktion bestätigt und dauerhaft im bestehenden Review-Log erfasst werden können.

## Beobachtungen (belegt, Datei:Zeile)

- `rust/crates/tb-chat/src/pipeline.rs:317-427` baut das `spam_learning`-Payload für den Changelog-Cog. Bei `AiReviewOutcome::Safe` wird bisher trotzdem `learn_pattern` mitgeschickt.
- `rust/crates/tb-chat/src/scam_pitch.rs:1544-1546` dokumentiert, dass ein Safe-Urteil selbst kein Muster lernt. Das bestehende `twitch_spam_review_decisions`-Log wird in `:1908-1930` für alle Judge-Ausgänge persistiert.
- `rust/crates/tb-internal-api/src/handlers/spam_learning.rs:87-151` lernt derzeit ausschließlich Spam-Muster. `:154-199` löscht ein vom Judge gelerntes Spam-Muster. Die DB-Constraint in `rust/migrations/20260712130000_spam_review_decisions.sql:10-16` erlaubt `clean` als Review-Verse.
- `/home/nathanael/repos/Deadlock-Bots/rust/crates/dl-changelog/src/lib.rs:214-252` erzeugt derzeit bei `verdict=safe` den Button `Als Spam korrigieren`.
- `/home/nathanael/repos/Deadlock-Bots/rust/crates/dl-bridges/src/twitch.rs:640-735` validiert die Discord-custom_id, prüft Moderationsrechte, ruft die Twitch-Internal-API auf und deaktiviert den Button nach Erfolg.
- `/home/nathanael/repos/Deadlock-Bots/docs/internal/spam-learning-buttons.md:1-53` beschreibt den bestehenden Cross-Repo-Vertrag und verlangt Rust-Tests für Parser, Button und API-Weitergabe.
- Eine Safe-Liste wird im aktiven `tb-chat`-Filter nicht geladen (`rust/crates/tb-chat/src/spam_filter.rs:827-900`). Safe-Feedback darf daher nicht als negatives Score-Signal oder Safe-Pattern gespeichert werden.

## Hypothesen (unbelegt)

- Der sichtbare Button stammt aus dem Rust-Changelog-Empfänger und nicht aus der Web-Dashboard-SPA. Das wird durch den direkten `spam_learning`-Payload und die Screenshot-Texte geprüft.

## Wahrscheinlich zu ändernde Dateien

- Twitch-Bot: `rust/crates/tb-chat/src/pipeline.rs`, `rust/crates/tb-internal-api/src/handlers/spam_learning.rs`, `rust/crates/tb-internal-api/src/lib.rs`.
- Discord-Bot: `rust/crates/dl-changelog/src/lib.rs`, `rust/crates/dl-bridges/src/twitch.rs`.
- Cross-Repo-Vertrag: `docs/internal/spam-learning-buttons.md`.

## Risiken / Seiteneffekte

- Der neue Klick ist eine interne DB-Schreibaktion. Er bleibt auf Moderations-/Adminrechte begrenzt und nutzt den bestehenden loopback-only Internal-API-Pfad.
- Kein Safe-Pattern wird in den aktiven Filter eingespeist. Das Feedback ist als `clean`-Beispiel im Review-Log sichtbar, damit spätere Prompt-/Gemini-Auswertung darauf zugreifen kann.
- Bereits gesendete alte `Als Spam korrigieren`-Buttons bleiben unverändert kompatibel; der neue Action-Prefix wird nur für neue Alerts erzeugt.

## Abschluss

- Der Screenshot-Fall erzeugt bei neuen Alerts einen Button `Als harmlos bestätigen`.
- Der Klick schreibt `clean` in `twitch_spam_review_decisions`; die aktive Spam-Filterung
  bleibt unverändert.
- Es gibt keinen Safe-Pattern-Insert und keinen direkten Gemini-Aufruf im Klickpfad.
