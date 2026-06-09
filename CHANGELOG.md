## #103 — Auth-Layer + Admin-Analytics-Routen (Rust Schritt 1b)

**Ausgangslage:** Der Rust-Rewrite hatte nach Schritt 1a drei öffentliche Read-only-Endpoints ohne jede Zugriffskontrolle. Alle Admin-seitigen Analytics-Routen — Streamer-Liste, Session-Übersicht — waren noch ausschließlich im Python-Backend.

**Geändert:** Rust `tb-dashboard`-Binary (Port 8767) bekommt einen vollständigen Auth-Layer und drei neue Admin-Routen:

- **Auth-Level-Extraktion** (`AuthLevel`): Jeder Request wird eingestuft — `Localhost` wenn Loopback-IP + localhost-Host-Header (kein Token nötig), `Admin` bei korrektem `X-Internal-Token`-Header (constant-time-Vergleich), `None` sonst. Der Extractor sitzt als axum-`FromRequestParts`-Impl direkt im Request-Pipeline, kein Middleware-Wrapper nötig. Partner-Session-Auth (Fernet-Cookie) bleibt deferred (ADR 0003 — kein Fernet-Nachbau, Migration erst wenn Login-Flow auf Rust wechselt).
- **IDOR-Guard** (`require_owner`): Blockiert `None`-Requests mit 401, lässt Admin/Localhost durch — vorbereitet für Partner-Level wenn es kommt.
- **Plan-Gating** (`require_extended_plan`): Liest `manual_plan_id` + `manual_plan_expires_at` aus `streamer_plans`, prüft gegen die bekannten Extended-Plan-IDs (`analytics_pro`, `analytics_extended`), respektiert Ablaufdaten. Admin/Localhost überspringen die Prüfung.
- `GET /twitch/api/v2/auth-status` — antwortet immer 200, liefert `auth_level` + `logged_in`; nützlich für Frontend-Conditional-Rendering.
- `GET /twitch/api/v2/streamers` — Admin-only; gibt alle aktiven Partner mit Live-Status + Viewer-Count zurück, sortiert nach Live-Zustand dann Viewer-Zahl.
- `GET /twitch/api/v2/overview?streamer=<login>&days=30` — Admin-only; aggregierte Session-Metriken (avg/peak Viewers, Airtime, Hours-Watched, Follower-Delta, Session-Count) für einen konfigurierbaren Zeitraum (7–365 Tage). Leerer Zeitraum liefert `{empty: true}` statt 500.

**Wie's jetzt funktioniert:** `into_make_service_with_connect_info` injiziert die echte Client-IP in jede Request-Extension — ohne diesen axum-Aufruf würde `ConnectInfo` nie befüllt und `AuthLevel::Localhost` in Produktion nie feuern. Alle SQL-Zeitvergleiche nutzen explizite `::TIMESTAMPTZ`-Casts (PostgreSQL lehnt `TEXT >= TIMESTAMPTZ` sonst ab), Aggregat-Summen explizit `::FLOAT8`/`::BIGINT` um NUMERIC-Rückgaben zu vermeiden. Jeder Test läuft in einem eigenen PostgreSQL-Schema (`SET search_path TO test_xxx`) — keine parallelen Testkonflikte möglich.

## #102 — Werbefrei deckt jetzt auch die Fake-Server-Warnung ab

**Problem:** Streamer mit Werbefrei-Abo (`chat.promos.disable` bzw. gesetztes `promo_disabled`) sollen keine automatischen Bot-Werbe-Announcements im Chat bekommen. Die regulären Promos (Chat-Aktivität, Viewer-Spike) hielten sich daran, weil sie über `_send_promo_message` laufen, das die Werbefrei-Sperre prüft. Die periodische Promo-Schleife rief aber zwei Inhalte — die Fake-Server-/Scam-Warnung und die Targeted-Promo — *direkt* auf, also am gemeinsamen Sende-Pfad und damit an der Sperre vorbei. Ergebnis: Ein Werbefrei-Kanal sah trotzdem die orange Warn-Announcement (die de facto den eigenen offiziellen Discord bewirbt) und ggf. die personalisierte Promo.

**Geändert:** Die Werbefrei-Sperre greift jetzt eine Ebene höher — direkt an der Quelle der Live-Kanal-Liste, über die die periodische Schleife iteriert. Gesperrte Kanäle werden vorab herausgefiltert, bevor irgendein Sende-Pfad sie überhaupt erreicht.

**Wie's funktioniert:** Sobald die Schleife die Liste der aktuell live-Kanäle geholt hat, läuft jeder Kanal einmal durch dieselbe Sperr-Prüfung, die der reguläre Promo-Pfad schon nutzt (DB-Lookup auf `promo_disabled` + Plan-Entitlement `chat.promos.disable`). Kanäle, die gesperrt sind, fallen aus der Liste — die nachfolgende Schleife sieht sie gar nicht mehr, weder für die Scam-Warnung noch für Targeted-, Stats- oder Viewer-Spike-Promos. Die Lurker-Steuer bleibt unberührt: Sie nutzt eine eigene Kanal-Liste und ist ein vom Streamer selbst aktiviertes Feature, kein aufgedrängter Werbeinhalt. Damit gilt „werbefrei heißt wirklich werbefrei" konsistent für alle automatischen Announcements statt nur für einen Teil der Sende-Pfade.

## #101 — Jeder Partner bekommt jetzt seinen eigenen Discord-Invite-Link

**Problem:** Der Bot sollte pro Streamer einen eindeutigen Discord-Invite erstellen, damit die Quelle jedes Joins (welcher Kanal hat jemanden reingebracht) nachvollziehbar ist. In der Praxis fiel der Bot aber regelmäßig auf den allgemeinen Fallback-Link zurück, besonders bei neu eingetragenen Partnern. Die Ursache: Der Twitch-Bot läuft als eigener Prozess mit einem eigenen Discord-Account — und dieser Account ist kein Mitglied im Deadlock-Discord-Server. `bot.guilds` war daher immer leer, `channel.create_invite()` wurde nie aufgerufen, und jeder Invite-Erstellungsversuch schlug lautlos fehl.

**Geändert:** Invite-Erstellung läuft jetzt über den internen Master-Broker (Port 8770). Der Hauptbot (der tatsächlich im Server ist) bekommt einen neuen Endpunkt `/internal/master/v1/discord/create-invite` und erstellt den Invite auf Anfrage. Der Twitch-Bot ruft diesen Endpunkt per HTTP via dem gleichen Token auf, das er ohnehin für Nachrichten-Routing nutzt.

Zusätzlich läuft beim Start ein Backfill-Task (`_ensure_partner_invites`), der alle aktiven Partner ohne Invite-Eintrag in der DB sequenziell nachversorgt — mit 0,5s Pause zwischen den Requests und einem automatischen Retry-Pass (60s Wartezeit) falls transiente Timeouts einzelne Requests beim Startup-Burst treffen.

**Wie's funktioniert:** Beim Bot-Start wartet `_ensure_partner_invites` auf das Discord-Ready-Event, fragt dann die DB nach Partnern ohne Eintrag in `twitch_streamer_invites`, und ruft für jeden `_create_streamer_invite` auf. Diese Methode baut eine HTTP-POST-Anfrage an den Broker mit `channel_id`, `reason` und einem zufälligen Idempotency-Key. Der Broker löst den Channel auf (`get_channel` / `fetch_channel`), ruft `channel.create_invite(max_age=0, max_uses=0, unique=True)` auf und gibt `invite_url`, `code`, `guild_id` und `channel_id` zurück. Der Twitch-Bot speichert das Ergebnis in `twitch_streamer_invites` (UPSERT) und cached es im Memory-Dict.

## #100 — Jahresabo: Bonusmonate automatisch + Admin-Planvergabe vollständig

**Problem:** Drei separate Baustellen. Erstens: Wer ein Jahresabo über den neuen Checkout-Flow buchte, bekam die versprochenen 2 Bonusmonate nicht — der Webhook hat sie nie in die DB geschrieben, weil das `bonus_months`-Feld nur im alten Legacy-Flow in die Stripe-Subscription-Metadata gesetzt wurde, im neuen API-Checkout-Endpunkt aber fehlte. Zweitens: Das Admin-Dashboard hatte im Dropdown zur manuellen Planvergabe nur 4 der 8 verfügbaren Pläne — `chat_quiet` (Werbefrei) und die neueren Bundles fehlten komplett, was das manuelle Setzen für diese Pläne blockierte. Drittens: Das Ablaufdatum-Feld zeigte das ISO-Format `YYYY-MM-DD`, was Tag und Monat verwechselbar macht.

**Geändert:** (1) Der neue API-Checkout-Endpunkt setzt jetzt bei `cycle_months = 12` automatisch `subscription_data.metadata.bonus_months = "2"` in die Stripe-Session — der Webhook liest das nach Zahlungseingang aus und verlängert das Ablaufdatum um 62 Tage über das Abo-Ende hinaus. (2) Das Admin-Dropdown enthält jetzt alle 8 Pläne inkl. Werbefrei, Bundle Werbefrei+Analyse und Bundle Komplett. (3) Das Ablaufdatum-Feld ist jetzt ein nativer Datums-Picker, der im Browser im deutschen Format (TT.MM.JJJJ) anzeigt.

**Wie's funktioniert:** Jahreskäufer erhalten ab sofort ihre Bonusmonate vollautomatisch: Stripe feuert `checkout.session.completed`, der Bot liest `bonus_months: 2` aus der Subscription-Metadata, addiert 62 Tage auf das `current_period_end` und schreibt das Ergebnis als `manual_plan_expires_at` in die DB — kein Admin-Eingriff mehr nötig. Checkout ohne verknüpften Login wird in beiden Flows jetzt hart geblockt: der alte Abbo-Flow leitet zurück zur Abo-Seite, der neue API-Flow gibt 401 zurück — eine Zahlung ohne zugeordnetem Account ist damit nicht mehr möglich.

## #99 — Global-Ban-Sweep nur noch für OAuth-autorisierte Kanäle

**Problem:** Der tägliche Ban-Sweep hat alle aktiven Partner-Kanäle durchgearbeitet — auch solche, die sich nie per OAuth autorisiert haben und bei denen der Bot gar kein Moderator sein kann. Das war verschwendete Arbeit und produzierte unnötige 403-Fehler.

**Geändert:** Der Sweep prüft jetzt vor dem Durchlauf, welche Streamer einen gültigen OAuth-Eintrag haben (`needs_reauth = FALSE`). Kanäle ohne Eintrag oder mit ungültigem Token werden übersprungen — invalide Token werden sowieso departnered.

**Wie's funktioniert:** Vor dem Sweep lädt der Bot einmalig alle `twitch_user_ids` mit gültigem OAuth aus der Datenbank. Nur Kanäle, die in dieser Menge enthalten sind, landen im Durchlauf. Wer kein OAuth hat, kommt gar nicht erst dran — statt mit 403 zu scheitern.

## #98 — Keine doppelte Werbung mehr

**Problem:** Der Bot hat seine Discord-Werbung manchmal doppelt gepostet — zwei Werbungen mit nur einer einzigen Chat-Nachricht dazwischen. Eigentlich gibt es eine Mindestschwelle, wie viele Nachrichten zwischen zwei Werbungen liegen müssen, aber die wurde übergangen. Grund: Über das Werben entscheiden zwei Stellen parallel — eine reagiert auf jede eingehende Chat-Nachricht, eine läuft als Timer im Hintergrund. Beide prüfen die Schwelle, senden dann aber erst nach mehreren Zwischenschritten und setzen den „zuletzt geworben"-Marker erst ganz am Ende. Trifft eine Chat-Nachricht genau in dieses Zeitfenster, sehen beide noch den alten Stand und werben gleichzeitig.

**Geändert:** Beide Werbe-Pfade werden jetzt pro Kanal gegeneinander gesperrt, sodass immer nur einer gleichzeitig entscheiden und senden darf.

**Wie's funktioniert:** Ein Schloss pro Kanal umschließt die komplette Sequenz „Schwelle prüfen → senden → Marker setzen". Während ein Pfad sendet, muss der andere warten; sobald er drankommt, sieht er den frisch gesetzten Marker und die noch nicht zurückgesetzte Nachrichtenschwelle und bricht ab. Doppelte Werbung ist damit ausgeschlossen, und die Mindestnachrichten-Schwelle greift wieder zuverlässig.

## #97 — Netzwerkweite Sperrliste bannt jetzt vorsorglich (nur wenn offline)

**Problem:** Es gibt eine netzwerkweite Sperrliste für unerwünschte Accounts (z.B. Scammer oder Leute, die gegen die Community-Richtlinien verstoßen haben). Bisher wurde so jemand erst gebannt, wenn er in einem der betreuten Kanäle tatsächlich geschrieben hat — der Ban passierte also reaktiv, mitten im laufenden Stream, oft in einem Kanal, in dem die Person vorher nie aufgetaucht war. Das war auffällig und für Zuschauer verwirrend (warum bannt der Bot jemanden, der gerade nichts getan hat?). Und wer einfach nichts schrieb, rutschte komplett durch.

**Geändert:** Zusätzlich zum reaktiven Ban gibt es jetzt einen vorsorglichen Abgleich, der gesperrte Accounts aus allen betreuten Kanälen bannt — aber bewusst nur, wenn der jeweilige Kanal gerade offline ist. Außerdem wurde die Chat-Begründung beim reaktiven Ban ehrlich gemacht.

**Wie's funktioniert:** Der Abgleich läuft zu zwei Zeitpunkten, beide an „Streamer ist offline" gekoppelt: einmal rund eine Stunde nachdem ein Stream endet (gezielt für genau diesen Kanal), und einmal täglich gegen 6 Uhr morgens als Sammeldurchlauf über alle Kanäle, die gerade nicht senden. Ist ein Kanal live, wird er übersprungen und beim nächsten Mal erneut versucht — so wird nie jemand sichtbar mitten im Stream gebannt. Damit nicht bei jedem Durchlauf dieselben Bans erneut an Twitch geschickt werden, merkt sich das System pro Kombination „Account × Kanal", wo der Ban schon gesetzt wurde. Der frühere reaktive Ban bleibt als Sicherheitsnetz — falls jemand schreibt, bevor der Offline-Abgleich den Kanal erreicht hat — sagt im Chat jetzt aber klar, dass die Person netzwerkweit gesperrt ist (Verstoß gegen die Community-Richtlinien), statt sie pauschal als Spam zu deklarieren. Wer neu auf die Sperrliste kommt, landet automatisch auch auf der Raid-Sperrliste.

## #96 — Engagement-AI: Validate+Erklär-Muster gebrochen

**Problem:** 44% aller generierten Nachrichten folgten dem Muster `[kurze Reaktion], [erklärender zweiter Teil]` — z.B. "haha genau, das ist halt typisch für den hero". Dieses Muster ist das sicherste Bot-Tell überhaupt: echte Chatter sagen entweder die Reaktion oder die Meinung — nie beides in einem Satz.

**Geändert:** Zwei Stellen.

1. **System-Prompt:** Neuer Abschnitt mit konkretem Gegenbeispiel (Show-don't-tell statt abstrakter Regel): Die KI soll bei einem Komma in ihrer Antwort alles nach dem Komma streichen und prüfen ob der erste Teil alleine steht. Dazu explizit: entweder Reaktion oder Meinung, nie beides plus Begründung.

2. **Gold-Beispiele in den Few-Shot-Vorlagen:** Die kuratierten Muster-Nachrichten wurden um mehr reine Reaktionen ergänzt ("alter bitte", "echt wild", "ngl stimmt", "no shot den", "ich auch") — kurze Fragmente ohne Erklär-Nachklapp. Diese erscheinen immer als erste im Prompt und konditionieren den Ton.

**Wie's funktioniert:** LLMs imitieren konkrete Beispiele besser als abstrakte Regeln. Wenn die ersten 4 Beispiele alle pure Reaktionsfragmente ohne Erklärung sind, schreibt das Modell automatisch im gleichen Register — statt zu erklären warum etwas "genau" ist.

## #95 — Engagement-AI: weniger Bot-isch, keine Dialekt-Imitation

**Problem:** Die AI hat sich im Chat zu oft verraten — vor allem durch einen einzigen Tell: 40% aller Nachrichten fingen mit "haja" an, weil das Modell den Ausdruck aus den Channel-Stilvorlagen übernahm und ihn auf jede Antwort klebte. Dazu kamen erfundene Compound-Wörter ("brumm-heizkessel", "konsequenz-plan"), broken Schweizerdeutsch in Channels wo Schweizer zuschauen, und gelegentlich `<3` als Antwort auf komplett unpassende Trigger.

**Geändert:** Drei Stellen gleichzeitig angepasst.

1. **Starter-Repeat-Guard in der Pipeline:** Nach dem Modell-Call wird das erste Wort der generierten Antwort mit dem ersten Wort der letzten Bot-Nachricht verglichen. Stimmen sie überein → Antwort verworfen (silent). Das ist ein harter Filter unabhängig vom Prompt — fängt auf, wenn das Modell die Prompt-Regel ignoriert.

2. **System-Prompt-Ergänzungen:** Vier neue Regeln direkt im Baseline-Prompt: (a) nie mit demselben Wort starten wie die vorherige Antwort, (b) "haja", "hmm", "naja", "danke" nicht als Opener, (c) kein Dialekt — auch wenn Schweizerdeutsch im Channel läuft, bleibt die Antwort normales Deutsch mit Chat-Slang, (d) `<3` nie als eigene Antwort senden.

3. **Style-Beispiel-Deduplication:** Die Few-Shot-Stilvorlagen, die dem Modell aus dem Channel-Chat gezogen werden, dürfen jetzt max. 2 Beispiele mit demselben Starter-Wort enthalten. Vorher konnten 6 von 8 Beispielen mit "haja" anfangen — das hat das Modell trainingsartig auf genau das konditioniert.

**Wie's funktioniert:** Prompt und Filter wirken als Doppelnetz: der Prompt verhindert, der Guard fängt auf. Die Stil-Dedup greift schon früher und sorgt dafür, dass das Modell gar nicht erst lernt, dass "haja" der Standard-Opener ist.

## #94 — Fairere Raid-Verteilung im Fallback (kein Partner online)

**Problem:** Wenn kein Partner live war, wurde der Fallback-Kanal immer nur nach wenigsten Viewern ausgewählt — ohne Rücksicht darauf, wie viele Raids jemand schon bekommen hat. Wer klein streamt und oft live ist, bekam dadurch unverhältnismäßig viele Raids.

**Geändert:** Die Fallback-Selektion (`select_fairest_candidate`) berücksichtigt jetzt `received_successful_raids_total` aus der Score-Datenbank als primären Sortierschlüssel. Viewer-Count bleibt Tiebreaker.

**Wie's funktioniert:** Vor der Auswahl wird für alle Kandidaten die bisherige Raid-Gesamtzahl aus der DB geladen. Sortiert wird dann zuerst nach wenigsten erhaltenen Raids — wer noch kaum Raids hat, kommt nach vorne. Kanäle die noch gar nicht im System sind, bekommen automatisch 0 und haben damit höchste Priorität. Viewer-Count und Follower-Count entscheiden bei Gleichstand. Das verhindert, dass ein Kanal mit wenig Viewern dauerhaft alle Fallback-Raids kassiert während andere leer ausgehen.

## #93 — Abo-Preise drastisch gesenkt + Raid-Boost-Gewichtung reduziert

**Ausgangslage:** Die Abo-Preise lagen zwischen 3,99 € und 13,99 € und wurden als zu hoch eingestuft — insbesondere weil das Analyse-Dashboard aktuell MiniMax als KI nutzt, nicht Claude.

**Geändert:** Alle sieben Abo-Stufen neu bepreist, neue Stripe-Preise angelegt und die alten deaktiviert. Zusätzlich wurde die Zusatz-Gewichtung für Raid-Boost-Abonnenten von 25 % auf 15 % gesenkt.

**Neue Preise:**
- Werbefrei: 3,99 € → **1,99 €/Monat**
- Raid Boost: 3,99 € → **1,99 €/Monat**
- Analyse Dashboard: 8,49 € → **1,99 €/Monat**
- Werbefrei + Raids: 6,99 € → **3,49 €/Monat** (spart 49 ¢ gegenüber Einzelkauf)
- Werbefrei + Analyse: 10,49 € → **3,49 €/Monat**
- Analyse + Raids: 11,49 € → **3,49 €/Monat**
- Alles drin (Komplett): 13,99 € → **4,99 €/Monat** (spart 0,98 € gegenüber Einzelkauf)

**Wie Preise in Stripe funktionieren:** Stripe-Preise sind unveränderbar — ein gesenkter Preis bedeutet immer: neuen Price-Eintrag anlegen, alten auf inaktiv setzen. Bestehende Abos laufen weiterhin auf dem alten Preis, bis sie verlängert oder migriert werden. Neue Checkout-Sessions zeigen sofort die neuen Preise. Die Raid-Boost-Gewichtung ist ein interner Score-Multiplikator: Abonnenten werden im Raid-Netzwerk weiterhin bevorzugt, aber weniger stark als vorher (15 % statt 25 % Score-Aufschlag).

## #92 — Live-Ping-Rolle: Umbenennung + Streamer pingen sich nicht mehr selbst

**Problem:** Die automatisch erstellte Discord-Ping-Rolle hieß `KANALNAME LIVE PING` (alles Großbuchstaben, englisch) — unpassend. Dazu hatte der Bot beim Erstellen der Rolle dem Streamer die Rolle direkt selbst zugewiesen: wer live geht, bekam die eigene Ping-Rolle und wurde beim nächsten Go-Live-Event angepingt. Das ist das Gegenteil von sinnvoll.

**Geändert:** Der Rollenname folgt jetzt dem Format `kanalname ist live`. Die Zuweisung der Ping-Rolle an den Streamer selbst wurde in beiden Code-Pfaden (Monitoring-Flow + Dashboard-Live-Flow) entfernt.

**Wie's funktioniert:** Die Rolle wird nach wie vor automatisch erstellt und im Live-Embed als Mention eingebunden — Zuschauer können sie sich selbst in Discord zuweisen, um Benachrichtigungen zu erhalten. Der Streamer bekommt sie nicht mehr automatisch und wird damit nicht über den eigenen Stream angepingt.

## #91 — Live-Ankündigung: Textzeile entfernt, Umlaute gefixt, Offline-Embed aufgeräumt

**Problem:** Drei Kleinigkeiten, die zusammen störten. (1) Vor jedem Live-Embed stand eine redundante Klartextzeile (`X ist live! Schau über den Button unten rein.`), die denselben Inhalt wie das Embed selbst doppelt angezeigt hat. (2) Im Footer (`fuer`) und im Content-Template (`ueber`) standen ASCII-Umlaute statt echter Zeichen — sichtbar für alle Streamer ohne eigene DB-Konfiguration, z. B. daxy. (3) Das Offline-Embed zeigte `OFFLINE` dreifach: im Embed-Titel, im Author-Label (`OFFLINE: Name`) und als eigenes Status-Feld.

**Geändert:** Content-Template auf reinen Rollen-Ping (`{rolle}`) gekürzt — die Textzeile fällt weg, der Mention bleibt. Footer-Text und Template-Fallbacks verwenden jetzt echte Umlaute. Im Offline-Embed ist das redundante Status-Feld (`OFFLINE`) entfernt und das Author-Label zeigt nur noch den Streamer-Namen ohne Präfix.

**Ergebnis:** Live-Embeds sehen kompakter aus (kein doppelter Informationstext über dem Embed). Offline-Embeds zeigen `OFFLINE` nur noch einmal im Titel. Alle Standardtexte verwenden korrekte Umlaute.

## #90 — Admin-Dashboard: Changelog-History, Raid-Historie, DB-Query, Raw-Chat-Lag-Fix, Memory-Fix

**Problem:** Sechs Baustellen im Admin-Dashboard auf einmal. (1) Die Changelog-Seite zeigte nie eine History, weil das Backend das Feld schlicht nicht lieferte. (2) Die Raids-Seite zeigte keine vergangenen Raids — gleicher Grund: Backend lieferte keine History. (3) Die EventSub-Seite zeigte bei inaktivem WebSocket 0 Subscriptions, ohne Hinweis was zuletzt registriert war. (4) Die Memory-KPI zeigte dauerhaft „0 B", weil `psutil` nicht im venv installiert war. (5) Der Raw-Chat-Lag-Warning blieb permanent aktiv für `its_raffi`, weil der Live-State seit dem EventSub-Ausfall nie auf Offline gesetzt wurde — der Bot behandelte einen 80 Tage alten Timestamp als „live". (6) Keine Möglichkeit, direkt Daten aus der DB abzulesen.

**Geändert:**
- **Changelog-History:** Der `/twitch/api/admin/config/overview`-Endpoint gibt jetzt `changelog.entries` mit den letzten 20 Einträgen aus der `internal_home_changelog`-Tabelle zurück — die History-Sektion auf der Changelog-Seite füllt sich damit automatisch.
- **Raid-Historie:** Derselbe Endpoint enthält jetzt `raids.history` mit den letzten 50 Einträgen aus `twitch_raid_history` (Streamer, Ziel, Viewer-Zahl, Zeitstempel, Erfolg). Die Raids-Seite im Dashboard zeigt sie direkt in der Tabelle.
- **EventSub Last-Known-Snapshot:** Wenn der WebSocket inaktiv ist und keine aktiven Subscriptions vorliegen, liest der EventSub-Endpoint den letzten Snapshot mit `listener_count > 0` aus `twitch_eventsub_capacity_snapshot` — inklusive `listeners_json` — und gibt ihn als `lastKnownSubscriptions` zurück. Die EventSub-Seite zeigt diesen Snapshot mit Zeitstempel als eigene Sektion.
- **Raw-Chat-Lag-Fix:** Das Live-Scope-JOIN in `_fetch_raw_chat_health_snapshot` prüft jetzt zusätzlich, ob `last_seen_at`/`last_started_at` des Live-States nicht älter als 4 Stunden ist. Eingefrorene States aus der Zeit vor dem EventSub-Ausfall gelten nicht mehr als „live" — die veraltete Warnung verschwindet beim nächsten Health-Poll.
- **Memory-Fix:** `psutil` ins venv installiert. Die RSS/Process-Memory-KPI im System Overview zeigt jetzt den echten Prozess-Speicherverbrauch.
- **DB Query:** Neuer Admin-Endpoint `GET /twitch/api/admin/system/query?sql=...` — nur SELECT erlaubt, max. 200 Rows, gefährliche Keywords (`INSERT`, `UPDATE`, `DROP` etc.) werden geblockt. Neue Seite „DB Query" in der Sidebar unter Operations: Tabellenliste zum Klicken, SQL-Textarea mit Ctrl+Enter, Ergebnistabelle.

**Wie's funktioniert jetzt:** Admin öffnet `/twitch/admin/operations/query`, klickt eine Tabelle an, der Editor füllt sich mit einem Basis-SELECT, Ctrl+Enter schickt die Abfrage ab, das Ergebnis erscheint als sortierbare Tabelle. Schreiboperationen schlägt der Server mit HTTP 400 zurück bevor eine Verbindung zur DB geht.

## #89 — Live-Ankündigungen: kein Schriftzug über dem Embed, echte Umlaute, Promos nur für aktive Partner

**Problem:** Drei separate Baustellen. Erstens stand über jedem Live-Embed ein redundanter Klartext-Satz („X ist live! Schau über den Button unten rein."), der denselben Inhalt wie das Embed doppelt zeigte. Zweitens verwendeten Embeds und Buttons ASCII-Ausweichreplacement statt echter Umlaute (`ue`, `fuer`, `ueber` statt `ü`, `für`, `über`). Drittens wurden Chat-Promos an Streamer gesendet, auch wenn deren Partner-Status inzwischen deaktiviert oder archiviert war.

**Geändert:** Der Freitext-Content der Go-Live-Nachricht enthält jetzt nur noch die Rollen-Mention (damit der Ping feuert), kein angehängter Beschreibungstext mehr — das Embed trägt alle Infos. Der Offline-Edit schreibt ebenfalls keinen Plaintext mehr über das OFFLINE-Embed. Alle `ue`/`fuer`/`ueber`-Einträge in Embed-Titeln, Feldern und Footer wurden durch echte Umlaute ersetzt. `_promo_channel_allowed` fragt jetzt live gegen `twitch_streamers_partner_state` ab: `is_partner_active = true` und `archived_at IS NULL` — wer nicht mehr aktiver Partner ist, bekommt keine Chat-Promos mehr.

**Wie's funktioniert:** Discord zeigt Mentions nur dann als echten Ping an, wenn die Mention-ID im Content-Text steht — deshalb bleibt der Content nicht leer, sondern enthält nur die `<@&role_id>`. Fehlt eine Rolle (kein Ping konfiguriert), ist Content `None` → keine sichtbare Textzeile über dem Embed. Die Partner-Prüfung passiert bei jedem `_promo_channel_allowed`-Aufruf synchron gegen die DB, da dieselbe Funktion ohnehin vor echten Netzwerk-/API-Operationen steht.

## #88 — Chat-Werbung: kein Spam zum Stream-Start, Fake-Server-Warnung wird endlich sichtbar

**Problem:** Drei zusammenhängende Macken in der automatischen Discord-Werbung im Chat. Erstens warf der Bot direkt zu Stream-Beginn eine Werbenachricht raus, obwohl noch niemand im Chat geschrieben hatte. Zweitens griff die eigentlich vorgesehene Regel „erst nach genug Chat-Aktivität werben" nicht — es wurde geworben, egal wie tot der Chat war. Und drittens tauchte die Warnung vor den gefälschten Discord-Servern („Deadlock Discord Deutschland" / „Deadlock German Competitiv HUB") praktisch nie auf.

Ursache war ein zweiter, neuerer Werbe-Weg (die zielgerichtete Promo mit KI-Vorauswahl), der parallel zum älteren lief, aber nur seinen eigenen 15-Minuten-Kanaltakt prüfte — nicht die Aktivitäts-Schwellen. Bei frischem Stream ist dieser Takt sofort „frei", also feuerte er sofort. Und weil er den Werbe-Slot dauerhaft belegte, kam der ältere Weg, an dem die Fake-Server-Warnung hängt, fast nie zum Zug — die Warnung wurde regelrecht ausgehungert, ihr Timer nicht mal gestartet.

**Geändert:** Beide Werbe-Wege hängen jetzt an derselben Aktivitäts-Schwelle, und die Fake-Server-Warnung wird eine Ebene höher entschieden — dort, wo feststeht, welcher Werbetyp den Slot überhaupt bekommt.

**Wie's funktioniert:** Bevor geworben wird — egal über welchen Weg — muss seit der letzten Werbung genug echte Chat-Aktivität zusammengekommen sein: eine Mindestzahl an Nachrichten seit der letzten Promo, mehrere verschiedene Schreiber im Zeitfenster und (nach der ersten Promo) ein paar neue Gesichter. Erst wenn das erfüllt ist, kommt eine Werbung — am toten Stream-Start also gar nichts. Die Fake-Server-Warnung wird jetzt zentral im fälligen Werbe-Slot geprüft: Ist ihr eigener Takt (Standard 45 Minuten pro Kanal) reif, kommt die Warnung statt einer normalen Promo — unabhängig davon, welcher Werbe-Weg sonst gefeuert hätte. So wechseln sich Werbung und Warnung von selbst ab, statt dass ein Weg den anderen verdrängt. Der Wortlaut bleibt bewusst vorsichtig („könnte/möglicherweise Fake"), nennt die beiden Server aber klar beim Namen.

**Betroffen:** Alle Partner-Kanäle, in denen der Bot Discord-Werbung postet. Zuschauer sehen am Stream-Anfang keine Werbung mehr ins Leere und bekommen die Warnung vor den Fake-Servern jetzt regelmäßig zu Gesicht.

## #87 — Admin-Dashboard: Logout und Direktaufrufe führen nicht mehr ins Leere

**Problem:** Zwei Ärgernisse beim Admin-Zugang. Erstens schickte der Logout-Button einen auf eine „Seite nicht gefunden", statt sauber abzumelden — er rutschte in den Anmelde-Weg der öffentlichen Streamer-Seite, dessen Ziel es auf der Admin-Adresse gar nicht gibt. Zweitens: Wer eine Admin-Unterseite direkt aufrief (z. B. einen gespeicherten Link), ohne angemeldet zu sein, bekam eine nackte Fehlerseite (401) statt zur Anmeldung geleitet zu werden — man musste erst umständlich die Startseite ansteuern und sich dort einloggen.

**Geändert:** Auf der Admin-Adresse führt der Logout jetzt direkt zur Admin-Anmeldung. Und ein Direktaufruf einer Admin-Seite ohne Anmeldung leitet automatisch zur Anmeldung weiter, statt eine Fehlerseite zu zeigen.

**Wie's funktioniert:** Der Abmelde-Vorgang erkennt jetzt, dass er auf der Admin-Adresse läuft, und nimmt den richtigen Discord-Abmelde-Weg — der die Sitzung beendet und auf die Anmeldeseite führt, statt in den Twitch-Login der Streamer-Seite zu rutschen (dessen Weiterleitungsziel auf der Admin-Adresse nicht existiert und deshalb „nicht gefunden" lieferte). Für Direktaufrufe prüft der vorgelagerte Reverse-Proxy, ob überhaupt eine Admin-Sitzung vorliegt: fehlt sie, geht es sofort zur Anmeldung; ist sie da, läuft alles unverändert weiter — Angemeldete merken nichts. Der Proxy musste dafür einmal neu gestartet werden, weil seine Konfigurationsdatei nur als Einzeldatei eingebunden war und Änderungen sonst nicht ankamen.

**Betroffen:** Admin-Login-/Logout-Komfort; für Zuschauer und Streamer nichts sichtbar.

## #86 — Chat-KI (Testphase): vom Cringe-Mitspieler zum stillen Zuschauer

**Problem:** Der KI-Stammgast, der gerade nur intern im Review getestet wird, verhielt sich wie ein aufdringlicher Mitspieler statt wie ein normaler Zuschauer. Er reagierte auf praktisch jede Chatzeile — auch auf einzelne Emotes, Begrüßungen, Reaktionswörter („gg", „easy") und Chat-Commands. Schlimmer: Lob für gute Spielzüge nahm er an, als hätte er selbst gespielt („läuft grad gut"), sprach einzelne Zuschauer wie ein Gastgeber mit Namen an und klinkte sich in Absprachen der Stammcrew ein. Dazu klangen die Antworten nach KI — zu lang, zwei ausformulierte Sätze mit Abschluss-Pointe, wo ein echter Chatter nur ein Fragment hinwirft.

**Geändert:** Eine zweistufige Brems-Logik, ein neues Selbstbild und ein an echten Chatdaten geeichter Schreibton. Außerdem hört die KI im Test nur noch echte Partnerkanäle mit.

**Wie's funktioniert:**
- Vor dem Sprachmodell sortiert ein billiger Vorfilter offensichtliches Rauschen aus, ohne das Modell überhaupt zu fragen: einzelne Emotes und Reaktionswörter, mehrfach wiederholte Emote-Ketten, Chat-Commands und Nachrichten, die mit „@name" direkt an eine bestimmte Person gehen. Das spart Rechenzeit und lässt sich nicht per Prompt „überreden".
- Das Modell weiß jetzt klar, dass es Zuschauer ist und nicht der Streamer: Lob und Zurufe zum Spielgeschehen gehören dem Streamer, nicht ihm; es grüßt, dankt und verabschiedet niemanden wie ein Gastgeber und mischt sich nicht in Pläne ein.
- Melden darf es sich nur noch bei einem echten Deadlock-Anlass — einer konkreten Frage oder Meinung zu Helden, Items, Builds oder Meta. Alles andere bleibt stumm; der Normalfall ist Schweigen.
- Der Schreibton wurde an rund 3000 echten Chatnachrichten gemessen (typisch: drei bis vier Wörter, ein einziger Satz, kaum Satzzeichen, echte Umlaute) und genau darauf geeicht — kurze Fragmente statt ausformulierter Absätze.

**Betroffen:** Intern. Die Chat-KI ist weiter in der Review-Testphase und für Zuschauer wie Streamer noch nicht aktiv.

## #85 — Admin-Dashboard: Affiliate-Abrechnung repariert + Admin-Login hält 2 Wochen

**Problem:** Zwei Dinge im Admin-Bereich. Erstens warf die Affiliate-Abrechnung beim Öffnen einen Server-Fehler (500) und lud gar nicht — die Datenbank-Abfrage fragte eine Spalte („Provisionssatz") ab, die es in der Vertriebler-Konten-Tabelle nie gegeben hat. Zweitens fiel die Admin-Anmeldung schon nach wenigen Stunden wieder raus (die Sitzung galt nur 6 Stunden), was sich anfühlte, als wäre das Dashboard ständig „tot": Oberfläche lädt, aber jede Aktion läuft ins Leere, weil man im Hintergrund längst abgemeldet war.

**Geändert:** Die nicht existierende Spalte fliegt aus der Abrechnungs-Abfrage. Die Gültigkeit der Admin-Sitzung steigt von 6 Stunden auf 2 Wochen.

**Wie's funktioniert:** Provisionen werden ohnehin als fester Betrag pro Buchung gespeichert, nicht als Satz am Konto — die Abfrage liest jetzt nur noch tatsächlich vorhandene Felder, der 500er ist weg und die Abrechnungs-Liste lädt wieder. Bei der Sitzung wurden beide beteiligten Lebensdauern angehoben: die zentrale Login-Sitzung (über die der Admin-Zugang läuft) und die davon abgeleitete Dashboard-Sitzung stehen jetzt auf 14 Tagen und verlängern sich bei jeder Nutzung automatisch — einmal anmelden reicht damit praktisch für zwei Wochen, statt mehrmals täglich neu einloggen zu müssen.

**Betroffen:** Admin-intern (Affiliate-Verwaltung und Login-Komfort), für Zuschauer und Streamer nichts sichtbar.

## #84 — Tracking-Tabelle: vestigiale Alt-Spalten entfernt

**Problem:** Auf der Tracking-Tabelle lagen noch vier Spalten aus einem alten Schema (Beschreibung des letzten Link-Checks, Link-Status, „hinzugefügt von", Zeitpunkt des letzten Checks) — partner-klingende Reste, die seit Langem von keinem Code mehr geschrieben oder gelesen wurden. Tot, aber verwirrend, weil sie auf der „nur Tracking"-Tabelle nach Partner-Daten aussehen.

**Geändert:** Diese vier toten Spalten wurden entfernt. Eine geprüfte Abhängigkeits-Analyse hat vorher bestätigt, dass kein Programmteil sie noch liest.

**Wie's funktioniert:** Ein weiterer versionierter Migrationsschritt entfernt die vier Spalten (idempotent, greift nur solange sie existieren). Ihre Daten waren ohnehin schon im Gesamt-Backup aus #83 enthalten. Die interne Identitäts-Spalte der Tabelle bleibt unangetastet. Damit enthält die Tracking-Tabelle wirklich nur noch Tracking-/Identitäts-Felder — keine partner-klingenden Altlasten mehr.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene.

## #83 — Partner-DB-Konsolidierung: doppelte Partner-Spalten endgültig entfernt

**Problem:** Partner-Eigenschaften (Verifizierungs-Status, Raid-Schalter, Stumm-Flags, Live-Ping-Einstellungen, Discord-Link-Pflicht) lagen jahrelang in **zwei** Tabellen gleichzeitig — einmal in der schmalen Partner-Tabelle (die eigentliche Wahrheit) und einmal als gespiegelte Kopie in der breiten Tracking-Tabelle. Zwei Pflegeorte für denselben Wert heißt: sie können auseinanderlaufen, und genau dieser Drift war die Wurzel der wiederkehrenden „mal stimmt der Partner-Status, mal nicht"-Verwirrung (siehe #80–#82).

**Geändert:** Die elf gespiegelten Spalten wurden aus der Tracking-Tabelle entfernt. Der Partner-Status lebt jetzt an genau einer Stelle.

**Wie's funktioniert:** Voraussetzung war die Vorarbeit aus #80–#82 — erst nachdem nachweislich kein Programmteil diese Spalten mehr aus der Tracking-Tabelle liest und die zentrale Schreib-Stelle sie nicht mehr dorthin spiegelt, sind sie „tot" und können weg. Ein versionierter Migrationsschritt legt beim Start einmalig ein vollständiges Backup der Tracking-Tabelle an und entfernt dann die elf Duplikat-Spalten; er greift nur solange die Alt-Spalten existieren und läuft sonst als No-op. Reine Tracking-Felder (Beobachtungs-Archivierung, „nur beobachtet"-Markierung, Identität) bleiben unangetastet. Ergebnis: eine einzige Quelle für den Partner-Status, kein Drift mehr möglich, und die ohnehin kanonische Lese-Sicht zieht ihre Werte unverändert aus der Partner-Tabelle.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene, Abschluss der Partner-DB-Konsolidierung.

## #82 — Partner-DB-Konsolidierung: Monitoring-Loop liest keine Partner-Config mehr aus der Tracking-Tabelle

**Problem:** Die Überwachungs-Schleife lud auch für reine Beobachtungs-Kanäle (keine Partner) Partner-Einstellungen wie „Discord-Link nötig" oder die Live-Ping-Rolle aus der breiten Tracking-Tabelle mit — Werte, die für einen Nicht-Partner nichts bedeuten. Das war der letzte offene Leser aus der Aufräumarbeit von #80/#81.

**Geändert:** Für Nicht-Partner setzt die Schleife diese Partner-Felder jetzt auf ihre Standardwerte, statt sie aus der Tracking-Tabelle zu lesen. Das echte Beobachtungs-Feld „archiviert?" bleibt unangetastet.

**Wie's funktioniert:** Aktive Partner kommen ohnehin schon aus der zentralen Partner-Sicht; nur der Zweig für reine Beobachtungs-Kanäle griff noch auf die gespiegelten Partner-Spalten zu. Für solche Kanäle sind diese Felder jetzt fest auf den Standard gesetzt (kein Link-Zwang, keine Live-Ping-Rolle) — exakt das, was dort heute ohnehin drinstand. Damit liest kein Programmteil mehr Partner-Status aus der Tracking-Tabelle; die doppelten Spalten lassen sich in einem späteren Schritt gefahrlos entfernen.

**Betroffen:** Nichts sichtbar — interne Datenhygiene, Abschluss der Leser-Umstellung.

## #81 — Partner-Status-Konsolidierung: zwei weitere Leser auf die zentrale Wahrheit

**Problem:** Mehrere Programmteile lasen Partner-Eigenschaften (z. B. „verifiziert seit", Raid-/Live-Ping-Einstellungen) direkt aus der breiten Tracking-Tabelle statt aus der zentralen Partner-Sicht. Dort liegen diese Eigenschaften nur als gespiegelte Kopie — pflegt man denselben Wert an zwei Orten, driften die beiden Stände früher oder später auseinander. Das ist die Fortsetzung der Aufräumarbeit aus #80.

**Geändert:** Zwei dieser Stellen beziehen den Partner-Status jetzt aus der zentralen Wahrheit bzw. fragen ihn gar nicht mehr aus der Tracking-Tabelle ab.

**Wie's funktioniert:** Der Post-Stream-Report lud zwei Partner-Konfig-Werte mit, die er nirgends verwendet hat — die werden schlicht nicht mehr abgefragt. Der Admin-Verifizierungs-Dialog entscheidet „schon verifiziert → keine erneute Benachrichtigung" jetzt anhand des echten Partner-Datensatzes statt anhand der gespiegelten Kopie in der Tracking-Tabelle. Für bestehende Partner ist das Ergebnis heute identisch (beide Kopien sind gleich); der Unterschied greift erst, wenn die doppelten Spalten später ganz aus der Tracking-Tabelle entfernt werden — dann gibt es nur noch eine Quelle, die nicht mehr veralten kann.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — reine interne Datenhygiene als Zwischenschritt der Partner-DB-Konsolidierung.

## #80 — Partner-Status-Check: kein veralteter Nachbau mehr

**Problem:** Der Bot beantwortet an mehreren Stellen die Frage „ist das ein operativ aktiver Partner?" — und zwei dieser Stellen waren sich uneinig. Die eine las den zentral gepflegten Partner-Status; die andere, eigentlich die strengere (sie steuert u. a. den Bot-Ban-/Blacklist-Schutz und seit der letzten Änderung auch das Engagement-AI-Gate), rechnete den Status von Hand aus dem rohen Partner-Datensatz nach. Dieser Nachbau war veraltet: Er prüfte nur, ob die Partnerschaft formal aktiv, nicht per Opt-out abgeschaltet und nicht admin-archiviert war — den Zustand „technisch pausiert" bzw. „Bot wurde in diesem Kanal gebannt" hat er übersehen. Folge: Kanäle, in denen der Bot pausiert oder gebannt war, zählten trotzdem als operative Partner. Genau das war der Auslöser dafür, dass die Engagement-AI in solchen Kanälen anschlug.

**Geändert:** Die strenge „operativ aktiver Partner"-Prüfung rechnet nichts mehr selbst nach, sondern liest dieselbe zentrale Wahrheit wie alle übrigen Partner-Checks.

**Wie's funktioniert:** Es gibt eine zentrale, laufend gepflegte Sicht auf den Partner-Status, die alle Ausschlussgründe zu einem einzigen „aktiv: ja/nein" zusammenfasst — formal aktiv, kein Opt-out, nicht archiviert, nicht pausiert, nicht gebannt. Die strenge Prüfung schaut jetzt genau dort nach, statt die Bedingungen einzeln und unvollständig selbst zusammenzusetzen. Dadurch ist sie immer auf demselben Stand wie diese Sicht: Wird ein Partner pausiert oder der Bot dort gebannt, fällt der Kanal sofort aus allen Pfaden, die diese Prüfung nutzen. Vorher konnte der handgestrickte Nachbau der zentralen Sicht „hinterherhinken", weil er bei jeder Erweiterung der Ausschlussgründe mit angepasst werden musste — und das zuletzt nicht geschehen war.

**Betroffen:** Bot-Ban-/Blacklist-Schutz und die (noch nicht scharfgeschaltete) Engagement-AI — jeweils nur für Kanäle, in denen der Bot pausiert oder gebannt ist.

## #79 — Engagement-AI: redet nur noch in echten Partner-Kanälen

**Problem:** Die KI prüfte vor dem Antworten nur zwei Dinge: ob sie für den Kanal eingeschaltet ist und ob dort gerade Deadlock live läuft. Den Partner-Status hat sie dabei komplett übersprungen. Der Bot ist aber auch in beobachteten Nicht-Partner-Kanälen präsent (rein fürs Statistik-Tracking), und genau dort war die Engagement-AI die einzige Chat-Funktion, die trotzdem mitgeredet hat — Moderation, Promos und die frechen Auto-Antworten schweigen in solchen Kanälen längst, weil sie alle denselben Partner-Check davorhängen. Zusätzlich blieb der Engagement-Schalter eines Kanals auf „an", selbst wenn die Partnerschaft später beendet wurde.

**Geändert:** Ein Partner-Gate vor jeder Engagement-Antwort, plus ein automatischer Aufräum-Schritt beim Beenden einer Partnerschaft.

**Wie's funktioniert:** Vor jeder möglichen Antwort prüft die KI jetzt zusätzlich denselben Partner-Status, den Moderation, Promos und das Kanal-Beitreten ohnehin schon verwenden — also über eine zentrale, gemeinsame Stelle, nicht über eine eigene Sonderlogik. Durch kommen nur operativ aktive Partner: reines Beobachten, ein Admin-Opt-out oder eine pausierte bzw. beendete Partnerschaft zählen bewusst nicht. Trifft das nicht zu, bleibt sie still — selbst wenn ihr Schalter (noch) auf „an" steht. Als zweite, unabhängige Sicherung wird beim Beenden einer Partnerschaft der Engagement-Schalter des Kanals automatisch ausgeschaltet, genau wie es dort schon mit der Raid-Berechtigung passiert; so bleibt kein „verwaister An-Zustand" in der Datenbank zurück. Damit greifen zwei Schichten: Das Gate fängt jede einzelne Nachricht in Echtzeit ab, der Aufräum-Schritt hält den gespeicherten Zustand sauber — fällt eine Schicht aus, schützt die andere weiter.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #78 — Engagement-AI: lockerer Stammgast statt Deadlock-Roboter

**Problem:** Nach der Themen-Eingrenzung war die KI zu steif — sie redete fast nur noch in ausformulierten Deadlock-Takes und ging auf lockeren Chat-Banter gar nicht mehr ein. Das wirkte wieder nach Bot, nur andersrum.

**Geändert:** Die KI ist wieder ein lockerer Stammgast, der mit dem Chat vibet — und sie schreibt kürzer und trockener, an einem echten Vorbild ausgerichtet.

**Wie's funktioniert:** Deadlock bleibt ihre Stärke und sie ist nur in Deadlock-Streams aktiv, aber sie zwingt das Thema nicht mehr in jede Nachricht: Banter, Reaktionen und Mitlachen sind wieder erlaubt, solange es zur Runde passt. Nachrichten, die klar an den Streamer gerichtet sind, lässt sie weiter aus, und bei fremden Themen spielt sie sich nicht als Experte auf. Für den Schreibstil dient jetzt der echte Chat-Ton eines Stamm-Streamers als Gold-Vorlage (kurz, trocken, viel Banter, oft nur ein paar Wörter) — diese Beispiele stehen im Stil-Vorbild immer ganz vorne, sodass die KI in diesem Register schreibt statt in langen Absätzen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #77 — Engagement-AI: schweigt, wenn kein Deadlock läuft

**Problem:** Die KI hat auch in Streams geantwortet, die gerade gar kein Deadlock zeigten — bei „Just Chatting", anderen Spielen oder wenn der Kanal offline war. Sie hing nur am Chat-Inhalt, nicht daran, was im Stream tatsächlich lief.

**Geändert:** Ein Stream-Gate vor allem anderen: Die KI wird pro Kanal nur aktiv, wenn der gerade live ist UND als Kategorie Deadlock läuft.

**Wie's funktioniert:** Der Bot pflegt ohnehin für jeden Streamer einen Live-Status samt aktueller Spiel-Kategorie. Bevor die KI überhaupt über eine Antwort nachdenkt, prüft sie diesen Status (kurz zwischengespeichert, damit das nicht jede Nachricht die Datenbank trifft): Ist der Kanal offline oder läuft etwas anderes als Deadlock, bleibt sie komplett still — egal wie deadlock-lastig eine einzelne Chat-Nachricht klingt. So redet sie nur dort, wo wirklich gerade Deadlock gestreamt wird.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #76 — Engagement-AI: nur noch Deadlock, kürzer, mit Streamer-Gespür

**Problem:** Im Mehr-Kanal-Test fiel auf, dass die KI sich überall einmischte — bei Begrüßungen, Resubs, Smalltalk, sogar bei Nachrichten, die klar an den Streamer gerichtet waren. Das wirkte aufdringlich und nach Bot. Dazu zwei technische Sachen: In belebten Chats (mehrere Leute gleichzeitig) brach jede Antwort mit einem Schnittstellen-Fehler ab, und die KI hatte keinerlei Hintergrundwissen über den einzelnen Streamer.

**Geändert:** Klare Eingrenzung aufs Thema, knappere Antworten, ein Fix für den Mehr-Personen-Fehler und ein wachsendes Profil pro Streamer.

**Wie's funktioniert:** Die KI antwortet jetzt nur noch, wenn es im Chat tatsächlich um Deadlock geht (Helden, Matches, Plays, Meta, Patches) — bei reinem Smalltalk, Subs, Begrüßungen oder Off-Topic bleibt sie still. Ist eine Nachricht erkennbar an den Streamer oder eine bestimmte Person gerichtet, hält sie sich raus, weil das nicht ihre Nachricht ist. Antworten sind kürzer und haben mehr Kante statt abwägender Absätze, und auf reine Emotes reagiert sie gar nicht. Der Mehr-Personen-Fehler lag daran, dass der Sprechername in einem separaten Feld mitgeschickt wurde, das die KI-Schnittstelle bei wechselnden Namen ablehnte — jetzt steht der Name direkt im Nachrichtentext, was den Verlauf bei vielen Chattern sogar klarer macht. Zusätzlich destilliert ein Hintergrund-Durchlauf alle paar Stunden pro Kanal ein kurzes Profil aus dem Chat (welche Helden der Streamer spielt, sein Hintergrund, der Community-Vibe, Running-Gags) — reines Kontextwissen, das der KI hilft, sich natürlich einzufügen, ohne es je vorzulesen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #75 — Engagement-AI: Stats-Gespür + die Soul wächst mit

**Problem:** Nach dem großen Umbau fehlten dem KI-Stammgast noch zwei Sachen. Erstens hatte er kein Gefühl dafür, welcher Held gerade wirklich stark oder beliebt ist — er konnte über die Meta-Stimmung reden, aber nicht über die nackte Stärke einzelner Helden. Zweitens war seine Persönlichkeit statisch: Er konnte sich nichts aus laufenden Gesprächen merken.

**Geändert:** Zwei neue Bausteine — echte Spielstatistiken als Stärke-Anhaltspunkt pro Held, und ein „Gedächtnis", das sich aus den Gesprächen selbst füllt.

**Wie's funktioniert:** Wird ein Held erwähnt, zieht die KI dessen aggregierte Win- und Pick-Rate aus der Statistik-Schnittstelle — aber bewusst nur als grobes Gefühl („Winrate über 50%, wird oft gespielt") statt als Zahlen-Tabelle zum Vorlesen. So weiß sie, ob ein Held gerade meta ist. Parallel läuft alle paar Stunden ein Reflexions-Durchgang: Die KI schaut sich die letzten Chats an, in denen sie mitgemischt hat, und merkt sich — wenn etwas hängen blieb (ein gutes Gespräch, ein Running Gag, ein cooler Move, ein lustiges Wort) — eine kurze Notiz. Diese Notizen hängen unter ihrer Grund-Persönlichkeit und werden später nur beiläufig aufgegriffen, wenn es gerade passt — nicht ausgepackt. So fühlt sich der Charakter mit der Zeit lebendiger an, statt jeden Tag bei null zu starten.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #74 — Engagement-AI: großer Qualitäts-Umbau gegen „klingt wie ein Bot"

**Problem:** Der KI-Stammgast, der sich in den Twitch-Chat einklinken soll, hatte zwei Kernprobleme. Erstens halluzinierte er Deadlock-Fakten — im Test erfand er ein Item samt frei erfundener Mechanik. Zweitens klang er unverkennbar nach KI: Floskeln wie „gute Frage, das kann ich gerade nicht belegen", reflexhaftes Zustimmen zu jeder Meinung, steife ganze Sätze statt Chat-Sprache.

**Geändert:** Der Antwort-Aufbau wurde von Grund auf umgebaut. Die KI bekommt jetzt echte Fakten vorgesetzt und bildet sich darauf eine Meinung, statt aus dem Gedächtnis zu raten — und sie hat einen festen Charakter mit eigenem Geschmack.

**Wie's funktioniert:** Vor jeder Antwort werden mehrere Faktenquellen zusammengezogen und der KI als Beleg mitgegeben: (1) Helden-/Item-Beschreibungen aus dem Deadlock-Wiki, (2) ein Stimmungsbild, das laufend aus den echten Chat-Nachrichten *aller* mitgelesenen Streams destilliert wird („wie fühlt sich die Meta gerade an"), und (3) die echten Änderungen des aktuellen Patches direkt aus den offiziellen Notes. Die KI darf nur über das reden, was in diesen Belegen steht; fehlt ein Beleg, trifft sie keine faktische Aussage, sondern weicht locker aus oder schweigt — statt einen Disclaimer abzulassen. Dazu kam ein fester Charakter („Soul"), den das Modell sich selbst geschrieben hat, plus eigene, aus allen 38 Helden samt Fähigkeiten gebildete Lieblings- und Hass-Helden — die liefern aber nur die Meinung, nicht den Ton: im Chat bleibt die KI knapp und trocken. Eiserne Regeln unterbinden das KI-Gehabe: nie zugeben, eine KI zu sein, nie über die eigene Funktionsweise reden, nicht jeder Meinung hinterherlaufen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI; am restlichen Bot ändert sich nichts.

## #73 — Highlight-Clips landen jetzt im dedizierten Thread

**Problem:** Der Highlight-Clipper hat fertige Gameplay-Clips in den allgemeinen Bot-Log-Kanal gepostet, wo sie zwischen anderen Bot-Meldungen untergingen.

**Geändert:** Ziel-Kanal auf den dedizierten Highlight-Thread umgestellt.

**Wie's funktioniert:** Der Clipper schickt fertige Clips per interner HTTP-API an den Deadlock-Bots-Prozess, der den Discord-Post übernimmt. Der API-Payload enthält eine `channel_id` — die zeigt jetzt auf den Thread. Discord-Threads verhalten sich aus Bot-Sicht identisch zu normalen Kanälen (gleiches `send()`-Interface), daher war keine weitere Code-Änderung nötig.

## #72 — Interne Blacklist-API komplett: Check, List und Remove

**Problem:** Nach dem ersten Blacklist-Endpunkt (`/add`) fehlten noch die restlichen Operationen — prüfen ob ein Kanal gebannt ist, alle gesperrten Kanäle auflisten und einen Bann wieder aufheben.

**Geändert:** Drei neue Endpunkte auf demselben internen API-Port (8776, nur localhost):

- `GET /raid/blacklist/check?login=kanalname` — antwortet mit `blacklisted: true/false` plus Grund und Zeitstempel falls vorhanden
- `GET /raid/blacklist` — gibt alle aktuell gesperrten Kanäle sortiert nach Eintragsdatum zurück
- `POST /raid/blacklist/remove` — entfernt einen Kanal aus der Blacklist, `removed: true` wenn er tatsächlich drin war

**Wie's funktioniert:** Alle drei Operationen laufen über direktes SQL auf `twitch_raid_blacklist`, in `asyncio.to_thread` gekapselt damit der Event-Loop nicht blockiert. Auth und Loopback-Schutz identisch zu `/add`. Die Endpunkte sind rein additiv — kein bestehender Code-Pfad wurde geändert.

---

## #71 — Interner API-Endpunkt: Twitch-Kanäle manuell in die Raid-Blacklist eintragen

**Problem:** Es gab keinen direkten Weg, einen Twitch-Kanal per API in die Raid-Blacklist einzutragen — der einzige Pfad war ein manuelles SQL-Insert in die Datenbank, was jedes Mal erforderte, den DSN-Secret in eine Shell zu laden.

**Geändert:** Neuer interner HTTP-Endpunkt `POST /internal/twitch/v1/raid/blacklist/add`. Akzeptiert `login` und optional `reason` im JSON-Body. Trägt den Kanal direkt per `ON CONFLICT`-Insert in `twitch_raid_blacklist` ein — idempotent, d.h. ein wiederholter Aufruf überschreibt nur Grund und Zeitstempel, legt keinen Duplikat-Eintrag an.

**Wie's funktioniert:** Der Endpunkt hängt in der bestehenden internen API auf Port 8776, die ausschließlich auf `127.0.0.1` lauscht und durch zwei Middleware-Schichten abgesichert ist: eine Loopback-Prüfung (Verbindungen nur von localhost) und eine Token-Auth via `X-Internal-Token`-Header. Port 8776 ist zusätzlich nicht in der UFW-Allowlist — von extern ist der Port komplett unerreichbar. Die DB-Operation läuft in einem Thread-Pool (`asyncio.to_thread`), damit der async Event-Loop nicht blockiert.

---

## #70 — Viewbot-Spam-Filter: trennscharf gegen Verschleierung, sicherer für echte Zuschauer

**Problem:** Die Viewbot-/SMM-Werbung im Chat (Marke „Ai viewers streamboo. com" und Verwandte) rutschte zuletzt durch, obwohl der Filter die Marke kannte. Der Trick der Spammer: Sie streuen Leerzeichen und Sonderzeichen in die Domain (`streamboo. com`, `s t r e a m b o o . c o m`), wodurch die wörtliche Mustererkennung ins Leere lief — der Treffer reichte nur für „verdächtig", nicht für eine Aktion, und derselbe Spam kam tagelang immer wieder. Gleichzeitig steckte ein latentes Fehlban-Risiko im Filter: generische Wendungen wie „best viewers" zählten so stark, dass ein normales Kompliment an den Streamer („you have the best viewers today") rechnerisch die Ban-Schwelle erreichen konnte.

**Geändert:**
- **Verschleierungs-robuste Erkennung:** Bekannte Spam-Domains werden jetzt erkannt, egal wie viele Leerzeichen oder Trenner dazwischenstehen.
- **Hart/Weich-Trennung:** Signale werden unterteilt in „hart" (echte Spam-Domain/Markenname — kommt in normalem Chat praktisch nie vor) und „weich" (Alltagswörter wie „viewers", „best viewers"). Nur harte Signale lösen automatische Maßnahmen aus.
- **Selbstlernend in beide Richtungen:** Eine KI prüft Grenzfälle und pflegt zwei Listen — eine Spam-Liste für neue/abgewandelte Schreibweisen und eine Schutz-Liste für Fehlalarme (harmlose Wendungen, die fälschlich angeschlagen haben).
- **Generische Floskeln entschärft:** Reine Komplimente wie „best viewers" können allein keinen Bann mehr auslösen.
- **Fremdschrift-Tarnung erkannt:** Buchstaben aus anderen Alphabeten, die wie lateinische aussehen (z. B. kyrillisches „о/е/а" in „strеаmbоо"), werden vor der Prüfung auf normale Buchstaben zurückgeführt.
- **Kontext als Verstärker:** Frisch erstelltes Konto und allererste Nachricht im Kanal erhöhen den Verdacht — aber nur, wenn ohnehin schon ein echtes Spam-Signal vorliegt.

**Wie's funktioniert:** Jede Chat-Nachricht bekommt einen Spam-Score. Ab 3 Punkten wird automatisch gelöscht und gebannt; darunter passiert je nach Signalstärke nichts oder die Nachricht wird (nur bei einem harten Treffer) entfernt, ohne den Schreiber zu bannen. Punkte gibt es u. a. für eine bekannte Spam-Domain (+2) und für das Muster „viewers <Wort>" (+1) — Alltagswörter allein erreichen die Ban-Schwelle also nie. Vor der Bewertung wird der Text „geglättet": eingestreute Leerzeichen und Sonderzeichen werden ignoriert, sodass `streamboo. com` und `s t r e a m b o o` denselben Treffer ergeben wie die saubere Domain. Damit das nicht überschießt, ist die Domain-Erkennung an eine echte Endung gekoppelt (`.com`, `.ru`, …) — harmlose Wortpaare wie „laptop smm" oder „stream boo" lösen daher nichts aus.

Auch Tarnung über fremde Alphabete läuft ins Leere: Zeichen wie das kyrillische „о", das optisch identisch zum lateinischen „o" ist, werden beim Glätten auf den lateinischen Buchstaben zurückgeführt — `strеаmbоо` wird also wie `streamboo` behandelt.

Zwei zusätzliche Kontext-Punkte verschärfen den Verdacht gezielt: ein erst kürzlich erstelltes Twitch-Konto (+1) und die allererste Nachricht eines Accounts in genau diesem Kanal (+1). Beide zählen aber **nur**, wenn bereits ein hartes Spam-Signal vorliegt — ein bekannter Spam-Bot, der mit frischem Konto seine erste Nachricht absetzt, kippt damit über die Ban-Schwelle, während ein neuer echter Zuschauer mit einer harmlosen Begrüßung garantiert unberührt bleibt.

Die KI-Prüfung läuft im Hintergrund: Schlägt der Filter an, ohne sicher zu sein, bewertet die KI nach, ob es echte Viewbot-Werbung ist. Bestätigt sie es, lernt das System die neue Schreibweise und erkennt sie beim nächsten Mal sofort. Sagt die KI „kein Spam", wandert das auslösende Alltagswort auf die Schutz-Liste und senkt den Score solcher Nachrichten künftig wieder — **außer** es kommt zusätzlich ein hartes Spam-Signal hinzu. Diese Vorrang-Regel verhindert, dass jemand ein harmloses Wort an eine echte Spam-Domain anhängt, um den Filter auszuhebeln.

**Für Zuschauer wichtig:** Automatische Banns treffen ausschließlich klar erkennbare Viewbot-Werbung mit echter Spam-Domain. Ein normales Gespräch über „viewers" oder ein Kompliment an den Streamer löst keine Maßnahme aus. Sollte trotzdem jemand fälschlich getimeoutet oder gebannt werden: Das ist sehr unwahrscheinlich, aber kein Bann ist endgültig — meldet euch beim Streamer oder einem Mod (Rückgängigmachen per `!unban`), wir schauen es uns an. Genau für solche Fälle gibt es die lernende Schutz-Liste.

---

## #69 — Chat-Actions im Admin-Dashboard vollständig repariert

**Problem 1 (Senden-Button tut nichts):** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn der Anzeigename vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce 220 ms nach dem Klick die Auswahl gecleart — `canSubmit` wurde false, der Button disabled. Fix: Vergleich jetzt gegen Login **und** Anzeigenamen.

**Problem 2 (Fehlermeldung „Twitch Chat Bot ist aktuell nicht verfügbar"):** Bot und Dashboard laufen als getrennte Prozesse. Alle Bot-Operationen (Streamer hinzufügen, entfernen, …) gehen über eine interne REST-API. Chat-Actions hatten diese Brücke nie bekommen — das Dashboard versuchte den Chat-Bot direkt anzusprechen und schlug deshalb immer fehl.

**Geändert:** Die interne API-Kette wurde um eine `partner_chat_action`-Callback-Route erweitert. Der Bot-Prozess führt die eigentliche Chat-Aktion aus (er hat Zugriff auf den Chat-Bot); der Dashboard-Prozess leitet die Anfrage über HTTP weiter. Wenn kein lokaler Chat-Bot vorhanden ist, greift das Dashboard jetzt auf diesen Callback zurück statt mit einer Fehlermeldung abzubrechen.

---

## #69 — Chat-Actions im Admin-Dashboard funktionstüchtig

**Problem:** Auf der Seite `/twitch/admin/community/chat` passierte beim Drücken von "Nachricht senden" scheinbar nichts — der Button blieb inaktiv oder die Aktion wurde nie abgeschickt.

**Ursache:** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn nach einem Klick auf einen Streamer-Vorschlag der Anzeigename (`displayName`) vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce die Auswahl 220 ms später wieder gecleart. Danach war kein Streamer mehr ausgewählt, `canSubmit` war false und der Button disabled.

**Geändert:** Die Vergleich-Logik prüft jetzt, ob der Eingabewert mit dem Login **oder** dem Anzeigenamen des ausgewählten Streamers übereinstimmt. Stimmt beides nicht, wird die Auswahl gecleart — stimmt eines, bleibt sie erhalten.

---

## #68 — Zielgerichtete Discord-Promos mit MiniMax-Preset-Auswahl

**Problem:** Alle Promo-Nachrichten waren bisher kanal-weite Announcements — gleicher Text für alle, egal ob jemand seit Monaten dabei ist oder gerade zum ersten Mal reinschaut.

**Geändert:**
- **Preset-System** (neu): 10 definierte Templates — 5 globale Announcements (competitive, community, chill etc.) und 5 user-spezifische Nachrichten mit @mention (welcome, lurker, mates, ranked, new player).
- **MiniMax-Auswahl**: MiniMax bekommt die letzten paar Chat-Messages des Ziel-Users + die Preset-Tags und entscheidet welches Template am besten passt. Kein Freitext — nur Auswahl aus vordefinierten Texten. Timeout 5s, Fallback auf Zufalls-Preset.
- **Stammgast-Ausschluss**: Wer ≥ 10 Chat-Messages in den letzten 30 Tagen hat (Stammgast), wird beim User-Targeting übersprungen. Zielgruppe: neue Chatter und stille Lurker.
- **1x pro User pro Tag**: Jeder User bekommt höchstens einmal täglich einen personalisierten Pitch.
- **Abwechslung Global/User**: Das System wechselt zwischen kanal-weiter Announcement und direktem @mention-Pitch — kein Double-Ping.
- **Vorrang + Fallback**: Targeted Promo hat Vorrang (eigener 15-Min-Cooldown). Feuert er nicht (Cooldown läuft noch), übernimmt das bestehende Activity/Spike-System.

**Wie's funktioniert:** Jeder Promo-Loop-Tick prüft pro Channel ob der targeted 15-Min-Cooldown abgelaufen ist. Wenn ja: aktive Chatter aus dem 8-Min-Aktivitätsfenster filtern, Kandidaten-Check (nicht Stammgast, nicht heute gepitcht, user_id via DB), MiniMax-Aufruf für Preset-Wahl, Message rendern + senden. @mention-Pitches gehen als normale Chat-Message, globale als farbige Announcement.

## #67 — Timeout-Erkennung über EventSub + Stream-Start-Pitch

**Problem:** Ein 10-Minuten-Timeout des Bots wurde über den bisherigen Drop-Code-Ansatz nicht erkannt, weil der Bot in dieser Zeit nichts schreibt. Außerdem kam der Werbefrei-Pitch beim nächsten Promo-Slot statt am nächsten Stream-Start. Pitch-Text war zu generisch.

**Geändert:**
- **Echtzeit-Erkennung via EventSub**: Die bestehende `channel.ban`-Subscription (feuert bei Timeout und permanentem Ban) prüft jetzt ob der Bot selbst das Ziel ist. Bei `is_permanent: false` → Timeout erkannt → TimeoutGuard wird aktualisiert.
- **Pitch zum Stream-Start**: Der Werbefrei-Pitch wird nicht mehr beim nächsten freien Promo-Slot gesendet, sondern 90 Sekunden nach dem nächsten Stream-Start – so ist der Streamer gerade frisch live und liest tatsächlich mit.
- **Klarerer Pitch-Text**: "Beim letzten Stream wurde der Bot in diesem Chat getimed outed 🙈 Falls die automatischen Promo-Nachrichten stören – es gibt ein Werbefrei-Abo…" Klar formuliert, kein Rätselraten.

**Wie's funktioniert:** Der `channel.ban`-Callback im Monitoring-Bot vergleicht die `user_id` des Gebannten mit der Bot-ID (aus `_twitch_chat_bot.bot_id`). Passt sie, trägt er den Timeout im `TimeoutGuard` (Singleton, gleicher Prozess) ein. Wenn der Streamer das nächste Mal live geht, überprüft der Go-Live-Handler ob ein Pitch aussteht, und plant ihn mit 90s Delay als asyncio-Task.

## #66 — Timeout-Schutz, Werbefrei-Pitch und schärfere Promo-Logik

**Problem:** Der Bot hat in Channels, die ihn regelmäßig timeout'en, trotzdem weitergemacht — ohne Konsequenz. Außerdem hat die Deadlock-Beta-Zugangserkennung zu viele Fehlzündungen produziert (z.B. "brauchst mitspieler?" löste fälschlicherweise einen Invite-Hinweis aus). Die Promo-Nachrichten wiederholten sich zu schnell und kamen zu oft.

**Geändert:**
- **Timeout-Tracking** (neu): Wenn der Bot in einem Channel getimed outed wird (Twitch meldet `sender_banned` beim nächsten Send-Versuch), wird das gezählt. Bei 2x an einem Tag oder 5x in einer Woche werden **alle Bot-Funktionen in diesem Channel für 7 Tage deaktiviert**.
- **Werbefrei-Pitch**: Nach einem erkannten Timeout schickt der Bot (sobald er wieder darf) einmalig einen Hinweis auf das Werbefrei-Abo, das Promo-Nachrichten abschaltet.
- **Beta-Zugang-Fix**: "Mitspiel\*"-Signale lösen nur noch einen Invite-Hinweis aus, wenn gleichzeitig ein starkes Zugangs-Signal ("beta", "zugang", "key" etc.) oder expliziter Deadlock-Bezug vorliegt. Damit fällt "moin, brauchst mitspieler?" nicht mehr in die Kategorie.
- **Mehr Nachrichten-Vielfalt**: Der Promo-Pool wuchs von 14 auf 22 Texte (aufgeteilt in 5 Kategorien). Zusätzlich wird die zuletzt gesendete Nachricht beim nächsten Mal ausgeschlossen.
- **Höhere Cooldowns**: Minimaler Promo-Cooldown von 30 auf 45 Min, maximaler von 120 auf 180 Min, globaler Cooldown von 60 auf 90 Min, Attempt-Cooldown von 5 auf 10 Min.

**Wie's funktioniert:** Der `TimeoutGuard` hält eine In-Memory-Liste der Timeout-Zeitpunkte pro Channel (rollendes 7-Tage-Fenster). Überschreitet die Tageszahl ≥ 2 oder die Wochenzahl ≥ 5, wird der Channel für 604.800 Sekunden stumm geschaltet — kein Discord-Post, kein Raid, kein Bot-Ban, keine Promos, nichts. Der Werbefrei-Pitch erscheint maximal einmal pro 24 Stunden pro Channel.

## #65 — Globale Chatter-Bannliste + Discord-Invite-Erkennung

- Bestimmte Nutzer können jetzt global gesperrt werden und werden in jedem Partner-Kanal sofort gebannt, sobald sie dort schreiben
- Discord-Invite-Links (discord.gg/...) werden automatisch im Log als verdächtig markiert
- Normales Reden über Discord oder "mitspielen" wird davon nicht berührt

## #64 — Highlight-Clips zeigen jetzt echte Combos aus dem Replay

- Clips werden ab jetzt nur noch für echte Combo-Kills erstellt — wenn der Spieler kurz vor dem Kill mindestens 2 Abilities eingesetzt hat (z. B. Hook → Sticky Bomb → Uppercut)
- Solo-Kills ohne Combo-Bewegung werden automatisch herausgefiltert
- Combo-Label erscheint im Discord-Clip-Embed (z. B. "Hook → Bomb → Uppercut")
- Für jedes Match wird das Valve-Replay (227 MB) automatisch geladen und mit dem Open-Source-Tool "boon" analysiert — kein manueller Eingriff nötig

## #63 — Highlight-Erkennung deutlich smarter

- Close Situations werden jetzt erkannt: wenn ein Kill und ein eigener Tod nah beieinander liegen (z. B. Kill dann 3 Sekunden später gestorben), wird das als Highlight gewertet
- Teamfights werden jetzt auch geclippt wenn der Spieler nur einen Kill gemacht hat (nicht mehr min. 2 nötig)
- Einzelne, isolierte Kills werden weiterhin nicht geclippt

## #62 — Highlight-Clips zeigen jetzt echte Highlights

- Einzelne Kills werden nicht mehr als Clips verschickt — das war uninteressantes 0815-Gameplay
- Nur noch Double Kills, Triple Kills und Team Fights werden geclippt
- Neue Bezeichnungen: Double Kill, Triple Kill, Quadra Kill statt generischem "Multi Kill"

## #61 — Highlight-Clips landen jetzt wirklich im Discord-Channel

- Clips wurden erstellt aber konnten nicht gesendet werden — Fehler beim Discord-Zugriff behoben
- Clips werden jetzt über den Deadlock-Bot in den Highlight-Channel gepostet

## #60 — Highlight-Clipper: Clip-Download funktioniert jetzt zuverlässig

- Clips konnten bisher wegen eines ffmpeg-Crashes nicht erstellt werden — der Fehler ist behoben
- yt-dlp nutzt jetzt das stabile System-ffmpeg statt einer statischen Binary die abstürzte
- VODs mit eingeschränkten Qualitätsstufen (z. B. Sub-Only-Streams) werden trotzdem verarbeitet

## #59 — Highlight-Clipper funktioniert jetzt für alle Partner-Streamer

- Der automatische Clip-Ersteller lief bisher gar nicht — Twitch-Zugangsdaten wurden falsch gesucht und nie gefunden
- Clips werden jetzt automatisch für alle 18 aktiven Partner-Streamer erstellt, die ihren Steam-Account bereits im Discord verknüpft haben
- Steam-Accounts werden direkt aus der Discord-Verknüpfung übernommen — kein separates Einrichten nötig
- Fertige Clips landen in einem Discord-Channel zur Durchsicht

## #58 — Streamer-Seite lädt jetzt blitzschnell ohne weißen Flash

- Die Streamer-Seite wird beim Build jetzt vollständig als fertiges HTML vorgerendert — Besucher sehen die Seite sofort, ohne kurzen weißen Ladeblitz
- Kein sichtbarer Unterschied für normale User, aber Suchmaschinen und KI-Crawler sehen exakt dasselbe wie echte Besucher — keine versteckten Tricks mehr

## #57 — Streamer-Landingpage komplett SEO-fähig gemacht

- Die Streamer-Seite hatte vorher nur einen leeren Titel ("Twitch-Bot") und keinen lesbaren Inhalt für Google — deshalb hat sie Google bisher gar nicht gecrawlt
- Jetzt mit klarem Titel ("Deadlock Auto-Raid Bot für Twitch — Streamer-Netzwerk"), Hero-Text, Feature-Liste, So-funktioniert-es-Schritten und FAQ-Block, die alle Suchmaschinen direkt lesen können
- Strukturierte Daten (Schema.org) für die Web-App, Breadcrumbs und FAQ ergänzt — damit Google AI Overviews, Bing Copilot und Perplexity die Seite zitieren können
- Vollständige Social-Media-Vorschau (Open Graph + Twitter Card) für Discord, Twitter und WhatsApp ergänzt — Links zeigen jetzt Vorschaubild und Beschreibung
- Statischer Inhalt bleibt für Crawler ohne JavaScript (Bing, Brave, DuckDuckGo) dauerhaft sichtbar, normale Besucher sehen die interaktive Version wie gewohnt

## #56 — Twitch Admin Dashboard komplett neu strukturiert

- Neues Admin-Cockpit auf `/twitch/admin` mit Live-Status auf einen Blick: aktive EventSub-Verbindung, ausstehende OAuth-Reauths, Bot-Uptime und Datenbank-Status
- Sidebar in fünf klare Bereiche gegliedert: Cockpit, Operations, Community, Content & Comms, Money & Compliance — keine endlose flache Linkliste mehr
- Globale Streamer-Suche oben in der Top-Bar — Login eintippen, Enter, direkt in der Detail-Ansicht
- Neue Bereiche im Dashboard: OAuth-Scope-Diff pro Streamer, Bot-Control (Reload, Promo-Mode), Engagement-AI-Steuerung, Chat-Aktionen senden, Roadmap- und Legal-Editor mit Vorschau, Audit-Log über alle Admin-Aktionen
- Das alte Legacy-Dashboard leitet jetzt automatisch auf die neue Oberfläche um — Bookmarks bleiben funktionieren

## #55 — Lade-Bildschirm für Rechtstexte vereinfacht

- Die Bot-Prüfseite zeigt jetzt nur noch einen Spinner mit „Einen Moment bitte …" und einem dezenten Hinweis
- Kein sichtbares Captcha, keine Erklärungstexte mehr — für normale Besucher wirkt es wie ein kurzer Ladevorgang

## #54 — Impressum & Datenschutz jetzt Bot-geschützt, Dashboard erweitert

- Impressum, Datenschutz und AGB sind jetzt durch eine unsichtbare KI-Bot-Sperre geschützt — normale Besucher werden automatisch weitergeleitet, ohne ein Captcha zu sehen
- Admin-Dashboard: Inhalte der Rechtstexte (Impressum, AGB, Datenschutz) und der Roadmap können direkt im Dashboard bearbeitet werden
- Admin-Dashboard: Neue Analytics-Übersicht und überarbeitete Navigation mit erweitertem Sidebar-Menü
- Zugriffs-Log des Dashboards wird jetzt in eine eigene Datei geschrieben (rotierend, max. 5 MB)

## #53 — Spam-Filter präziser und resistenter gegen Unicode-Tricks

- Spam-Schreibweisen mit verschlüsselten Buchstaben (z. B. `sᴛʀᴇᴀᴍbᴏᴏ`, `𝗩𝗶𝗲𝘄𝗲𝗿𝘀`) werden jetzt korrekt erkannt und gebannt
- Die KI-Überprüfung startet nur noch bei echten Spam-Signalen (Phrase, Fragment oder Spam-Domain) — harmlose Viewer-Erwähnungen werden nicht mehr unnötig geprüft
- Von der KI bestätigte Muster fließen sofort als vollwertige Filter-Bedingung in die Spam-Bewertung ein — gleicher Score-Mechanismus wie für manuell gepflegte Einträge

## #52 — Auto-Ban lernt jetzt selbst neue Spam-Muster dazu

- Nachrichten die verdächtig wirken aber noch nicht gebannt werden, prüft MiniMax M2.7 automatisch im Hintergrund
- Wird Spam bestätigt, merkt sich der Bot das Kernmuster (Domain, Phrase, Keyword) dauerhaft in der Datenbank
- Zukünftige Nachrichten mit demselben Muster werden direkt erkannt und gebannt — kein manuelles Nachtragen mehr nötig
- Auch Score-0-Nachrichten mit URLs oder Viewer-Keywords werden geprüft (wie der Miracle-Ghost-Fall)

## #51 — AI-Engagement pro Kanal im Admin-Dashboard steuerbar

- Im Streamer-Detail des Admin-Dashboards gibt es jetzt einen Toggle für den AI-Engagement-Chatter
- Der Schalter zeigt den aktuellen Status (Aktiv/Inaktiv) und wer ihn zuletzt geändert hat
- Änderungen greifen sofort — kein Reload nötig

## #50 — Streamer-Dashboard und Clip-Workflow zentral dokumentiert

- Tarife (Free, Raid Boost, Werbefrei, Analyse Dashboard, Bundles), Free-vs-Paid-Cutoffs, Testphase und Kündigung sind jetzt klar erklärt
- Auch der Clip-Workflow (Approval, Upload, Retention) und das Affiliate-Portal sind dokumentiert — Streamer- und Viewer-Fragen dazu werden vom FAQ-Bot im Discord direkt beantwortet

## #49 — KI-Analyse besser auffindbar im "Was tun?"-Bereich

- Die KI-Analyse ist jetzt ein eigener Reiter neben "Pro Session" und "Empfehlungen" — vorher lag sie ganz unten am Seitenende und war kaum zu finden

## #48 — Analyse-Dashboard aufgeräumt: 14 Tabs werden zu 7

- Statt 14 Tabs gibt es jetzt 7 klar getrennte Bereiche — kein langes Suchen mehr in einer überfüllten Leiste
- Coaching, KI-Analyse und Stream-Reports sind im neuen Bereich "Was tun?" gebündelt — alle Empfehlungen an einem Ort
- Audience, Viewer und Chat liegen jetzt zusammen unter "Publikum"
- Wachstum, Vergleich und die Kategorie-Leaderboards sind unter "Wachstum" zusammengefasst
- Alte gespeicherte Links zu einzelnen Tabs funktionieren weiterhin

## #47 — Analytics-Tour komplett überarbeitet: 5 Schritte durch echte Dashboard-Features

- Tour zeigt jetzt die KPI-Kacheln direkt (Viewer, Follower, Retention) statt nur Tab-Buttons
- Neuer Schritt erklärt das KI-Insights-Panel und was der Bot analysiert
- Wachstum, Chat und Coaching als eigene Schritte mit besseren Erklärungen
- Tour startet jetzt zuverlässig — alter "gesehen"-Status wird beim Seitenwechsel automatisch zurückgesetzt

## #46 — Onboarding-Tour: Übergabe zur Abo-Seite zuverlässiger + Button-Label korrigiert

- Letzter Button im Dashboard-Onboarding heißt jetzt "Zur Abo-Seite" statt "Fertig"
- Beim Wechsel zur Abo-Seite wird die alte "Tour gesehen"-Markierung automatisch gelöscht — Tour startet immer korrekt
- Kein manuelles Löschen von Browserdaten mehr nötig

## #45 — Tour-Robustheit: fehlende Anker überspringen ohne permanente Deaktivierung

- Wenn eine Tour beim Start keine Elemente im DOM findet, wird sie nicht mehr dauerhaft als "gesehen" markiert
- Bisheriges Verhalten: Tour hat sich selbst deaktiviert, obwohl der Nutzer sie nie gesehen hat
- Tour wird beim nächsten Seitenaufruf korrekt nochmal angezeigt, sobald die Seite vollständig geladen ist

## #44 — Pricing-Tour Bugfix: richtige Anker im FeaturePicker

- Pricing-Tour wurde sofort übersprungen weil die Anker auf dem falschen Komponent lagen (PlanCardRedesign statt FeaturePicker)
- Tour-Kacheln zeigen jetzt die drei echten Features: Werbefrei, Raid Boost und Analyse
- Preview-Version: Tour startet erst wenn Pläne vom Server geladen sind (kein Frühstart mit leerem DOM)

## #43 — Automatische Onboarding-Tour durch Dashboard, Pläne und Analytics

- Neue Nutzer werden nach der Dashboard-Tour automatisch zur Abo-Seite weitergeleitet
- Auf der Abo-Seite erklärt eine Spotlight-Tour die drei Plan-Stufen (Free, Basic, Extended) mit je einer kurzen Erklärung
- Nach der Pricing-Tour geht es direkt weiter zum Analyse-Dashboard mit einer eigenen Tour
- Die Analytics-Tour zeigt die wichtigsten Tabs: Übersicht, Streams, Wachstum und Chat
- Alle drei Touren können einzeln übersprungen werden und zeigen sich nur beim ersten Besuch

## #42 — Pricing-Seite überarbeitet: Vergleich korrigiert & Auswahl klarer

- Feature-Vergleichstabelle zeigt jetzt echte Plan-Spalten: Free / Werbefrei / Raid Boost / Analyse / Alles drin — statt ungenauer Tier-Labels
- „Bot-Werbung deaktivieren" steht jetzt korrekt beim Werbefrei-Plan, nicht erst beim Bundle
- Trial-Plan zeigt keine Chat-Werbung-Deaktivierung mehr (war schon im Backend korrekt, ist jetzt auch im Vergleich sichtbar)
- Feature-Kacheln auf der Pricing-Seite haben jetzt einen klaren Hinweis „+ Auswählen" und einen erklärenden Titel darüber

## #41 — Bundle-Preise angepasst

- Werbefrei + Raid Boost: 5,99 → 6,99 €/Mo. (geringerer Rabatt, da Raid Boost von anderen Streamern abhängt)
- Werbefrei + Analyse: 11,49 → 10,49 €/Mo. (2 € Rabatt statt bisher 99 ct)
- Alles drin und Analyse + Raid Boost bleiben unverändert

## #40 — AGB: Name und Links korrigiert

- Name „EarlySalty / Deadlock Partner Network" → „Deutsche Deadlock Community" in AGB und Seitentitel
- Zurück-Button und Footer-Link auf der AGB-Seite zeigen jetzt korrekt auf `/twitch/pricing`
- Kündigung in §5 verlinkt jetzt auf das Dashboard statt auf die veraltete Abo-Seite

## #39 — Jahresplan: Sofortabbuchung, 14 Monate Zugang und Rechtssicherheit

- Jahrestarif wird jetzt direkt beim Kauf abgebucht (kein Trial-Zeitraum mehr)
- Käufer eines Jahresabos erhalten automatisch 2 Bonusmonate on top — insgesamt 14 Monate Zugang
- Widerrufsbelehrung nach § 356 Abs. 5 BGB direkt im Checkout sichtbar und bestätigungspflichtig
- AGB aktualisiert: neuer Abschnitt zu sofortiger Leistungserbringung, Jahresbonus klar beschrieben, „6-Monats"-Option entfernt
- „30 Tage kostenlos testen"-Button auf der Pricing-Seite ist jetzt vollständig funktional (einmalig pro Account)
- Feature-Picker auf der Pricing-Seite ersetzt die alten Plan-Karten

## #38 — Neue Abo-Pläne, Jahrestarif und überarbeitete Pricing-Seite

- Zwei neue Bundle-Pläne: „Werbefrei + Analyse" (11,49 €/Mo.) und „Alles drin" (13,99 €/Mo.)
- Jahrestarif (12 Monate) mit 20 % Rabatt auf allen bezahlten Plänen wählbar
- Pricing-Seite vollständig überarbeitet: übersichtliches Toggle Monatlich/Jährlich, keine Informations-Überflutung
- Alte `/twitch/abbo`-Seite leitet dauerhaft auf `/twitch/pricing` weiter
- Stripe-Produkte und -Preise für die neuen Bundles automatisch angelegt

## #37 — Security-Scanner-Alerts bereinigt

- 271 offene Code-Scanning-Alerts (Semgrep + CodeQL) vollständig bearbeitet
- Echte Bugs behoben: undefinierte Variable in Tests, ungenutzte Variable, Lambda-Zuweisung
- Alle False-Positive-Alerts mit korrekten Suppression-Kommentaren versehen (`# nosemgrep`, `# lgtm[...]`)
- 4 Discord-Snowflake-ID-Alerts als False Positive über GitHub API dismissed

## #36 — Werbefrei-Plan für 3,99 € + besseres Onboarding und FAQ

- Neuer Plan „Werbefrei" für 3,99 €/Monat: Die Bot-Discord-Einladung in deinem Chat ist dauerhaft aus — auch wenn ein Admin gerade einen globalen Aktions-Text aktiv hat
- Combo „Werbefrei + Raid Boost" für 5,99 €/Monat (spart 2 € gegenüber Einzelkauf)
- Onboarding-Tour im Dashboard erweitert: erklärt jetzt auch was der Bot grundsätzlich macht, deinen aktuellen Plan, Werbung-Einstellungen und wo du Hilfe findest
- Neue Sidebar-Sektion „Hilfe" mit Direktlink zur FAQ und Knopf zum Neu-Starten der Tour
- FAQ um drei neue Sektionen ergänzt: „Was macht der Bot eigentlich?", „Pläne & Preise" und „Chat-Werbung"

## #35 — Clips ohne Alterswarnung und Twitch-Embeds funktionsfähig

- Clips von xradoo_ und miracleghost9 ersetzt — beide hatten eine Twitch-Alterswarnung, die das Einbetten verhinderte
- Ersetzt durch Clips von friduzockt und einsbezi aus der Community
- Caddy-Konfiguration repariert: Twitch-Embeds auf der /streamer/-Seite wurden durch eine zu restriktive Content-Security-Policy geblockt — jetzt erlaubt
- Demo-Dashboard kann wieder in die Streamer-Website eingebettet werden (gleiches CSP-Problem behoben)

## #34 — Branding der öffentlichen Website vereinheitlicht

- Titel und Link-Vorschau zeigen jetzt „Deutsche Deadlock Community" statt „EarlySalty"
- Kaputte Clips (gelöschte Medal-Links, tote EarlySalty-Clips) durch echte Community-Clips ersetzt
- Favicon auf allen Seiten einheitlich — EarlySalty-„E" im Bot-Favicon entfernt
- Logos über alle Projekte auf denselben Stand gebracht
- „DDC"-Abkürzung im gesamten öffentlichen Website-Text durch ausgeschriebenen Namen ersetzt

## #33 — Dashboard zeigt Twitch-Auth nach Erstanmeldung korrekt als aktiv

- AutoRaid-Status wird nach der ersten Twitch-Autorisierung sofort als aktiv angezeigt
- Bisher blieb das Dashboard auf „inaktiv", obwohl die Auth korrekt gespeichert war und Raids funktionierten
- Betrifft nur neue Streamer beim allerersten Auth-Vorgang, nicht Re-Auth

## #32 — Bot-Absturz beim Start behoben

- Twitch-Bot und Dashboard starten wieder fehlerfrei
- Fehler trat auf, weil eine neue Storage-Funktion intern vergessen wurde zu verknüpfen

## #31 — GitHub Actions Minutenverbrauch deutlich reduziert

- Fünf tägliche Security-Workflows auf reine Event-Trigger umgestellt (kein Schedule mehr)
- Security-Scans und Secret-Scanning laufen jetzt nur noch bei Push und Pull Request
- Semgrep bricht den Build nicht mehr bei jedem Fund ab — Ergebnisse werden als Artifact gespeichert

## #30 — Sicherheitslücke in Test-Abhängigkeit geschlossen

- pytest in der CI-Test-Pipeline auf Version 9.0.3 angehoben — schließt eine Privilege-Escalation-Lücke über das /tmp-Verzeichnis

## #29 — Security-Scan: 390 Alerts bereinigt

- Sensible Werte (User-IDs, Streamer-Logins, Dateipfade) werden jetzt überall vor dem Logging bereinigt — verhindert Log-Injection
- Discord-Nutzer-IDs im Code sind als öffentliche IDs markiert, nicht als Secrets
- Rund 350 False-Positive-Alerts aus dem Semgrep-Scanner (SQL-Queries, Logger-Credential, HTML-Format, Dynamic-Imports) wurden mit präzisen Suppression-Kommentaren ausgestattet, damit echte neue Probleme künftig auffallen
- 63 unbenutzte Imports, 41 unbenutzte Variablen und weitere kleine Stil-Verstöße automatisch bereinigt

## #28 — Hintergrund-Konsolidierungen (Audit-Cleanup Phase 2)

- Streamer-Plan- und Billing-Lookups laufen jetzt über eine gemeinsame Quelle — ein Test, der wegen Schema-Drift rot war, ist wieder grün
- Schnellere Streamer-Suche im Dashboard durch zwei neue Datenbank-Indexe für case-insensitive Logins
- Datenbank-Pool gehärtet: Default-Verbindungen 4 → 10, Connect-Timeouts gesetzt, automatischer Retry bei seltenen Postgres-Deadlocks
- Internal-API-Server -319 Code-Zeilen schlanker (doppelte Helper-Logik in policy.py konsolidiert)
- Doku auf Stand gebracht: korrekte Routen-Übersicht in INDEX/API.md, Stream-Report-Sektion ergänzt, veraltete „geplant"-Marker entfernt

## #27 — Stabilität verbessert (Audit-Cleanup Phase 1)

- Social-Media-Uploads laufen nicht mehr in endlose Wartezeiten, wenn TikTok-, Instagram- oder Login-Provider hängen — alle externen Calls haben jetzt feste Timeouts
- Bot-Reload entfernt nicht mehr versehentlich noch laufende Hintergrund-Module aus dem Speicher — weniger sporadische Crashes nach Cog-Reloads
- Doppelte HTTP- und KI-Client-Initialisierungen zusammengezogen, künftige Wartung einfacher
- Test- und Linter-Konfiguration vereinheitlicht, fehlende Dependency `cryptography` korrekt deklariert

## #26 — KI Chat-Analyse (MiniMax Deep) funktioniert jetzt

- "Analyse starten"-Button im Chat-Analytics-Dashboard war kaputt — der Backend-Endpoint crashte sofort
- Ursache: fehlendes `import json` im Backend-Modul
- Zusätzlich: TypeScript-Buildfehler behoben (fehlende `streamer`-Prop und Typcast im Donut-Chart)
- Dashboard neu gebaut und Bot neu gestartet

## #25 — Security-Deep-Scan verschlankt, kein Sicherheitsverlust

- Python-Security-, JavaScript-Security- und Semgrep-Scans aus dem Deep-Scan entfernt — diese laufen bereits täglich in der Security-Fortress
- Deep-Scan fokussiert sich jetzt auf Trivy-Filesystem-Scan und OSSF-Scorecard — beides hat keinen Doppelläufer
- Weniger doppelte CI-Minuten, gleiche Abdeckung

## #24 — CI-Laufzeiten optimiert, kein Sicherheitsverlust

- Security-Scans (Container, IaC, Supply-Chain) laufen jetzt wöchentlich statt täglich — Schutz bleibt vollständig durch Push-/PR-Trigger
- Security-Incident-Automation läuft jetzt täglich statt alle 6 Stunden — 75 % weniger Runs
- Dependency-Review hat keinen sinnlosen Tages-Schedule mehr

## #23 — CI-Artifacts werden nach 30 Tagen automatisch gelöscht

- Alle automatisch erzeugten CI-Berichte (Security-Scans, Dependency-Reports, Logs) werden ab jetzt nach 30 Tagen automatisch von GitHub entfernt
- Verhindert, dass sich der GitHub-Actions-Speicher dauerhaft volläuft

# Changelog

## #22 — Werbung sensibler für neue Zuschauer + Viewer-Trigger auch bei Normalzahlen

- Discord-Einladung wird jetzt schon bei 3 Chat-Nachrichten im Fenster ausgelöst statt 5
- Viewer-basierter Trigger greift ab sofort auch wenn die Zuschauerzahl einfach auf normalem Niveau liegt — kein "Spike" mehr nötig
- Cooldown für den Viewer-Trigger auf 60 Minuten gesenkt (war 90)
- Neue Bedingung: Promo wird nur gesendet, wenn mindestens 2 Chatter im aktuellen Fenster die letzte Werbung noch nicht gesehen haben — verhindert, dass dieselbe Zuschauerschaft mehrfach dieselbe Meldung bekommt; nach 2 Stunden gilt ein Zuschauer wieder als "neu"
- Im Admin-Dashboard gibt es jetzt Felder für Start- und Endzeit der globalen Promo-Überschreibung — die zeitbefristete Steuerung funktioniert damit korrekt

## #20 — Stream-Reports: Rating-System, neues Report-Layout + Auto-Retry

- Jeder Report hat jetzt Bewertungs-Buttons (Gut / Neutral / Schlecht) mit optionalem Kommentar — direkt unter dem jeweiligen Report sichtbar
- Reports zeigen jetzt alle Analyse-Abschnitte aus dem neuen Minimax-Schema: Snapshot, Kritische Momente, Audience, Chat-Diagnose, Wachstum, Vergleich und Maßnahmen
- Keine chinesischen Zeichen mehr in Reports — Minimax bekommt jetzt eine explizite Sprachanweisung
- Fehlgeschlagene Reports werden jetzt automatisch alle 30 Minuten bis zu 3x erneut versucht
- Minimax-Anfragen brechen nach 3 Minuten automatisch ab statt ewig zu hängen

## #19 — Stream-Reports für alle Partner freigegeben

- Stream-Reports sind jetzt für alle aktiven Partner sichtbar — kein kostenpflichtiger KI-Plan mehr nötig
- Fehlerbehebung: Dashboard-Service wurde nach Code-Änderungen nicht neu gestartet, Reports haben deshalb nicht geladen

## #18 — Stream-Reports: Neues Analyse-Schema + Minimax komplett aufgedreht

- Report-Prompt komplett neu geschrieben: 5 konkrete Analyse-Aufgaben (Kritische Momente, Audience-Qualität, Chat-Diagnose, Wachstums-Signale, Ehrlicher Vergleich)
- Minimax darf jetzt deutlich mehr schreiben: Token-Limit von 6.000 auf 16.000 erhöht
- Report-Ausgabe folgt jetzt einem klaren deutschen Schema (snapshot, momente, audience, chat_diagnose, wachstum, vergleich, massnahmen)
- Fehler-Fallback passt sich dem neuen Schema an — Dashboard bricht nicht mehr bei Parse-Fehlern

## #17 — Stream-Reports: Backfill beim Start + weitere SQL-Bugfixes

- Beim Bot-Start werden automatisch die letzten 3 Sessions pro Streamer mit einem Minimax-Report nachgefüllt, falls noch keiner existiert
- Wöchentliche Titel-Insights: zweiter SQL-Bug behoben (Session-Lookup ging an falscher Spalte, Sessions wurden nie geladen)
- Bisherige Stream-Reports ohne Fehler werden nicht doppelt generiert

## #16 — Stream-Reports mit Minimax funktionieren jetzt für alle Streamer

- Nach jedem Stream-Ende erstellt Minimax automatisch einen detaillierten Report mit Viewer-Kurve, Chat-Analyse und Vergleich zu früheren Sessions
- Reports werden für alle Streamer generiert — kein kostenpflichtiger Plan mehr nötig, um die Funktion zu nutzen
- Admins können alle Reports im Dashboard einsehen, unabhängig vom Streamer-Plan
- Wöchentliche Titel-Insights waren wegen eines Datenbankfehlers kaputt — dieser ist jetzt behoben
- Tabellen für KI-Reports werden beim ersten Start automatisch angelegt, falls die Migration noch nicht gelaufen ist

## #15 — Voice-Reaction: Bot führt jetzt echte Gespräche nach Outreach & Raids

- Nach jedem Outreach und jedem Outreach-Boost-Raid kann der Bot eigenständig im Streamer-Chat antworten — wie ein echter Community-Mensch, nicht wie ein Sales-Bot
- Der Bot hört kurz in den Stream rein und reagiert sinnvoll auf das, was der Streamer gerade sagt oder im eigenen Chat schreibt
- Bei klar interessierten Streamern landet automatisch eine Discord-Notification beim Team, damit ein Mensch persönlich übernimmt
- Standardmäßig deaktiviert — wird im Staging mit Trockenlauf aktiviert, bevor live geantwortet wird
- Komplettes Audit-Log pro Konversation, damit das Verhalten manuell durchgesehen und nachjustiert werden kann

## #14 — Beta: Auto-Highlight-Clips per Discord DM (EarlySalty)

- Nach jedem Deadlock-Match werden automatisch Highlights erkannt (Triple Kill, Multi Kill, Team Fights)
- Clips werden direkt aus dem Twitch-VOD ausgeschnitten und per Discord DM gesendet
- Erkennt Multi-Kills (≥3 Kills in 10 Sek) und Team-Fights (≥4 Kills in 15 Sek)
- Prüft alle 10 Minuten auf neue Matches der letzten 24 Stunden

## #13 — Bot taucht nicht mehr in Analyse-Daten auf

- Der Bot selbst (`deutschedeadlockcommunity`) wird jetzt überall aus Chat-Statistiken und Analyse-Dashboards herausgefiltert
- Viewer-Rankings, Publikumsauswertungen und Chat-Tiefenanalysen zeigen keine Bot-Einträge mehr

## #12 — Dashboard-Login funktioniert wieder + sauberes Partner-Status-Gating

- Dashboard-Login war komplett kaputt: SQL-Query referenzierte nicht-existierende Spalten (`is_partner`, `archived_at`, `created_at`) und brach mit 503 ab. Login funktioniert jetzt wieder zuverlässig.
- Wer sich erfolgreich einloggt und nicht permanent gesperrt ist, wird automatisch wieder als aktiver Partner geführt — kein manuelles Reset mehr nötig nach Re-Auth.
- Departnered/archivierte Streamer können sich jetzt einloggen und kommen ins Dashboard, sehen aber nur Verwaltung, Pläne und Affiliate-Bereich. Analyse, Social Media und Title-Generator bleiben gesperrt bis ein gültiger Twitch-OAuth durchläuft (außer Bot-Bann oder permanenter Block).
- Verwaltung-Seite zeigt einen klaren Hinweis-Banner mit „Jetzt neu autorisieren"-Button, wenn der Partner-Status nicht aktiv ist.

## #11 — Plan-Preise um 50% gesenkt

- Raid Boost: 7,99 € → **3,99 €** pro Monat
- Analyse Dashboard: 16,99 € → **8,49 €** pro Monat
- Bundle (Analyse + Raid Boost): 22,99 € → **11,49 €** pro Monat
- 6-Monats- und 12-Monats-Tarife folgen automatisch (mit den bestehenden 10% / 20% Mehrjahresrabatten)
- Stripe-Preise synchronisiert; bestehende Abos rechnet Stripe weiter zum bisherigen Betrag ab, neue Buchungen laufen automatisch auf die halbierten Preise

## #10 — Social-Media-Dashboard ist jetzt eine eigene Seite

- Im Analyse-Dashboard gibt es keinen „Social Media"-Tab mehr; das Tooling sitzt unter der eigenen URL `https://deutsche-deadlock-community.de/social-media-admin`
- Die neue Seite hat einen schlanken eigenen Header (Admin-Badge + Rück-Link auf `/analyse`) und zeigt direkt die Clip-Pipeline ohne Tab-Navigation drumherum
- Partner sehen die Seite weiterhin nicht; ohne Admin-Recht kommt eine klare „Admin-Zugriff erforderlich"-Meldung
- Caddy ist um die neue Route erweitert, der Login-Redirect kehrt nach erfolgreicher Twitch-Auth direkt auf das Social-Media-Dashboard zurück

## #9 — Discord-Freigabe für Clips + Auto-Approve pro Plattform

- Fertig angereicherte Clips landen jetzt zuerst in einer Freigabe-Schleife, statt sofort in die Upload-Pipeline zu rutschen
- Ein Admin bekommt pro Clip eine Discord-DM mit Vorschau, plattformspezifischen Hashtags und den Aktionen „Posten", „Bearbeiten" oder „Skip"
- Beim Freigeben lassen sich YouTube Shorts, TikTok und Instagram Reels einzeln auswählen
- Zusätzlich gibt es im Social-Media-Dashboard neue Auto-Approve-Schalter pro Plattform, damit bestimmte Ziele nach einer Freigabe immer automatisch mit in die Queue gelegt werden
- Cross-Posting startet erst nach Freigabe oder Auto-Approve und nicht mehr schon vor dem Approval-Schritt

## #8 — Social-Media-Phase 3: Performance-Tracking, LLM-Reports und Analytics-Tab

- Veröffentlichten Clips werden jetzt pro Plattform in 24h-, 7d- und 30d-Buckets nachgezogen, inklusive Views, Likes, Comments, Shares, Watch-Time, CTR und Engagement-Rate
- Jede Woche kann automatisch ein deutscher LLM-Report für einzelne Streamer entstehen; zusätzlich gibt es einen monatlichen Cross-Streamer-Report sowie einen wöchentlichen Admin-Report per Discord-DM
- Im Admin-Dashboard gibt es jetzt einen eigenen Analytics-Bereich mit Charts pro Clip und einer Report-Liste für gespeicherte Streamer-, Cross- und Admin-Reports
- Migration weiter separat: vor dem ersten Einsatz einmal `python bot/migrations/social_media_phase3_analytics.py` ausführen, damit die Analytics-Spalten und die neue Tabelle `social_media_reports` angelegt werden

## #7 — Social-Media-Dashboard 2.0: Phase 0–2 (Layout-Editor, Auto-Aufbereitung)

- Bestehender Tab „Streams" wurde in „Social Media" umbenannt und ist vorerst nur für Admins sichtbar
- Clips bekommen jetzt automatisch ein vertikales 9:16-Layout mit Game- und Cam-Box, das pro Streamer als Default speicherbar ist und pro Clip übersteuert werden kann
- Eigene MP4s lassen sich direkt im Dashboard hochladen und werden 14 Tage aufbewahrt, bevor sie automatisch aufgeräumt werden
- Neue Auto-Aufbereitung: Clips werden lokal transkribiert, Deadlock-Begriffe (Helden, Items, Abilities, Slang) werden korrigiert und ein lokales LLM (Ollama auf dem Server, kein Datenabfluss) schlägt Title, Description und Hashtags je YouTube/TikTok/Instagram vor
- Externe LLMs (z. B. MiniMax oder Claude Haiku) bleiben standardmäßig aus und werden nur genutzt, wenn ein Admin den Schalter „External-LLM-Consent" ausdrücklich aktiviert
- Migration nicht automatisch — vor dem ersten Lauf einmal `python bot/migrations/social_media_phase2_enrichment.py` ausführen, damit die neuen Tabellen `deadlock_vocab` und `social_media_clip_enrichment` angelegt sind

## #6 — Changelog im Dashboard zeigt jetzt die letzten Updates

- Die Sektion „Was gibt's Neues" auf dem Streamer-Dashboard ist jetzt befüllt
- Alle bisherigen Verbesserungen (#1–#5) sind dort als Einträge sichtbar
- Künftige Updates erscheinen automatisch dort, sobald sie veröffentlicht werden

## #5 — Aktive Tab-Buttons im Analyse-Dashboard ohne kaputten 1px-Halo

- Aktiver Tab (z. B. „Übersicht") hatte einen harten cyan 1px-Strich am Rand, der mit dem Card-Highlight kollidierte und broken aussah
- Border ersetzt durch weichen Inset-Highlight + sanfteren Außen-Glow

## #4 — Glow-Tuning und feines Hintergrund-Grid

- Mini-KPI-Karten (Ø Viewer, Follower, Chat-Aktivität, Stream-Stunden) leuchten jetzt dauerhaft in ihrer Trendfarbe und nicht erst beim Hover
- Health-Score-Karte hat ein deutlich dezenteres Glow, damit es nicht mehr in den Bereich daneben überstrahlt
- Subtiles Gitternetz-Pattern im Dashboard-Hintergrund — wirkt weniger leblos, aber bleibt im Hintergrund

## #3 — Build-Toolchain auf aktuelle Node-LTS aktualisiert

- Build-System läuft jetzt auf Node 22 LTS statt der alten Node-18-Version
- Frontend-Build ist etwas schneller und ohne Versionswarnungen
- Keine Auswirkungen auf die Bot-Funktionalität, rein interne Aufräumung

## #2 — Streamer-Dashboard mit deutlich mehr Vibe

- Karten haben jetzt einen weichen farbigen Glow am Rand und heben sich beim Hover sichtbar an
- Header bekommt eine subtile rotierende Aura im Hintergrund
- Health-Score-Ring leuchtet farblich passend (grün/gelb/rot) mit Drop-Shadow
- Wochen-KPIs bekommen pro Karte eine farbige Trend-Aura (grün bei +, rot bei -)
- Sparkline-Linien glühen leicht in ihrer Trendfarbe
- Last-Stream-Mini-Stats (Ø Viewer, Peak, Follower, Chat) bekommen Hover-Spotlight und Text-Glow
- Activity-Items haben jetzt einen vertikalen Akzent-Streifen, farblich nach Typ (Raid grün, Ban rot, Warnung gelb)
- Letzte Streams Liste hat einen blau-violetten Akzent-Streifen pro Eintrag
- Live-Indikator pulsiert mit zusätzlichem roten Außenglow

## #1 — Streamer-Dashboard schneller, schöner und mit funktionierender Navigation

- Dashboard lädt deutlich schneller (Doppelter API-Request entfernt, Backend in mehrere parallele Aggregationen aufgeteilt)
- Sidebar-Links zu Overview, Streams und Chat funktionieren wieder und springen direkt auf den richtigen Tab
- Beim Laden erscheint sofort eine animierte Vorschau (Skeleton) statt eines leeren Spinners
- Neuer Live-Indikator im Header zeigt, ob du gerade live bist, mit aktueller Viewer-Zahl und Stream-Titel
- Wochen-KPIs (Ø Viewer, Follower, Chat, Stream-Stunden) haben jetzt eine Mini-Trendlinie der letzten 7 Tage
- Neue Sektion „Letzte Streams" listet die letzten 5 Streams mit Datum, Dauer, Ø Viewer, Peak und Follower-Zuwachs
- Aktivitäts-Feed lässt sich nach Raids, Bans und Warnungen filtern und mit „Mehr laden" ausklappen
- Sanftere Hintergrund-Animation, dezentere Optik und mehr Mikro-Animationen in Sidebar, Cards und Listen
