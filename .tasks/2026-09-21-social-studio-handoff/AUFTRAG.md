# Auftrag: Social-Media-Dashboard integrieren und live abschließen

## 1. Ziel und Entscheidung des Nutzers

Arbeite am Repo `EarlySalty/Deadlock-Twitch-Bot`, Zielroute:

`https://deutsche-deadlock-community.de/social-media-admin?streamer=earlysalty`

Übernimm die Design- und Integrationsarbeit aus dem Chat in die bestehende Produkt-App. Baue keine weitere isolierte Vorschau als Endergebnis.

Das alte Dashboard war überladen: Cropper und Pipeline auf derselben Seite, verschachtelte Boxen, uneinheitliche Badges, gedrängte Aktionen und unruhige Abstände. Die neu entworfene Struktur gefiel dem Nutzer. Seine ausdrückliche Korrektur war: Farben, Stil und Logo müssen unserem vorhandenen Designsystem entsprechen; das neue Layout zunächst beibehalten.

Verbindliche Richtung:

- Aufgeräumte Struktur und Bedienung der Referenz erhalten.
- Vorhandenes Community-Design verwenden, kein neues Indigo-/Violett-Rebranding.
- Keine Rückkehr zur überladenen alten Anordnung.
- Kein ungefragter Produkt-Rename. „Social Studio“ ist ein Arbeitsname der Vorschau, keine beschlossene Umbenennung.

Der aktuelle Übergabeauftrag veröffentlicht Material als Branch und Draft-PR. Du als nachfolgender Integrationsagent erledigst anschließend die Produktintegration bis zu Tests, Push, Gate, Merge, Live-Deploy und eigenem Cleanup.

## 2. Material und ehrlicher Ausgangsstand

Die Referenz liegt neben dieser Datei unter `reference/`. Es ist kein Chat-Anhang oder Upload erforderlich. Lies zuerst `README.md`, `reference/README.md`, `reference/BRAND.md` und `reference/INTEGRATION.md`.

Enthalten sind die Komponenten `DashboardShell.jsx`, `CommunityBrand.jsx`, `ClipQueueCard.jsx`, `AutoPilotSchedule.jsx`, `WorkspaceDialog.jsx`, `Example.jsx`, ergänzende Styles, Vorschau-Logik und Tests. `reference/preview.html` ist eine selbstständige interaktive Demo. Unter `checks/screenshots/` liegen auf dem Projekthost neu gerenderte Referenzbilder.

Die Gold-Version wurde im Chat nicht in die Produkt-App integriert. Die Demo verändert Beispieldaten im Arbeitsspeicher, verbindet keine echten Konten und verarbeitet keine Videodateien. Ihr Cropper ist schematisch. Die React-Dateien wurden syntaktisch geprüft, nicht als integrierte Produkt-App abgenommen. Ergebnisse der Vorschautests ersetzen deine Produkttests nicht.

Das ursprüngliche ZIP hieß `social-studio-industrial-gold.zip`. Seine Quellen wurden unverändert importiert und durch `reference-manifest.json` abgesichert. Der gebaute HTML-Entwurf und das Tailwind-Stylesheet werden aus diesen Quellen wiederhergestellt. Originale Ergebnisdateien der vorherigen Session sind im Referenzordner erhalten und als historisch zu behandeln; aktuelle Übergabeprüfungen stehen in `EVIDENCE.md` und `checks/`.

Nicht `preview.html` als Ersatz für die Produktseite deployen. Demo-APIs, Mock-Konten, Testfehler-Schalter und simulierte Erfolgszustände haben im Produktionspfad nichts zu suchen.

## 3. Git, Bestand und Ownership zuerst prüfen

Der zuletzt verwendete Hauptcheckout lag unter `/home/nathanael/repos/Deadlock-Twitch-Bot`. Ältere Projektanweisungen nennen `/home/naniadm/Documents/Deadlock-Twitch-Bot`; tatsächliche Pfade prüfen.

Dieser neue Übergabe-Branch:

`docs/social-studio-handoff-20260921`

Er wurde von `origin/main` mit Basis `1327f414ffb8ccb1988276c003cb30f2dbfbfba8` erstellt. Er verändert den Task-Ordner, nicht die Produktkomponenten.

Historische, vor Beginn der Übergabe geprüfte Arbeitsstände:

| Branch | Letzter bekannter Stand | Bedeutung |
| --- | --- | --- |
| `feat/social-studio-redesign-20260921` | `1327f414` | Lokal angelegt, beim letzten Check kein eigener Redesign-Commit, kein Push und kein PR |
| `feat/social-media-ui-redesign-20260921` | `8a2afe31` | Separater vorhandener Remote-Branch, zuletzt „fix: Inter im Dashboard laden“, kein PR gefunden |
| `feat/twitch-ddc-brand-20260921` | `8d4169bc` | Separater vorhandener Remote-Branch zu Markenbeschriftungen |

Die alten Branches sind nicht identisch mit dem hier importierten Gold-Entwurf. Nicht ungeprüft übernehmen, mergen oder löschen. Bei einer früheren Prüfung enthielt `origin/main` bereits den Commit `29dd9c2d`, „feat: Social Media Dashboard neu strukturieren“. Es kann daher brauchbare Produktstruktur geben. Aktuelle `main`, Feature-Branches und Live-Artefakt vergleichen, statt aufgrund eines veralteten Checkouts neu zu bauen.

Der Hauptcheckout stand zuletzt auf dem fremden Branch `feat/player-multi-steam-rank-me`. Fremde Änderungen nicht zurücksetzen, überschreiben oder mitcommitten. Ownership anhand Status, letztem Commit und Worktree-Liste klären. In einem zulässigen eigenen Arbeitsbaum arbeiten.

## 4. Designsystem und Logo

Geprüfte Referenzquellen im Twitch-Bot-Repo:

- `bot/dashboard_v2/src/index.css`
- `bot/shared-theme/industrial-gold.css`
- `bot/shared-theme/typography.css`
- `bot/dashboard_v2/public/brand/deadlock-d-logo.png`
- Logo-Verwendung: `website/src/components/layout/Navbar.tsx`

Die Gold-Referenz nutzt Schwarzflächen `#0d0d0d`, `#121212`, `#161616`, Antikgold `#C5A059`, Messing `#D6B676`, Text `#f2eee6` und `#9d968a` sowie dunkle Schrift `#241A12` auf Gold. Das sind Werte aus der damaligen Prüfung. Für die Integration die aktuell verbindlichen Repo-Tokens verwenden und keinen zweiten globalen Farbsatz einführen.

Gold kennzeichnet Primäraktionen, aktive Navigation und dezente Materialkanten. Statusfarben bleiben semantisch davon getrennt. Neutrale Schwarzflächen, zurückhaltende Schatten und Materialwirkung statt großflächiger brauner Kästen, dekorativer Neonflächen oder schwerer Rahmen in jeder Untergruppe.

Das vorhandene Deadlock-D-Logo verwenden, auch mobil. Nicht das ältere `ddc-logo.svg` mit Blitz und Teal-Verlauf übernehmen und kein neues Logo zeichnen. Im Produkt das Originalasset verwenden. Die Referenz enthält eine kleine WebP-Darstellung desselben Motivs.

Manrope für Bedienung und Fließtext, Sora für Überschriften, KPI-Schrift gemäß vorhandenem Designsystem. Die Offline-Bilder zeigen eine Ersatzschrift. Im Produkt nachweisen, dass die vorhandenen Fonts tatsächlich laden. Fontdateien nicht als Übergabedateien verteilen.

Die gemeinsame Dashboard-Shell vorsichtig integrieren: keine zweite konkurrierende Navigation und keine unbeabsichtigten Designänderungen an Analyse, Uplink oder Verwaltung.

## 5. Informationsarchitektur und Komponenten

### Pipeline

Schlanke Clip-Liste als Standard. Kartenansicht entsprechend der Referenz beibehalten, soweit sinnvoll. Statusfilter, Suche und Plattformfilter. Subtile Kennzahlen für Freigaben, geplante Posts, Vorrat und Fehler. MP4-Upload und Twitch-Import bleiben erreichbar, stehen aber nicht als dominierende Dauerboxen über der Queue.

Echte Pagination statt einer unsichtbaren Begrenzung auf die ersten 24 Clips. Suche und Filter müssen ihren Geltungsbereich korrekt darstellen.

### Clip-Karte

Fokus auf Thumbnail, Titel, Quelle, Dauer, Views und relevante Zielplattformen. Bei freigabefähigen Clips sind Freigeben und Ablehnen die Hauptaktionen. Layout, Transkript/Metadaten, Vorschau, Original, Archivierung und gegebenenfalls Posting-Abbruch in das „…“-Menü.

Aktionen passend zum tatsächlichen Clip-Zustand. Ladezustände, Doppelklickschutz und verständliche Fehler an der betroffenen Karte. Ablehnen und Archivieren nicht semantisch gleichsetzen.

### Auto-Pilot und Zeitplan

Freigabemodus, Plattform-Aktivierung, Posts pro Woche, Tageslimit, Uhrzeiten und Kanal-Zeitzone übersichtlich anordnen. Vorhandene Kategorie- und Untertiteloptionen erhalten. Entwurf und gespeicherten Stand unterscheiden, Eingaben bei Fehlern erhalten. Pausierte Plattformen behalten ihre Einstellungen. Keinen wirkungslosen neuen globalen Ein-/Ausschalter erfinden.

### Templates und Layouts

Kanal-Standard von einzelnen Clip-Anpassungen trennen. Den vorhandenen echten `LayoutEditor` verwenden. Clip-Bearbeitung in fokussiertem Dialog statt mitten in der Queue. Den Geltungsbereich ausdrücklich anzeigen. Geometrie- und Renderer-Verträge erhalten. Bei gescheitertem Speichern den Editor offen lassen und den Fehler darin anzeigen.

### Konten und Einstellungen

Twitch als Quelle, YouTube/TikTok/Instagram als Veröffentlichungsziele. Echte Verbindungszustände und passende OAuth-Aktionen. Sprache und vorhandene VOD-Archiv-Einstellungen erhalten.

Analytics und Reports nicht entfernen. Sie können über eine untergeordnete Aktion erreichbar bleiben, ohne die vier Hauptbereiche erneut zu überladen.

## 6. Produktbausteine weiterverwenden

Frontend: `bot/dashboard_v2`, React, TypeScript, Tailwind und TanStack Query.

Relevante Dateien:

```text
src/pages/SocialMedia.tsx
src/pages/SocialMediaAdmin.tsx
src/components/layout/DashboardShell.tsx
src/components/layout/DashboardSidebar.tsx
src/components/socialmedia/LayoutEditor.tsx
src/components/socialmedia/EnrichmentPanel.tsx
src/components/socialmedia/AnalyticsTab.tsx
src/components/socialmedia/labels.ts
src/components/socialmedia/kartenZustand.ts
src/api/socialMedia.ts
src/types/socialMedia.ts
src/i18n/dictionary.ts
```

Vorhandene API-Funktionen, deren aktuelle Verträge vor Verwendung zu prüfen sind:

| Bereich | Funktionen |
| --- | --- |
| Queue | `fetchClips`, `decideClipApproval`, `discardClip`, `cancelScheduledPost`, `fetchTwitchClips`, `uploadClip` |
| Zeitplan | `fetchPostingPlan`, `savePostingPlanSettings`, `savePlatformSchedule`, `saveCategoryAutoPost` |
| Konten | `fetchPlatformStatus`, `oauthStartUrl`, `disconnectPlatform` |
| Layout | `fetchStreamerLayout`, `saveStreamerLayout`, `setClipLayoutOverride` |
| Vorschau | `requestPreview`, `getPreviewStatus`, `previewFileUrl` |
| VOD-Archiv | `fetchVodArchiveSettings`, `saveVodArchiveSettings` |

Die Referenzkomponenten in die vorhandene TypeScript-App überführen oder entsprechende bestehende Komponenten refaktorieren. Keine parallele App oder ungeschützte Fetch-Schicht daneben bauen. Auth, Berechtigungen, CSRF-Schutz und bestehende Fehlerbehandlung erhalten.

## 7. Integrationsfallen und verbindliche Abnahme

### Status und Freigaben

Ein veralteter `approval.state` darf einen veröffentlichten, verworfenen oder gerade veröffentlichenden Clip nicht wieder freigebbar machen. `approved` bedeutet nicht automatisch geplant; dafür müssen passende echte Posting-Termine vorliegen. Teilveröffentlichung nicht als vollständig erfolgreich anzeigen. Vorbereitung, Bearbeitung, Veröffentlichung und Fehler unterscheiden.

Zielplattformen nach tatsächlicher Nutzbarkeit vorbelegen. Abgelaufene Tokens und ausgeschaltete Kadenz berücksichtigen. Servermeldungen über ausgelassene Plattformen sichtbar halten.

### Kennzahlen und Vorrat

`fetchClips` liefert eine Seite plus `total`. Aus 24 oder 100 geladenen Clips berechnete Statuszahlen sind keine belegten Gesamtsummen. Geeignete Aggregationen verwenden oder eine kleine getestete serverseitige Ergänzung bauen. Eine Stichprobe nicht als Gesamtsumme beschriften.

Ein Clip für zwei Plattformen kann zwei geplante Posts bedeuten. Erledigte und historische Termine nicht erneut zählen. `created_at` ist kein Veröffentlichungszeitpunkt. Vorrat aus `postingPlan.pool` übernehmen und `reicht_fuer_tage = null` nicht als „0 Tage“ darstellen.

### Zeitplan und Teilfehler

Das Backend hat mehrere Speicherendpunkte. Ein gemeinsamer Speichern-Button macht die Änderung nicht atomar. Geänderte Felder kontrolliert speichern und anschließend den kanonischen Serverstand übernehmen.

Bei Teilfehlern den tatsächlichen Stand neu laden, gespeicherte Teiländerungen ehrlich anzeigen und noch offene Eingaben erhalten. Keine vollständige Rücknahme behaupten. Automatisierung nicht vor den zugehörigen Einstellungen aktivieren. Besonders `full_auto` erst nach erfolgreicher Übernahme notwendiger Plattform-/Filtereinstellungen setzen.

Entwürfe, Mutationen, Editorzustand und Query-Caches an den Kanal binden. Keine Einstellungen aus Kanal A in Kanal B speichern. Nach fehlgeschlagenem Laden keine erfundenen Defaults als echten Stand speichern.

### Konten

Requests an den ausgewählten `streamer` binden. `uses_global_fallback` beachten: keine Aktion anbieten, die versehentlich die gemeinsame Verbindung anderer Kanäle trennt. „Verbunden“ nicht mit „öffentliche Uploads freigegeben“ gleichsetzen. Bestehende Audit- und Plattformbeschränkungen korrekt darstellen.

### Nicht vorhandene Einstellungen

Der damals gelesene `PostingPlan` enthielt keinen frei wählbaren Vorlauf in Stunden und keine generischen Mindest-Views-/Dauerfilter. Keine wirkungslosen Controls einbauen. Vorhandene Kategorien und Aufbereitungsregeln übernehmen, ohne aus dem UI-Redesign ungefragt ein neues Scheduler-Projekt zu machen.

## 8. Tests und Sichtprüfung

Zuerst Ausgangszustand und bestehende Tests prüfen, danach gezielte Regressionstests für eigene Änderungen.

Abdecken:

- Terminale/veröffentlichende Clips nicht freigeben; Zielplattformen, fehlende/abgelaufene Konten und pausierte Kadenz.
- Ablehnen, Archivieren, Posting-Abbruch, Pagination und korrekte Kennzahlen.
- Ladefehler, leere Listen, Uploadfehler und defekte Thumbnails.
- Teilfehler beim Speichern, erhaltene Entwürfe, Kanalwechsel mit offenen Formularen oder laufenden Requests.
- Layout-Dialog mit Speicherfehler, Admin-/Partnerzugriff, deutsche und englische Texte.
- Keine Designregression in anderen Dashboard-Bereichen.

Produktbuild, Typecheck, gezieltes Linting und relevante bestehende Tests ausführen. Bei Rust-Änderungen zusätzlich die betroffenen Rust-Prüfungen. Fremde Dateien nicht durch pauschales Formatieren verändern.

Bestehende Tests beachten, besonders `brandPalette`, `socialMediaContract`, `socialMediaLayout`, `zeitplanFormular`, `dashboardShell` und Sprachtests.

Am integrierten Frontend Desktop sowie 320 und 390 Pixel Breite prüfen: kein horizontaler Seitenüberlauf, lesbare Aktionen/Statuszeilen, geladenes Original-Logo und geladene Produktfonts. Tastatur, Fokusführung, Escape und Rückkehr zum Auslöser prüfen. Fehler bleiben sichtbar; keine JavaScript-Fehler in den geprüften Abläufen. Screenshots bei fertig geladenem, stabilem Zustand erstellen.

Keine echten Clips zum Test veröffentlichen, Konten trennen, Zugänge vergeben oder `full_auto` aktivieren. Mutierende Abläufe isoliert mit geeigneten Fixtures prüfen. Live-Daten und Auth-Geheimnisse nicht als Testartefakte veröffentlichen.

## 9. Git, Gates und Deployment

Geltende `AGENTS.md` und `CLAUDE.md` sowie die Skills `graphify-nutzung`, `rolle-merge-schleuse`, `rolle-deploy-verifizierer` und `rolle-doku-redakteur` lesen. Vor Neubau vorhandene Lösungen über Graphify und anschließend im Code prüfen.

Im bestehenden Repo mit eigenem zulässigem Branch/Worktree arbeiten. Keine neue Repo-Kopie in Documents. Vor Commit/Merge `git log -1`, `git status` und `git worktree list` prüfen. Ein Git-Schritt pro Aufruf und literale absolute Pfade im Gate-Pfad.

Eigene verifizierte Commits direkt pushen. Bei Unterbrechung Arbeit nachvollziehbar als WIP sichern und pushen. Der Produktauftrag endet nicht mit einem lokalen Commit oder einem offenen Feature-Branch.

Test- und Review-Gates durchlaufen und Befunde beheben, Gates nicht umgehen. PR nach Repo-Praxis verwenden, aber nicht als Endpunkt stehen lassen. Nach erfolgreicher Prüfung nach `main` integrieren und `main` pushen, dabei die im Skill vorgeschriebene Push-Form beachten.

Den wirklichen Produktions-Auslieferungspfad ermitteln. Den gemergten Stand bauen und deployen; betroffene Dienste entsprechend ihrer Architektur neu starten/reloaden. Nicht blind Bot oder Caddy neu starten, wenn rein statische Auslieferung genügt; einen erforderlichen Neustart umgekehrt nicht auslassen.

Build-/Migrationsstand beachten, keine bereits angewandte Migration verändern. Release-Artefakte, Docker-Volumes und fremde Worktrees nicht pauschal löschen.

Live-Beweis: Zielroute, tatsächlich ausgelieferte JS-/CSS-Artefakte und DOM prüfen. Sichtbare Gestaltung und funktionierende Datenanzeige nachweisen, HTTP 200 genügt nicht. Bei Prozesswechsel PID, ausführbare Datei, Journal und Inhaltsanker nach Deploy-Skill belegen. Bei statischem Deploy passende Artefaktbeweise liefern, keinen Prozesswechsel erfinden.

Erst nach erfolgreichem Merge und Live-Nachweis eigene gemergte Remote-/lokale Branches und zugehörige Worktrees entfernen. Vorher Ancestry und ungemergte Arbeit prüfen. Fremde Branches aus Abschnitt 3 nicht automatisch löschen.

Secrets nicht ausgeben und nicht in Dateien, Logs, Screenshots oder Chat schreiben. Vorhandenen Secret-Loader verwenden. Sudo bei Bedarf nichtinteraktiv nach Projektvorgabe.

## 10. Dokumentation und Abschluss

Task-Akte mit Scope, Entscheidungen, Tests und Integrationsnachweis pflegen. Kein neues `CHANGELOG.md`. Kuratiertes Funktionswissen bei Bedarf in `Deadlock-Docs/internal` ablegen. Keine ungefragten öffentlichen Community-Ankündigungen; Regeln für das Admin-Werkzeug beachten.

Abschlussbericht mit tatsächlich eingebautem Produktumfang, Merge-SHA/PR, Tests und Einschränkungen, Live-URL mit genauer UI-Stelle, Desktop-/Mobil-Nachweisen, Deploy-/Artefaktbeweis und eigenem Cleanup-Status.

`MERGEPROTOKOLL[MS-1]`, `LIVEBEWEIS[DV-1]` und `TEXTNACHWEIS[DR-1]` nach den geltenden Skills ausfüllen. Keine Beweise erfinden.

Fertig ist die Produktintegration, wenn das echte Dashboard die aufgeräumte Struktur im vorhandenen Community-Design mit echten APIs verwendet, getestet und auf `main` gepusht ist und der Live-Zustand nachgewiesen wurde. Eine weitere HTML-Vorschau, ein ZIP oder ein ungemergter Branch allein erfüllt diesen Integrationsauftrag nicht.
