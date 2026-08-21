status: erledigt
datum: 2026-08-21
klasse: mittel

## Befund

0 bestätigte Befunde.

1. `rust/crates/tb-chat/src/pipeline.rs:1201`: Der manuelle Safe-Volltext wird
   vor Score, Mention-Auflösung, Moderationsaktion und Spam-Judge geprüft. Ein
   Treffer wird als `SAFE_PATTERN` geloggt und beendet den Spam-Pfad.
2. `rust/crates/tb-internal-api/src/handlers/spam_learning.rs:317`: Der
   Korrektur-Button liest die stabile Spam-Row-ID, sperrt die Row, schreibt den
   kanonisierten Originaltext als manuelles Safe-Pattern, auditert `clean` und
   entfernt die Spam-Row in einer Transaktion.
3. `rust/crates/dl-changelog/src/lib.rs:233` und
   `rust/crates/dl-bridges/src/twitch.rs:735`: AI `spam` führt über `correct` in
   die Safe-Korrektur; AI `safe`, Fehler oder übersprungene Reviews führen über
   `learn` in die Spam-Korrektur. HTTP-Fehler bleiben sichtbar und Buttons werden
   erst nach Erfolg deaktiviert.
4. `rust/crates/dl-changelog/src/lib.rs:177`: Ein Safe-Fallback über 78 Zeichen
   wird verworfen, nicht gekürzt. Der normale Pfad nutzt die Row-ID und kann die
   vollständige Nachricht serverseitig holen.

## Zwillingssuche

`rg` über beide Repositories fand nur die produktiven Twitch-Routen, die beiden
Rust-Buttonpfade und die bestehende Migration. Keine zweite aktive Safe- oder
Spam-Buttonimplementierung wurde gefunden. Python bleibt Legacy und wird für
diesen Rust-Live-Pfad nicht erweitert.

## Fremddienstpfade

2/2 geprüft: Changelog-Cog erhält das versionierte Payload und baut genau einen
Button; Twitch-Bridge sendet den passenden internen API-Endpunkt, prüft Status
und Antwortfeld und meldet Fehler zurück.
