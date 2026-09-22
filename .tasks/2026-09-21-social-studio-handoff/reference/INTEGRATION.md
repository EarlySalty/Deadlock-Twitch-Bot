# Hinweis zur Markenrevision

Die Informationsarchitektur und API-Zuordnungen dieses Dokuments bleiben
bestehen. Die Indigo-/Zinc-Beispielgestaltung ist durch Industrial Gold
ersetzt. Farb-Tokens und Schriftstapel kommen aus der bestehenden Dashboard-App;
das originale Community-Logo ersetzt das generische App-Symbol.
Die Markenquellen stehen in `BRAND.md`.

`src/styles.css` ist für die Produktintegration vorgesehen. Die eigenständige
`preview.html` bettet keine Fontdateien ein und nutzt den System-Fallback.
Sie verändert keine Live-Daten.

# Anbindung an Deadlock-Twitch-Bot

Diese Notizen beruhen auf dem gelesenen Frontend unter `bot/dashboard_v2`. Die Lieferung verändert keine laufende Anwendung. Vor dem Einbau den aktuellen Branch und API-Vertrag erneut abgleichen.

## Vorhandenes statt Doppelbau

`origin/main` enthielt beim Lesen bereits den Commit `29dd9c2d` („Social Media Dashboard neu strukturieren“) mit vier Bereichen und einer neuen Shell. Deshalb nicht den älteren Arbeitscheckout als Integrationsbasis verwenden. Die hier gelieferten Komponenten sind eine separate Design-/Code-Referenz, keine Aussage, dass dieser Commit bereits auf der öffentlichen Website läuft.

Die bestehenden Komponenten `LayoutEditor`, `EnrichmentPanel` und `AnalyticsTab` weiterverwenden. Die API-Schicht in `src/api/socialMedia.ts` enthält bereits Auth-/Fehlerbehandlung; keine zweite Sammlung ungeschützter fetch-Aufrufe daneben bauen.

## Zuordnung

| Neue Oberfläche | Vorhandene Funktion |
|---|---|
| Pipeline mit echter Pagination | `fetchClips({ status, streamer, page, page_size })` |
| Freigeben | `decideClipApproval({ clipDbId, decision: 'approve', platforms })` |
| Ablehnen | `decideClipApproval({ clipDbId, decision: 'skip', platforms })` |
| Archivieren / Verwerfen | `discardClip(clipDbId)` |
| Geplanten Post stoppen | `cancelScheduledPost(clipDbId)` |
| Zeitplan laden | `fetchPostingPlan(streamer)` |
| Modus / Zeitzone / Untertitel | `savePostingPlanSettings(streamer, payload)` |
| Plattform-Kadenz / Zeiten | `savePlatformSchedule(streamer, platform, payload)` |
| Kategorien | `saveCategoryAutoPost(streamer, categoryKey, autoPost)` |
| Verbindungsstatus | `fetchPlatformStatus(streamer)` |
| OAuth verbinden | `oauthStartUrl(platform, streamer)` |
| Verbindung trennen | `disconnectPlatform(platform, streamer)` |
| Kanal-Layout | `fetchStreamerLayout`, `saveStreamerLayout` |
| Clip-Layout | `setClipLayoutOverride(clipDbId, layout)` |
| MP4-Upload | `uploadClip({ file, streamer_login })` |

## React-Anbindung

Die Daten für die Queue-Karte vor der Übergabe normalisieren:

```jsx
const queueClip = {
  id: apiClip.clip_db_id,
  title: apiClip.title,
  thumbnailUrl: apiClip.thumbnail_url,
  duration: apiClip.duration_seconds,
  views: apiClip.view_count,
  source: apiClip.source_kind === 'manual_upload' ? 'Upload' : 'Twitch',
  status: mappedStatus,
  targets: apiClip.approval?.approved_platforms ?? [],
  // Termine zuvor mit Intl in der Kanal-Zeitzone formatieren.
  scheduledLabel: formattedSchedule,
};

<ClipQueueCard
  clip={queueClip}
  streamer={streamer}
  availableTargets={connectedAndUsablePlatforms}
  onApprove={async (id, platforms) => {
    await decideClipApproval({ clipDbId: id, decision: 'approve', platforms });
    await queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
  }}
  onArchive={async id => {
    // In der Freigabe-Ansicht bedeutet diese Aktion „Ablehnen“.
    await decideClipApproval({ clipDbId: id, decision: 'skip', platforms: [] });
    await queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
  }}
  onEditLayout={clip => setEditingClip({ id: clip.id, mode: 'layout' })}
  onEditTranscript={clip => setEditingClip({ id: clip.id, mode: 'enrichment' })}
/>
```

Dieses Fragment beschreibt die Einbindung im vorhandenen Query-Container, nicht eine zusätzliche eigenständige App. In anderen Status-Ansichten muss `onArchive` auf die gewünschte Archivierungsaktion statt auf „skip“ zeigen. Fehler nicht verschlucken: Die Karte erwartet eine abgelehnte Promise und zeigt deren Meldung an.

### Status nicht gleichsetzen

`awaiting_approval` wird `review`; `approved` mit tatsächlichen geplanten Terminen wird `scheduled`; `published_all` wird `published`; `discarded`/`skipped` werden `archived`; `failed`/`published_partial` benötigen einen Fehler-/Teilstatus. `pending`, `enriched` und `editing` bleiben Vorbereitungszustände. `publishing` darf nicht erneut freigegeben werden. Der vollständige Backend-Status sollte zusätzlich erhalten bleiben, damit diese Vereinfachung der Ansicht keine Zustandsmaschine ersetzt.

Ein alter `approval.state = awaiting_approval` darf einen bereits veröffentlichten, verworfenen oder gerade veröffentlichenden Clip niemals wieder freigebbar machen. Zielplattformen nicht allein aus dem eingeschalteten Auto-Pilot ableiten: Kontenstatus, verfügbare Rechte und ggf. pausierte Kadenz prüfen.

### Zeitplan ist aktuell keine atomare Gesamtressource

`AutoPilotSchedule.onSave` nimmt einen vollständigen Entwurf entgegen und erwartet als Ergebnis den kanonischen Plan. Der vorhandene Server hat dafür mehrere Endpunkte, nicht eine atomare Gesamtänderung.

Im Integrationscontainer nur geänderte Felder senden, Plattform-/Kategorieänderungen kontrolliert ausführen und den Plan anschließend neu laden. Schlägt eine spätere Teiländerung fehl, können vorherige Änderungen bereits gespeichert sein. Dann den tatsächlichen Serverstand neu laden, einen Teilfehler anzeigen und ausdrücklich keine angeblich vollständige Rücknahme behaupten. Für eine echte Alles-oder-nichts-Speicherung ist ein transaktionaler Backend-Endpunkt nötig.

Automatisierung nicht vor ihren Sicherheits-/Filtereinstellungen einschalten. Besonders beim Wechsel zu `full_auto` den finalen Freigabemodus erst setzen, nachdem zugehörige Einstellungen erfolgreich übernommen wurden. Die UI-Speichersperre verhindert Doppelklicks, ersetzt aber keine Servertransaktion.

React-Komponente je Kanal mit `key={streamer}` mounten. Das vermeidet, dass ein noch offener Entwurf beim Kanalwechsel im falschen Konto landet. `loadError` übergeben; bei unbekanntem Stand keine erfundenen Defaults speichern.

## Wichtige semantische Grenzen

### Kennzahlen und Pagination

`fetchClips` liefert eine Seite plus `total`, keinen vollständigen globalen Aggregationssatz. Freigabe-/Fehlerzählungen aus 24 oder 100 gelesenen Clips dürfen nicht als Gesamtzahlen ausgegeben werden. Entweder echte serverseitige Summen verwenden oder die Stichprobe klar kennzeichnen. Die Offline-Demo kennt ihren vollständigen kleinen Datensatz.

„Geplante Posts“ zählt Zielplattformen: Ein Clip auf YouTube und TikTok sind zwei Posts. Bereits veröffentlichte Plattformen und historische Termine ausgeschlossener Clips nicht erneut zählen. `created_at` ist kein Veröffentlichungszeitpunkt. Einen „Heute veröffentlicht“-Wert nur aus tatsächlichen Posting-Ereignissen berechnen.

Die Vorratsprognose aus `postingPlan.pool` übernehmen; `null` bei ausgeschalteter Automatik ist keine echte Null-Tage-Prognose. Nach einer erfolgreichen Kadenzänderung den vom Server neu berechneten Stand verwenden.

### Konten

Alle Requests mit dem gewählten `streamer` scopen. Bei `uses_global_fallback` keinen normalen „Trennen“-Knopf anbieten, der eine globale Verbindung für andere Kanäle entfernen könnte. „Token verbunden“ und „Plattform erlaubt öffentliche Uploads“ getrennt behandeln. Twitch als Quelle nicht automatisch mit dem Auth-Zustand aller Zielplattformen gleichsetzen.

### Upload, Layout und Editor

Die MP4-Demo verarbeitet nur den Dateinamen und lädt keine Datei hoch. Den bestehenden echten Upload mit seinen serverseitigen Format-/Größenprüfungen integrieren.

Die Vorschau des 9:16-Editors ist schematisch, keine FFmpeg-Vorschau. Den vorhandenen `LayoutEditor` in `WorkspaceDialog` rendern und dabei zwischen `saveStreamerLayout` und `setClipLayoutOverride` unterscheiden. Nach einer fehlgeschlagenen Speicherung den Dialog offen halten. Canvas-Pixel und Backend-Koordinaten nicht durch neue, inkompatible Geometrie ersetzen.

### Weitere Einstellungen benötigen Backend-Unterstützung

Der gelesene `PostingPlan` umfasst Freigabemodus, Zeitzone, Untertitel, Plattform-Zeiten/-Kadenz und Kategorien. Es gibt dort keinen frei wählbaren zeitlichen Vorlauf und keine generischen Mindest-Views-/Dauerfilter. Diese Einstellungen erst anbieten, wenn API und Scheduler sie tatsächlich verarbeiten.

## Einbaugrenzen

Die React-Komponenten wurden hier syntaktisch geparst/transpiliert, nicht im vorhandenen App-Build oder gegen das echte Backend ausgeführt. Vor einem Merge sind mindestens Produktbuild, bestehende Vertragstests, authentifizierter Browserreview, Kanalwechsel, leere/fehlerhafte Abfragen, OAuth-Fehler, Teilfehler beim Speichern und Posting-Sperren zu prüfen.

Keine Produktionsumschaltung und kein Bot-Neustart sind Bestandteil dieses Pakets.
