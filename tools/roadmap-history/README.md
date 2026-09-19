# Interaktiver Feature-Stammbaum

Die private Admin-Seite `/twitch/admin/roadmap-history/` zeigt die Entwicklung als
maßstäbliche Zeitachse im Stil eines Git-Graphen oder U-Bahn-Plans: ein Stamm,
aus dem jede Funktion als Ast an ihrem ersten datierten Nachweis abzweigt. Die
frühere Swimlane-Tabelle mit Monatsspalten ist entfernt. Die Änderungsliste bleibt
eine ergänzende, paginierte Ansicht und ersetzt nicht den Graphen.

## Bedienung und Darstellung

Der Start zeigt den Uplink-Zweig bei lesbaren **100 %**, nicht eine winzige
Gesamtübersicht. „Alle Zweige“ öffnet den ganzen Stammbaum; „Ansicht einpassen“
zeigt ihn als Übersicht. Die Funktionsauswahl fokussiert einen Teilbaum
einschließlich seiner notwendigen Vorfahren. Beispiele:

- Twitch Bot → Uplink → Ausgabemodi → AV1 & 2K.
- Twitch Bot → Partneraufnahme → Aufnahmesperren → Stream-Tag-Regeln.
- Twitch Bot → Chat-Befehle → Rang/Spielerabfragen → Steam-Verknüpfung.
- Twitch Bot → Caster-Studio → Caster-Overlay → Kamera-Portal.

**Eine gemeinsame, maßstäbliche Zeitachse trägt alle Elemente.** Die x-Position ist
überall das echte Datum; oben stehen Monate mit feinen Wochenstrichen. Der Stamm
„Twitch Bot“ läuft vom ersten bis zum letzten Nachweis. Jede Funktion ist ein Ast,
der am ersten datierten Beleg aus seinem Elternast abzweigt (weiche Kurve) und bis
zur letzten Änderung läuft. Astdicke und Helligkeit zeigen die Aktivität; ruhende
Äste laufen dünn und blass aus. Funktionen aus dem Import teilen sich den
Stammanfang und sind als „Bestand beim Start“ gekennzeichnet, weil ihr gemeinsames
Startdatum eine Datengrenze ist und kein echter gleichzeitiger Beginn. „other“ bleibt
ein ehrlicher Rest in der Liste und wird nicht als Ast gezeichnet.

Auf jedem Ast sitzen **Meilensteine** als Punkte am echten Datum. Ein Meilenstein
bündelt zusammenhängende Änderungen derselben Funktion, deren Abstand kleiner als
etwa drei Tage ist. Punktgröße zeigt den Umfang, Farbe die Änderungsart (neu,
Ausbau, Fehlerbehebung, Sicherheit). Der Titel ist der aussagekräftigste
`feat`-Betreff des Bündels, sonst der größte Commit, nie ein zufälliger. Reine
Pflege (Doku, Tests, Abhängigkeiten, Merges) erzeugt keinen Meilenstein. Hover zeigt
Datum, Titel und Anzahl; der Klick öffnet **alle** Einzelereignisse des Bündels in
der Detailleiste.

Äste werden nicht als feste Zeilen gestapelt, sondern platzsparend in freie Bahnen
gelegt: Eine Bahn wird wiederverwendet, sobald ein früherer Ast endet. Knoten
außerhalb des sichtbaren Ausschnitts werden nicht als DOM-Elemente gehalten. Die
vollständige Auswahl bleibt im Modell und in der paginierten Historie erhalten.

Ziehen verschiebt den Graphen; Mausrad und Pfeiltasten navigieren. Strg + Mausrad
oder +/− zoomen. „100 %“ stellt die lesbare Ausgangsgröße wieder her; „Ansicht
einpassen“ ist eine ausdrücklich gewählte Übersicht. Unterzweige lassen sich
auf-/zuklappen. Suche, Zeitraum, Änderungsart und Bereich filtern die Historie.
Dokumentation, Tests, Abhängigkeiten und Pflege sind optional sichtbar.
Notwendige Vorfahren bleiben als gestrichelte Kontextknoten erhalten. Eine neue
Suche öffnet zuvor eingeklappte Pfade und sucht über alle Hauptzweige.

Filter, Fokus, Klappzustand, Zoom/Position und Auswahl werden im Direktlink
wiederhergestellt. Die rechte Detailleiste ist auf Desktop **nicht modal**: Der
Graph bleibt sichtbar und bedienbar. Auf Mobilgeräten ist sie ein nativer modaler
Dialog mit Fokusbegrenzung, zuverlässigem Schließen, Escape und Fokusrückgabe.
Eltern und Kinder sind anklickbar. Historien haben 40 Ereignisse pro Seite mit
Vor-/Zurück-Navigation; die zusätzliche Änderungsliste lädt jeweils 80 Einträge.

## Stil und Abhängigkeiten

Die Oberfläche verwendet die aktuellen neutralen Flächen und Gold-/Messingtöne
von `bot/dashboard_v2/src/index.css` sowie die Schriftstapel und Laufweiten aus
`bot/shared-theme/typography.css`. Keine braunen Vollflächen, keine unterschiedlich
gefärbten Produktbereiche, keine erfundenen Profile oder Kennzahlen. Statusfarben
kennzeichnen Änderungsarten. Alle Repository-Texte werden als Textknoten gerendert.

Das Produkt bleibt eine einzelne selbständige HTML-Datei mit eingebetteten Styles,
JavaScript und inertem JSON. Kein Framework-Build, keine Diagramm-CDN, keine neue
externe Schriftabhängigkeit, keine GitHub-API und kein LLM im Generator. Python
benötigt nur die Standardbibliothek einschließlich `zoneinfo`. Playwright wird
lediglich für die Browser-Abnahme gebraucht.

## Datenvertrag (Schema 2)

`features.json` enthält explizit `id`, `parentId`, `group`, `title`, `description`,
Zuordnungsregeln und `relation`. `product` ist die reservierte Produktwurzel.
`other` erhält unklare Ereignisse, ohne einen echten Funktionsbereich vorzutäuschen.
Doppelte IDs, Zyklen, verwaiste Kinder und unbegründete Beziehungen werden verworfen.

Jede Beziehung hat eine `kind`-Angabe (`editorial` oder `historical`), eine nichtleere
`reason` und nach Möglichkeit konkrete Repository-Pfade in `sources`. Die hier
enthaltenen Beziehungen sind **redaktionelle fachliche Einordnungen**, keine aus
ähnlichen Commit-Wörtern erfundenen historischen Abspaltungen oder technischen
Abhängigkeiten. Das Modell kann historisch belegte Beziehungen unterscheiden;
diese benötigen zusätzlich einen vollständigen Beleg-Commit. Der Generator prüft
Komponentenpfade gegen den ausgewerteten Git-Baum und bewahrt fehlende Quellen als
`missingSources`. Unbelegte Beziehungen werden nicht als bestätigt angezeigt.

Quelle sind ausschließlich die vom angegebenen Git-Ref erreichbaren Commits des
Twitch-Bot-Repositorys. Der Generator pinnt den Ref vor der Auswertung. Ein erneuter
Lauf mit derselben Revision und Taxonomie liefert dieselben Ereignisse und
Zuordnungen; nur die Erzeugungszeit ändert sich. Merge-Commits werden nicht nochmals
gezählt. Nicht gemergte Arbeiten erscheinen bei `--ref origin/main` nicht als
Bestandteil des Main-Datenstands.

Ein Commit belegt **keinen Deploy**. „Erster Git-Nachweis inkl. Kinder“ ist der
früheste zugeordnete Commit im Teilbaum, kein verifiziertes Einführungsdatum. Der
erste direkte Nachweis an der Funktion steht separat in den Details. Beispielsweise
belegen ältere Titel-Performance-Commits nicht die Einführung des späteren Studios;
der Elternbereich heißt deshalb fachlich „Twitch Titel-Werkzeuge“.

Ein `feat`-Commit erzeugt **niemals automatisch eine neue Funktion**. Neue Funktionen
benötigen einen eigenen begründeten Taxonomie-Eintrag. Spezifische Commit-Titel haben
Vorrang vor allgemeinen Router-/Dashboard-Pfaden; danach folgen spezifische
Dateipfade. Bei einem Treffer für Kind und Vorfahr erhält nur das spezifische Kind
den Ereigniseintrag. Übergreifende Zuordnungen zu unabhängigen Funktionen bleiben
möglich. Mehrdeutige Ereignisse bleiben mit ihren Kandidaten unter „Noch nicht
zugeordnet“ erhalten, statt sie abzuschneiden oder pauschal Infrastruktur zu nennen.
Globale Zahlen und Teilbaumhistorien deduplizieren nach Commit-SHA.

Manuelle Korrekturen stehen unter `overrides`. Beispielstruktur, kein echter Eintrag:

```json
{
  "<vollständiger Commit-SHA>": {
    "features": ["uplink-av1"],
    "title": "Optionaler, anhand des Diffs geprüfter Anzeigetitel",
    "reason": "Pflicht: konkrete Begründung der geprüften Zuordnung",
    "note": "Optionale ergänzende Einordnung"
  }
}
```

Der vollständige Hash, bekannte eindeutige Funktions-IDs und eine Begründung sind
Pflicht. Der originale Commit-Titel bleibt bei Korrekturen unverändert erhalten.
Originaltitel, Zuordnungsgrund, Dateipfade und Code-Diff sind in den Details erreichbar.
Datumsangaben verwenden Europe/Berlin; Zeitfenster beziehen sich auf den Datenstand.
Die chronologische Sortierung verwendet die tatsächlichen Zeitpunkte, nicht den
lexikografischen ISO-Text mit möglicherweise unterschiedlichen UTC-Offsets.
Gleichzeitige Commits werden stabil nach SHA sortiert. Zeitstempel ohne explizite
Zeitzone werden abgewiesen, statt von der Zeitzone des Generator-Rechners abzuhängen.
Flache Klone und ein über sieben Tage alter Erzeugungsstand werden sichtbar gemeldet.

## Lokal erzeugen und testen

Aus dem Repository-Root:

```sh
git fetch origin main
python3 -m unittest discover -s tools/roadmap-history -p 'test_*.py' -v
node --test tools/roadmap-history/model.test.mjs tools/roadmap-history/family.test.mjs
python3 tools/generate_roadmap.py --ref origin/main
```

Ergebnis: `dist/roadmap-history/index.html`. `--repo`, `--ref` und `--output` sind
konfigurierbar. Veröffentlichung ersetzt die Datei erst nach erfolgreicher
vollständiger Erzeugung atomar.

Für die Browser-Abnahme:

```sh
npm install --prefix .roadmap-test-runtime --no-audit --no-fund --ignore-scripts playwright@1.58.2
node .roadmap-test-runtime/node_modules/playwright/cli.js install chromium
node tools/roadmap-history/browser.test.mjs
```

Der Testserver bindet ausschließlich `127.0.0.1` und wird auch nach Fehlschlägen
zusammen mit dem Browser geschlossen. Der Browser testet den echten generierten
Datenbestand; nur der separate Angriffstest verändert eine Kopie dieser Daten.
Keine öffentliche Debug-Route, keine Produktionssession und kein Auth-Bypass.

Nachweise unter `dist/roadmap-history/`:

- `family-desktop.png` und `family-desktop-1440.png`: fokussierter Uplink-Ast.
- `family-all-fit.png`: eingepasste Gesamtansicht aller Zweige auf der Zeitachse.
- `family-detail.png`: geöffnetes Meilenstein-Bündel mit rechter Detailleiste.
- `family-mobile.png` und `family-mobile-detail.png`: 390-Pixel-Ansichten.
- `family-hostile-long-text.png`: lange Texte und HTML-Injektionsversuch.
- `browser-report.json`: tatsächlich bestandene Prüfgruppen, Quellrevision,
  Datenmenge und Graphgeometrie.

Die PNGs müssen zusätzlich visuell angesehen werden. Erfolgreiche Modell- oder
Browser-Assertions allein ersetzen keine Kontrolle von Lesbarkeit und Linienführung.
Generiertes HTML, Screenshots und Testcache bleiben außerhalb der Git-Historie.

`.github/workflows/roadmap-history.yml` führt dieselben Prüfungen nur für relevante
Roadmap-PRs aus. Andere Bot-/Frontend-Änderungen lösen diesen Job nicht aus. Ein
neuer Push ersetzt einen noch laufenden Prüflauf; es gibt keine zusätzliche
regelmäßige Ausführung und keinen Deploy. Die bestehenden Action-Pins werden
wiederverwendet. Screenshots und Bericht liegen sieben Tage als zugriffsgeschütztes
Actions-Artefakt `roadmap-history-browser-proof` vor. Das vollständige private
Historien-HTML wird nicht als Artefakt hochgeladen.

## Vorhandener Veröffentlichungspfad und fehlende Freigabe

Ziel bleibt `/twitch/admin/roadmap-history/`. Die bisher beschriebene Caddy-Route
liefert `/srv/deadlock-roadmap` mit vorgeschalteter Admin-Authentifizierung. Weder
Caddy noch Authentifizierung wurden für diesen Auftrag geändert.

Der bestehende Installer `ops/systemd/install-roadmap-history.sh`, der Dienst
`deadlock-roadmap-history.service` und sein stündlicher Timer werden weiterverwendet.
Es wird kein zweiter Deploy-Mechanismus oder Timer aufgebaut. Ein Twitch-Binary-Deploy
veröffentlicht diese statische Seite **nicht**.

**In diesem Entwicklungszugang ist der Produktionsstatus nicht bestätigt.** Der
MCP-Zugriff auf `/srv/deadlock-roadmap` wurde als außerhalb der erlaubten Roots
abgewiesen; `systemctl` und `curl` sind nicht in der Programm-Allowlist. Diese
Grenzen werden nicht über Python, andere Prozesse, generisches sudo oder eine
Änderung der Allowlist umgangen. Ein erfolgreicher lokaler Generator- oder
Browser-Test ist kein Live-Nachweis.

Der exakt noch erforderliche Installationsschritt ist die **freigegebene Ausführung
des vorhandenen Installers aus dem geprüften, vollständig gemergten Quellstand**:

```text
ops/systemd/install-roadmap-history.sh
```

Dafür benötigt der Betrieb einen ausdrücklich autorisierten, auf diesen Installer
begrenzten Ausführungsweg. Der Installer kopiert Generator und sechs Assets nach
`/opt/deadlock-roadmap/`, installiert die vorhandenen Unit-Dateien, erzeugt
`/srv/deadlock-roadmap/index.html` und aktiviert den vorhandenen Timer. Diese
Ausführung wurde in der vorliegenden Entwicklungsabnahme nicht vorgenommen.

Der Installer sichert eine alte Seite einschließlich undatierter kuratierter
Einträge außerhalb des Webroots in `/var/backups/deadlock-roadmap/`. Er erfindet
für diese Alt-Einträge keine Datumswerte. Der Update-Dienst läuft anschließend als
`nathanael`, pinnt `origin/main` und darf nur Webroot und Git-Metadatenverzeichnis
beschreiben. Fetch-/Generatorfehler lassen die bisherige HTML-Datei unverändert.
Normale neue Commits werden automatisch aufgenommen; geänderte UI-/Generator-Assets
benötigen eine erneute autorisierte Installation. Fremde Arbeitsbaumänderungen
werden weder ausgecheckt noch gestasht oder zurückgesetzt.

Nach autorisierter Installation sind Dienst-/Timerstatus und Logs zu prüfen.
Zusätzlich muss der Betrieb die tatsächliche Seite **angemeldet** öffnen, den
neuen Stammbaum und seinen Quellstand bestätigen und **unangemeldet** den weiterhin
wirksamen Zugriffsschutz prüfen. Keine öffentliche Abnahme-Route hinzufügen.
Rollback benötigt dieselben Betriebsrechte: Timer stoppen und einen geprüften
Backup-Snapshot atomar wiederherstellen; Backups niemals über den Webserver anbieten.

## Nachgewiesene lokale Abnahme

Mit Main-Datenrevision `bca7e4d4f0ab605266ce730311dbee4077d7e502` (Nachprüfung am 19.09.2026):

- 3.677 eindeutige Nicht-Merge-Commits, vollständiger Klon; 52 Taxonomie-Einträge
  nach der Verfeinerung der Sammeleimer (Dashboard, Social, Raids, Runtime,
  Moderation, Partner in echte Unterfeatures geteilt).
- 31 Python-Tests und 29 Node-Tests erfolgreich.
- 14 Browser-Prüfgruppen erfolgreich, einschließlich identischer chronologischer
  Reihenfolge in Generator und Browser auf dem echten Datenbestand, echter
  Astwurzeln auf der Elternbahn (keine falschen Anschlüsse), überschneidungsfreier
  Astbeschriftungen, Meilenstein-Bündel mit vollständigen Einzelereignissen,
  Kontext-Eltern, Maus-/Tastaturbedienung, Zoom, Einpassen, Filter, Direktlinks,
  Seitenwechsel, HTML-Injektion und Desktop-/Mobil-Details.
- Die Gesamtansicht zeichnet 51 Äste auf einer maßstäblichen Zeitachse über acht
  Monate und passt sich ohne vertikales Dauer-Scrollen ein; „other“ erscheint nicht
  als Ast. 463 Meilensteine bündeln 2.966 zugeordnete Änderungen.
- Der dichte Modelltest bewahrt 1.200 Ereignisse vollständig in wenigen Bündeln
  ohne 1.200 DOM-Knoten; nur der sichtbare Ausschnitt wird gehalten.
- Keine JavaScript-Laufzeitfehler oder externe Font-/Diagramm-/CDN-Anfragen im Browser-Test.

Spätere Actions-Berichte nennen ihren eigenen gepinnten Datenstand. Die lokale
Abnahme behauptet weder eine authentifizierte Produktionsprüfung noch einen Deploy.
