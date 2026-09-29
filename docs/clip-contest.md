# Clip des Monats

Öffentliche Seite: `/clips`. API und Monatsabschluss laufen im bestehenden Rust-Dashboard. Diese Änderung wird als PR vorbereitet, nicht gemergt oder ausgeliefert.

## Bestehende Bausteine

Vor dem Bau wurden die lokalen Graphify-Graphen von Deadlock-Twitch-Bot, Deadlock-Bots und Website geprüft. Der Clip-Bestand ist `twitch_clips_social_media`, mit `ClipRepository` in `tb-social-media/src/clip/repository.rs`. Der gemeinsame Discord-OAuth-Broker liegt in `dl-dashboard/src/web.rs` hinter dl-web. Das Markenpaket ist `Website/dl-brand`, öffentlich unter `/brand/`.

Die Einreichung registriert einen von Helix geprüften Clip über das vorhandene Repository. Sie genehmigt keinen Social-Media-Upload, verbraucht kein bezahltes Clip-Kontingent und führt keine zweite Veröffentlichungsstrecke ein. Ein gültiger Eintrag im gemeinsamen Clip-Bestand kann erhalten bleiben, wenn anschließend die Wettbewerbs-Transaktion scheitert. Einreichung und Punkte-Ereignis selbst werden ausschließlich gemeinsam bestätigt.

Twitch verwendet `/twitch/auth/login?next=%2Fclips`, den vorhandenen OAuth-State, dessen Browserbindung und denselben Code-Tausch. Nur das Ziel `/clips` erzeugt eine öffentliche Zuschauer-Sitzung vor dem Partner-Gate. Es aktiviert keine Partnerschaft. Discord verwendet den vorhandenen lokalen Broker, dessen einmaliges Ergebnis und ein browsergebundenes State-Cookie. Beide Identitäten liegen verschlüsselt im vorhandenen `dashboard_sessions`-Speicher unter dem Typ `clip_contest`. Dieser Typ gewährt weder Partner- noch Admin-Rechte. Es gibt keinen zweiten OAuth-Client und keinen neuen OAuth-Token-Speicher.

## Regeln und Zeitgrenzen

Alle Monatsgrenzen gelten in Europe/Berlin, einschließlich Sommerzeit und Jahreswechsel. Vom 1. um 00:00 Uhr bis zum 22. um 00:00 Uhr sind Einreichungen möglich. Danach wird bis zum nächsten Monatsersten um 00:00 Uhr abgestimmt. Der erste Gewinnerabruf am Folgemonat und der minütliche Hintergrundlauf schließen fällige Monate ab. Ein Neustart holt ausstehende Abschlüsse nach.

Eine Einreichung braucht eine verifizierte Discord- oder Twitch-Identität. Helix bestätigt Clip-ID, Existenz, Quellkanal, Spiel und Erstellungszeit. Zulässig sind HTTPS-Clip-Adressen von Twitch ohne Zugangsdaten oder abweichenden Port, Deadlock, ein aktuell aktiver Partnerkanal und höchstens 60 Tage alte Clips. Eine technische Pause, manuelle Abmeldung oder Archivierung sperrt den Quellkanal. Drei Einreichungen je bekannter verifizierter Person und Monat sind erlaubt; Ausblenden gibt keinen Platz zurück. Derselbe Clip kann in einem Monat nur einmal teilnehmen.

Für eine Stimme muss die Sitzung von Discord stammen. Das Konto muss mindestens 30 Tage alt sein und die aktuelle Mitgliedschaft mindestens sieben Tage bestehen. Das Kontoalter stammt aus der Discord-ID. Die Mitgliedschaft kombiniert `activity.guild_member_directory` mit den neueren Join-/Leave-Ereignissen aus `activity.member_events`. Ein Wiedereintritt beginnt die Frist neu. Ein mehr als 26 Stunden alter Vollabgleich wird nicht als belastbarer Mitgliedschaftsnachweis akzeptiert. Fehler erlauben keine Stimme.

Pro Konto sind fünf Stimmen im Monat möglich, höchstens eine je Clip. Einreicher, Quellkanal und Clip-Ersteller dürfen nicht für einen eigenen Clip abstimmen, soweit die Plattform-IDs über bestätigte Verknüpfungen zugeordnet sind. Verknüpfungen kommen ausschließlich aus `twitch_streamer_identities`; Namen und das probabilistische Zuschauerregister werden nicht als Identitätsbeweis verwendet. Verknüpfte Identitäten und deren gespeicherte Alias-Historie teilen die Einreichungsgrenze. Unverknüpfte Konten lassen sich ohne zusätzlichen Identitätsnachweis nicht einer realen Person zuordnen; es wird keine gegenteilige Missbrauchsgarantie behauptet.

Bei gleicher Stimmenzahl gewinnt die früher eingereichte Aufnahme, danach die kleinere Einreichungs-ID. Auch Clips ohne Stimmen können gewinnen, wenn es nicht genug besser platzierte Einreichungen gibt. Bei weniger als drei sichtbaren Clips werden entsprechend weniger Plätze vergeben. Auch ein leerer oder vollständig ausgeblendeter Monat wird endgültig geschlossen.

## Transaktionen und Moderation

Alle Schreibvorgänge eines Monats teilen eine PostgreSQL-Transaktionssperre. Die aktuelle Datenbankzeit wird nach dem Sperrerwerb und bei Einreichungen erneut nach dem Clip-Repository-Aufruf geprüft. Ein verspäteter Request darf nicht in die vorige Phase schreiben. Slot-Constraints begrenzen Stimmen zusätzlich auf fünf und Einreichungen auf drei. Zusammengesetzte Fremdschlüssel verhindern Stimmen und Gewinner für den falschen Monat.

Monatsplätze und Stimmenzahlen werden einmalig gespeichert. Die Hall of Fame, Stimmen, Moderationsereignisse und Punkte-Outbox sind durch Datenbank-Trigger gegen Änderungen und Löschungen geschützt. Admins können Clips ausblenden und wieder anzeigen. Die öffentliche Liste und das Archiv zeigen ausgeblendete Clips nicht; die gespeicherten Stimmen und ursprünglichen Platzierungen bleiben erhalten. Eine Moderation nach Monatsende schließt den Monat zuerst unter derselben Sperre ab und verändert damit keine nachträgliche Rangfolge.

Alle Browser-Schreibwege verlangen einen passenden Origin oder Referer und verwerfen Cross-Site-Anfragen. Die Caddy-Route entfernt vom Client gelieferte interne Authentifizierungsheader. Die Rate-Limits betragen sechs Einreichungsversuche und zwanzig Stimmversuche je Minute und IP, zusätzlich zu den dauerhaften Kontogrenzen. Der Discord-Login ist auf zehn Starts je Minute und IP begrenzt. Der Request-Body ist auf 4 KiB begrenzt. Listen sind seitenweise begrenzt; das Archiv liefert jeweils höchstens 24 Monate.

## Übergabe an die Einsatzpunkte-Engine aus Prompt 2

`twitch_clip_contest_effort_outbox` enthält append-only:

| Feld | Bedeutung |
|---|---|
| `event_type` | `clip_submitted` oder `clip_top3` |
| `partner_twitch_user_id` | Unveränderliche ID des Quellkanals, nicht des Einreichers |
| `streamer_login` | Namens-Snapshot für die Nachvollziehbarkeit, nicht zur Identitätsauflösung |
| `source_id` | Eindeutige, wiederholungsfeste Herkunft |
| `occurred_at` | Einreichungszeit oder exakte Berliner Zeit des Ergebnis-Monatsersten |
| `metadata` | `schema_version=1`, Quellkanal-ID, Monat, Einreichung, bei Gewinnern zusätzlich Rang und Stimmen |

Der veröffentlichte Engine-PR #997 (`07f75b427f8936010cd1822b6bad49db996ee77b`) liest `metadata.submission_id` und verbindet die Outbox mit der gespeicherten Einreichung. Die Partnerzuordnung erfolgt über deren unveränderliche `broadcaster_twitch_id`, nicht über einen Login. Unser PostgreSQL-Test führt genau diese Consumer-Abfrage gegen die tatsächliche Wettbewerbs-Migration aus und prüft alle elf erzeugten Ereignisse, ihre Quellkanal-ID und die drei Gewinner-Ränge.

Punktehöhe und Wochenlimits bleiben Aufgabe dieser Engine. Der Wettbewerb rechnet keine zweite Punktelogik nach. Beide PRs sind für den tatsächlichen Betrieb zusammen erforderlich. Es wird keine produktive Gutschrift behauptet, da dieser Auftrag ausdrücklich keine Zusammenführung, Migration oder Auslieferung umfasst.

## Datenbank und Konfiguration

Eine Migration: `20260927023000_clip_contest.sql`. Sie erweitert nur den Wettbewerbsbereich und die gezielten Schreibrechte, die das bestehende Clip-Repository benötigt. Kein CREATE-Recht für Dienstrollen, keine Ausführung gegen Produktionsdaten in diesem Auftrag. Der Schema-Snapshot ergänzt die tatsächlich auf einer Scratch-Datenbank gelesenen neuen Spalten.

Die Mitgliedschaft nutzt denselben zentralen Lesepool wie die Partner-Challenges. Die bestehende lokale Datenbankidentität verbindet sich mit der zentralen Datenbank aus der normalen Konfiguration; ein zusätzlicher DSN oder Secret-Speicher ist nicht nötig. Ein fehlender oder älter als 26 Stunden alter Mitgliederabgleich erlaubt keine Stimme, auch wenn ein altes Join-Ereignis vorhanden ist. Twitch verwendet den bereits aufgebauten Helix-Client; der Discord-Broker den vorhandenen internen Token.

Zum späteren Freischalten gehören Twitch- und Caddy-PR zusammen. Die Caddy-Änderung enthält ausschließlich explizite `/clips`-Pfade in `@public_twitch`. Die Assets bleiben Teil des bestehenden Website-Builds unter `/streamer/assets/`; Marken-Assets kommen von `/brand/`.

## Oberfläche und Nachweise

Die Seite verwendet echte dl-brand-Logos und Tokens, mobile-first Schwarz/Gold, große Top-3-Karten mit denselben Abstimmungs- und Moderationsaktionen wie alle anderen Clips und ein Monatsarchiv. Stimmen und Identität werden nicht im Browser-Speicher abgelegt. Ladefehler, zu junge Konten, fehlende Mitgliedschaft und verbrauchte Stimmen werden sichtbar erklärt. Twitch-Player werden erst nach einem Klick geladen; bei zu schmalen Karten öffnet sich der Clip direkt auf Twitch statt in einem nicht unterstützten kleinen Player.

Die Browser-Screenshots unter `docs/screenshots/clips/` zeigen **synthetische Wettbewerbsdaten**, kombiniert mit bereits vorhandenen Gameplay-Vorschaubildern. Sie sind kein Nachweis echter öffentlicher Abstimmungen. Der Browsertest verwendet einen nur an Loopback gebundenen Testserver und fängt externe Player-Aufrufe ab.

Reproduzierbar:

```sh
cd rust
TB_TEST_REQUIRE_DB=1 TB_TEST_DATABASE_URL='<disposable PostgreSQL DSN>' \
  cargo test --locked -p tb-dashboard-api --lib clip_contest -- --nocapture
cd ../website
npm test
npm run build
node tests/clips-browser.mjs /path/to/playwright/index.mjs /path/to/Website/dl-brand /path/to/chromium
```

Die PostgreSQL-Tests laufen mit einer neuen, isolierten Schema-ID und testen die Migration, Grenzen, konkurrierende Stimmen, Selbststimmen, unveränderliche Auditdaten, wiederholten Monatsabschluss und die Trennung der Zuschauer-Sitzung vom Partner-Dashboard. Der bestehende SQLx-CI-Job führt diese Tests mit seiner Wegwerf-Datenbank ebenfalls aus.
