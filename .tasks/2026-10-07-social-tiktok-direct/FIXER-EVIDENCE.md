# Fixer, Folgeprüfung zu Gate-Runde 1

## Gesicherter Stand

Der erhaltene Implementierungsstand f637d138 wurde vor Änderungen mit normalem Push auf origin/feat/social-tiktok-direct-post gesichert. Fixcommit: 59c904d8. Origin/main wurde frisch geholt; Rebase meldete unveränderte Basis e0b0dbaf. Fremde Änderungen im gemeinsamen Checkout wurden nicht angefasst.

## Korrigierte Befunde

1. queue_upload_handler prüft den Zeitplan vor save_choice. Ein abgelehnter Zeitplan erreicht keine TikTok-Schreiboperation. PostgreSQL-Regression prüft invalid_schedule sowie unveränderte Clip-Angaben, Job-Angaben, Termin und Jobanzahl.
2. Beide JSON-Feldabfragen im Upload-Worker normalisieren JSON-null über NULLIF(..., 'null'::jsonb) zu SQL-NULL. Frühere Postfach-Jobs verwenden damit wieder den bisherigen Resolver. Neue Uploads bleiben am getrennten Direct-Post-Resolver gegen fehlende oder unvollständige Angaben gesperrt.
3. Der gemeinsame Freigabeguard erlaubt einen neuen Auftrag nach status=failed und bestätigtem tiktok_publish_status=FAILED. Die frühere Vorgangsnummer und ihre Angaben bleiben erhalten. NULL, STATUS_UNAVAILABLE, laufende Übertragung und PUBLISH_COMPLETE öffnen keine neue Freigabe. Dieselbe Funktion gilt für Queue- und Approval-Handler. PostgreSQL-Regression prüft neue Job-ID, erhaltene Historie, neuen Snapshot und acht gesperrte Zustände.

Zwillingssuche: save_choice im Queue- und Approval-Handler, die beiden JSON-Abfragen im Worker, queue_upload und upload_already_exists in der Approval-Verarbeitung geprüft. Änderungen bleiben auf drei Rust-Dateien begrenzt. Kein produktiver Post und kein Deploy vor ALLOW.

## Prüfungen

Arbeitsverzeichnis: /home/nathanael/.worktrees/tb-social-tiktok-direct/rust. Eigener PostgreSQL-16-Cluster unter /tmp/tb-tiktok-db-666eaa47 auf Port 59471. Wiederanlauf benötigte zusätzlich -k /tmp/tb-tiktok-db-666eaa47, weil der Standard-Socketordner nicht beschreibbar ist. Keine Produktionsdatenbank für Tests.

SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres /home/nathanael/.cargo/bin/cargo test -j 2 -p tb-dashboard-api --offline social_media -- --include-ignored --test-threads=1

Exit 0: 55 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1278 Bibliothekstests ausgefiltert. Zwei neue PostgreSQL-Regressionen liefen erfolgreich. Log: /tmp/tb-tiktok-fixer-api-tests.log.

SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres /home/nathanael/.cargo/bin/cargo test -j 2 -p tb-social-media --offline -- --include-ignored --test-threads=1

Exit 0: 305 Bibliothekstests und 1 Integrationstest bestanden, 0 fehlgeschlagen, 0 ignoriert. Log: /tmp/tb-tiktok-fixer-social-tests.log.

## Folgeprüfung und anschließende Integration

Gate-Runde 2 mit --model gpt-6.1-sol ergab ALLOW auf 59c904d8 und bestätigte jeden der drei alten Befunde als FIXED. Log: /tmp/tb-tiktok-gate-round-2.log.

Vor dem tatsächlichen Merge hatte sich origin/main auf 07f511a3 weiterbewegt. Der verpflichtende zweite Rebase integrierte D2. Konflikte wurden unter Erhaltung beider Funktionen gelöst: YouTube-Sichtbarkeit und TikTok-Veröffentlichungsstand in API und Clipkarte; D2-Wartezustand und faire Verbindungsprüfung neben dem TikTok-Resolver im Worker. Fehlende TikTok-Verbindungen mit vorhandener Freigabe erhalten den D2-Wartezustand. Fehlende Freigaben erlauben weiterhin keinen neuen Upload. Die beiden D2-Workerfixtures tragen jetzt eine vollständige individuelle TikTok-Freigabe und video.publish. Die Plattformfähigkeit nennt den neuen Uploadmodus direct_post statt inbox. Der integrierte Stand wird erneut geprüft; ALLOW von Runde 2 wird nicht als Freigabe des neuen Standes ausgegeben.

## Tatsächliche Bildbelege

Die vom ursprünglichen Implementierer aufgenommenen PNGs wurden vom Fixer angesehen:

- /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-desktop.png
- /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-mobile-bottom.png
- Weitere obere Mobilaufnahme: /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-mobile-top.png

Desktop zeigt Kontonamen, Beschreibung, Vorschau, leere Sichtbarkeitsauswahl, Interaktionen, Werbung, Musikbestätigung und gesperrten Einplanen-Button. Die gescrollte Mobilaufnahme zeigt die unteren Einstellungen und Aktionen im Dialog. Die Videos sind hier Testfixtures. Das ist eine eigene Sichtprüfung, keine unabhängige visuelle Zustimmung des Gate und kein Live-Beweis eines echten Clips.

## Weiter gültige Freigabe

Genau ein SELF_ONLY-Test auf earlysalty nach ALLOW, Merge und Deploy. Eigenes Deadlock-Video zuerst ansehen, keine erkennbar fremde Musik, Beschreibung und Musikbestätigung im Dashboard. Quelldateien vor einer Fehlmeldung auch über den Dienst-Bind-Mount unter /proc/<PID>/root prüfen. Clip-ID im Abschluss dokumentieren. Kein anderer Post.

Bei erneutem BLOCK gilt die inzwischen ausdrücklich erteilte autonome Fixschleife: neuer nativer Fixer-Subagent je Runde, derselbe Gate-Prüfer gpt-6.1-sol, kein neuer T3-Thread. Nach fünf erfolglosen Runden oder echtem Blocker melden.

TESTNACHWEIS[TW-1]: 55 passed, 0 ignored | Baseline: nicht neu gemessen rot
WIRKUNGSPRUEFUNG[WP-1]: 3 Befunde korrigiert | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Fixer-EVIDENCE.md
