# Inventar des Twitch-Bot-Vollreviews

Stand: 2026-10-08. Dies ist eine Bestandsaufnahme mit Paketschnitt, keine Defektsuche und kein Qualitätsurteil.

## Eingefrorene Grundlage

- Repository: `/home/nathanael/repos/Deadlock-Twitch-Bot`.
- Ausschließlich verwendeter Arbeitsbereich: `/home/nathanael/.worktrees/tb-vollreview-artefakte`.
- Branch: `audit/tb-vollreview-20261008`.
- Basis: `origin/main` bei Auftragsbeginn, `0ecae1370f1a80d1a101249b5c932663d69be8af`.
- Inventarisierter Commit: `e8801a0202059dbf919863101904d9a152a94367`.
- Zwischen Basis und inventarisiertem Commit sind ausschließlich `AUFTRAG.md` und `REGISTER.md` dieses Auftrags verschieden. Der Produktionsbestand ist identisch.
- Der Orchestrator hat während der Inventarisierung weitere Auftragsdokumente committed. Beobachteter HEAD: `6937e4a61f43a9c08174fa95c96f49da149ca859`. Der Unterschied zum eingefrorenen Inventar besteht ausschließlich aus geändertem `AUFTRAG.md`, geändertem `REGISTER.md` und neuem `REVIEW-VERTRAG.md`. Die Zahlen hier beziehen sich absichtlich auf den vereinbarten Commit, nicht auf einen beweglichen HEAD.
- Die drei neuen Inventarartefakte sind unterstützende Auftragsdokumente und nicht in den Zahlen des eingefrorenen Commits enthalten.

Dateibestand und Bytegrößen stammen aus `git ls-tree -rl e8801a0202059dbf919863101904d9a152a94367`. Textzeilen wurden aus den zugehörigen Git-Blobs gezählt, nicht aus dem fremden, verschmutzten Hauptcheckout. Eine letzte Zeile ohne abschließenden Zeilenumbruch zählt als eine Zeile. Bei Binärdateien gibt es keine Textzeilenzahl. Für das später ausgeschlossene Werkzeug `tools/boon` wurde bei der allgemeinen Größenaufnahme lediglich der Binärstatus festgestellt, kein Inhalt ausgegeben oder Quellreview durchgeführt. Secret- und ENV-Pfade sowie ai-coach wurden nicht gelesen. Bytegrößen sind logische Blobgrößen, keine belegten Dateisystemblöcke.

## Ergebnis und Abdeckungsbilanz

| Zuordnung | Dateien | Bytes | Textzeilen |
|---|---:|---:|---:|
| P: eindeutiges Review-Paket | 1745 | 22788893 | 597180 |
| S: unterstützende Daten und Dokumente | 1025 | 63672463 | 95095 |
| G: generierte Referenzen | 921 | 1473190 | 51422 |
| X: ausdrücklicher Ausschluss | 1 | 2784736 | nicht gelesen |
| Gesamt | 3692 | 90719282 | 743697 |

Jeder der 3692 versionierten Pfade hat genau eine dieser Zuordnungen. Alle 1745 primären Review-Dateien haben genau einen Eigentümer. Nicht zugeordnete primäre Dateien: **0**. Mehrfach zugeordnete primäre Dateien: **0**. Nicht vorhandene Referenzpfade: **0**. Nicht vorhandene Eigentumsmuster: **0**.

Es gibt **108 Pakete**: 56 mit P0, 49 mit P1 und 3 mit P2. Die fünf ursprünglich beauftragten Blickwinkel ergeben 540 Review-Kombinationen. Der während der Arbeit ergänzte sechste Blickwinkel Bauqualität ergibt mit demselben Schnitt 648 Kombinationen. Diese Zahlen beschreiben die spätere Review-Arbeit; hier wurde kein Reviewer gestartet und kein Befund erhoben.

Das größte Paket umfasst 11983 Textzeilen. Die großen Einzeldateien `tb-chat/src/promos.rs` mit 7777 Zeilen und `tb-dashboard-api/src/handlers/social_media.rs` mit 7677 Zeilen haben eigene Pakete. Das Stream-Audit-Binary mit einer 6647-zeiligen `main.rs` ist ebenfalls ein eigener Lesebereich. Übergreifende Aufrufer, Tests, Auth, Middleware und Proxy-Schichten sind Referenzen, keine mehrfachen Dateieigentümer.

## Tatsächlich vorhandener Rust-Bestand

Der Auftrag nennt beispielhaft 26 Crates. Tatsächlich enthält `rust/Cargo.toml:3-38` **28 Crates unter `rust/crates/`**, zusätzlich eine lokale Vendor-Crate und fünf Workspace-Binary-Pakete. Es gibt keine starre Paketanzahl aus der Auftragszahl.

Die folgende Bereichsgröße umfasst sämtliche versionierten Dateien des jeweiligen Crate-Verzeichnisses, also auch separat klassifizierte Assets und Testdaten. Die primären Paketgrößen stehen in `PAKETE.md` und `pakete.json`.

| Crate unter rust/crates | Dateien | Bytes | Textzeilen | Pakete |
|---|---:|---:|---:|---|
| fireworks-model-selection | 2 | 9135 | 257 | R01 |
| tb-analytics | 83 | 1936649 | 51078 | AN01 bis AN08 |
| tb-chat | 71 | 2291403 | 62751 | CH01 bis CH08 |
| tb-config | 23 | 186010 | 5315 | R02 |
| tb-crypto | 8 | 23169 | 650 | R03 |
| tb-dashboard-api | 206 | 5341668 | 127307 | DA01 bis DA19 |
| tb-db | 22 | 247813 | 5652 | R04 |
| tb-domain | 7 | 15700 | 479 | R05 |
| tb-effort | 16 | 214937 | 5245 | R06 |
| tb-engagement | 40 | 783551 | 20467 | EN01 bis EN03 |
| tb-error | 2 | 1575 | 49 | R07 |
| tb-highlight | 14 | 154565 | 4306 | R08 |
| tb-http-core | 8 | 21176 | 680 | R09 |
| tb-internal-api | 39 | 950853 | 26027 | IA01 bis IA04 |
| tb-knowledge | 16 | 40400 | 1204 | R10 |
| tb-llm | 20 | 174670 | 4724 | R11 |
| tb-load | 4 | 16082 | 406 | R12 |
| tb-monitoring | 48 | 1034790 | 28540 | MO01 bis MO04 |
| tb-observability | 9 | 53943 | 1609 | R13 |
| tb-platform-core | 10 | 49515 | 1546 | R14 |
| tb-raid | 72 | 1278531 | 34066 | RA01 bis RA04 |
| tb-scout | 7 | 77337 | 2061 | R15 |
| tb-social-media | 69 | 1961515 | 31583 | SM01 bis SM04 |
| tb-stream-audit | 10 | 130646 | 3504 | R16 |
| tb-tips | 4 | 7621 | 279 | R17 |
| tb-transport-discord | 6 | 74195 | 1992 | R18 |
| tb-transport-twitch | 17 | 269718 | 7429 | R19 |
| tb-vod-archive | 10 | 171735 | 4685 | R20 |

`rust/vendor/uplink-infisical-transport/` hat drei Dateien, davon zwei primäre Quelldateien in R21 und eine unterstützende README. Produktives eingebettetes Wissen unter `rust/knowledge/` gehört K01, trotz Markdown- und Textformat.

### Binaries

| Bereich | Dateien | Bytes | Textzeilen | Eigentümer |
|---|---:|---:|---:|---|
| rust/bin/tb-bot | 44 | 1486776 | 38781 | BO01 bis BO06 |
| rust/bin/tb-dashboard | 3 | 32554 | 891 | BI01 |
| rust/bin/tb-category-collector | 5 | 55447 | 1416 | BI02 |
| rust/bin/tb-stream-audit | 2 | 290530 | 6678 | BI03 |
| rust/bin/tb-stt-server | 3 | 63981 | 1879 | BI04 |
| rust/bin/build_revision.rs | 1 | 1982 | 43 | BU01 |

Zusätzliche Rust-Binary-Quellen liegen in `rust/crates/tb-config/src/bin/tb-config-check.rs`, `tb-config-inventory.rs` und `rust/crates/tb-social-media/src/bin/korpus_ernte.rs`, `render_clips.rs`. Sie sind R02 beziehungsweise SM03 zugeordnet und nicht wegen ihres Orts unter einer Crate übersehen.

## Frontends und weitere produktionsrelevante Bereiche

| Bereich | Versionierte Dateien | Primäre Dateien | Größe oder Zeilenbeleg | Pakete |
|---|---:|---:|---|---|
| bot/dashboard_v2 | 320 | 303 | 2704972 Bytes, 65126 Textzeilen einschließlich Lock und Assets | FE01 bis FE07, FE09, FE10 |
| bot/admin_dashboard | 80 | 78 | 721102 Bytes, 18866 Textzeilen einschließlich Lock und Doku | AD01 bis AD03 |
| bot/shared-theme | 3 | 3 | 23856 Bytes, 642 Textzeilen | FE08 |
| website | 154 | 121 | Website-Quelldaten und öffentliche Assets, siehe WE-Paketgrößen | WE01 bis WE03 |
| rust/migrations | 181 | 181 | 466237 Bytes, 10421 Textzeilen; 180 SQL-Dateien plus .gitkeep | DB01 |
| bot/migrations | 8 | 8 | 60085 Bytes, 1343 Textzeilen | DB02 |
| rust/test-support | 4 | 4 | 9236 Bytes, 272 Textzeilen | DB02 |
| ops | 82 | 75 | Dienste, Rollen, Release, Proxy-Snippets, Highlight und STT | OP01, OP02 |
| scripts | 6 | 6 | 9124 Bytes, 334 Textzeilen | OP01, BU01 |
| tools | 21 | 19 | Python-Legacy, Roadmap-Historie, ausgeschlossenes Binärwerkzeug | TO01 |
| tests | 4 | 4 | Legacy-Testverträge und Build-Revisionsprüfung | TO01 |
| rust/knowledge | 24 | 24 | 49684 Bytes, 761 Textzeilen | K01 |
| rust/.sqlx | 915 | 0 | 974109 Bytes, 35183 Textzeilen generierte SQLx-Metadaten | G, Referenz von BU01 und SQL-Paketen |

Auch Root-Scannerregeln, `.githooks/`, `.github/`, Workspace-Konfiguration, Toolchain, Deployment-Beispiel, SQL-Wartungsskripte und `rechnung-vorschau.html` gehören BU01. Vorhandene GitHub-Konfiguration wird inventarisiert, aber nicht als Merge-Gate behandelt. Keine GitHub-Aktion wurde gestartet.

Python-Bestand wird ausschließlich als Legacy beziehungsweise vorhandenes Verwaltungs-, Build- oder Testwerkzeug erfasst. Beispiele sind der Watchdog und Credential-Provisionierer unter `ops/systemd/`, der Detector unter `ops/highlight-detector/`, Analyse- und Roadmap-Dateien unter `tools/`, Bildwerkzeuge im Dashboard und das Crypto-Testorakel. Keine Python-Funktion wurde erweitert, ausgeführt oder gefixt.

## Unterstützende und generierte Pfade

Die Regeln im nächsten Abschnitt sind vollständig und haben Vorrang vor breiten Paketverzeichnissen.

| Unterstützende Gruppe | Dateien | Bytes | Textzeilen |
|---|---:|---:|---:|
| .tasks/ ohne separat generierte Locks | 701 | 49113853 | 34677 |
| agent-docs/ | 26 | 255768 | 3498 |
| docs/ einschließlich Screenshots | 126 | 4953976 | 16932 |
| features/ | 4 | 44414 | 697 |
| rust/docs/ | 92 | 3004859 | 34924 |
| website/.tasks/ | 2 | 7183 | 113 |
| Sonstige Dokumente, Lizenzen und Testdaten | 25 | 276207 | 3892 |
| Sonstige Medienassets | 49 | 6016203 | 362 |

Die sonstigen Unterstützungsgruppen sind durch die Endungsregel eindeutig bestimmt. Dazu gehören beispielsweise das Schema-Test-Snapshot in tb-db, die tb-social-media-Fontdatei samt Lizenz und Logo, das Caster-Hintergrundbild, Dashboard-Bilder, Website-Clips und die Roadmap-Historiengrafik. HTML, CSS, SQL, TOML, JSON und produktive Wissensdateien werden nicht pauschal als Daten aus dem Review genommen.

Generierte Lockdateien sind genau:

- `.tasks/2026-09-21-social-studio-handoff/checks/package-lock.json`
- `.tasks/2026-09-21-social-studio-handoff/reference/package-lock.json`
- `bot/admin_dashboard/package-lock.json`
- `bot/dashboard_v2/package-lock.json`
- `rust/Cargo.lock`
- `website/package-lock.json`

Diese sechs Dateien haben zusammen 499081 Bytes und 16239 Textzeilen. Die beiden historischen Task-Locks bleiben reine Historie. Die vier aktiven Locks sind Lieferkettenreferenzen von BU01 und den Frontend-Paketen. Eine spätere Abhängigkeitsprüfung darf sie als Referenz auswerten; die Kennzeichnung G ist keine Aussage über Sicherheit.

## Ausschlüsse und Grenzen

1. `tools/boon`: versioniertes Binärwerkzeug, 2784736 Bytes, keine in diesem Repository lesbare Quelle. Bei der Größenaufnahme als Binärdatei klassifiziert, anschließend vom Quellreview ausgeschlossen. Keine Ausführung und keine Ausgabe des Inhalts. Ein Quellreview dieses Werkzeugs ist mit dem Repo-Bestand nicht möglich.
2. `ai-coach`: kein versionierter Pfad dieses Namens im eingefrorenen Twitch-Repo. Das externe Projekt wurde nicht geöffnet, abgefragt oder verändert. Die vorhandenen Twitch-Pfade mit Namen `coaching` beziehungsweise `tb-stream-audit` sind sichtbar diesem Repository zugeordnet; sie werden nicht ohne Beleg mit dem externen Projekt gleichgesetzt.
3. ENV-Dateien und echte Secretdateien: keine solchen versionierten Pfade im Bestand anhand des dokumentierten Ausschlussfilters. Unversionierte ENV-Dateien, Secret-Manager-Inhalte, Datenbankzugänge und installierte Konfigurationsbestände wurden nicht gelesen. Quellcode wie `credentials.rs`, `secret_sink.rs` und der Credential-Provisionierer ist kein Secretbestand und wird nur als Quellpfad inventarisiert.
4. Abhängigkeiten außerhalb des Git-Baums, Build-Artefakte, laufende Dienste, Produktionsdaten und `/etc` sind außerhalb dieser rein versionierten Bestandsaufnahme. Keine Behauptung über den tatsächlichen Live-Zustand.
5. Allgemeine Doku und historische Taskkopien sind Unterstützung, kein zweiter Produktionscode. Ihre Zuordnung ist vollständig; ein eigenständiges Doku- oder Historienreview gehört nicht zu diesem Paketschnitt.

## Nachgewiesene Abwehr- und Routingreferenzen

Vor der Codeorientierung wurden der Skill `code-suche` und zwei `graphify query`-Aufrufe verwendet. Der Worktree enthält keinen `graphify-out`-Link. Deshalb wurde ausschließlich der vorhandene Graph `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json` mit `--graph` abgefragt. Kein Graph wurde generiert oder erneuert. Graphresultate sind Wegweiser, keine Vollständigkeitsgrundlage und kein Beleg für Live-Konfiguration. Die vollständige Pfadabdeckung stammt aus Git.

- `rust/crates/tb-dashboard-api/src/lib.rs:96` und folgende: öffentliche Router; `:230` und folgende: authentifizierter Router; `:1004-1015`: eingebundene Partner- und CSRF-Middleware; `:1019`, `:1060`, `:1141`: Admin-Router; `:1307`: Auth-Router; `:1624`: Billing-Webhook; `:1985-1991`: OBS-WebSocket; `:2057`: Clip-Wettbewerb. Die Routerzusammenführung bleibt DA01 und ist Referenz für alle Handlerpakete.
- `rust/crates/tb-dashboard-api/src/auth/csrf.rs:89`: CSRF-Middleware; `auth/partner_gate.rs:140`: Partner-Gate; `auth/streamer_scope.rs:86`: Kanalzuordnung aus dem Graphen; `auth/level.rs:53`: Authstufe. Diese Dateien gehören DA02.
- `rust/crates/tb-dashboard-api/src/handlers/forward_auth.rs:55`: Admin-Session für Forward-Auth. Gehört DA04 und ist Abwehrreferenz für Admin-Browser und Proxy.
- `rust/crates/tb-http-core/src/middleware/auth.rs:16`, `loopback.rs:15` und `idempotency.rs`: gemeinsamer HTTP-Schutz in R09.
- `rust/crates/tb-internal-api/src/lib.rs:414-423`: interner Router mit eigener `security`-Middleware. Deshalb zusätzlich `rust/crates/tb-internal-api/src/security.rs` referenzieren; Schutz aus tb-http-core nicht pauschal auf diesen Router übertragen.
- `ops/caddy/partner-profiles.caddy:3-12`: versionierter öffentlicher Profilmatcher und Reverse-Proxy. `ops/caddy/partner-profile-assets.caddy:2-6`: Profilassets und Reverse-Proxy. Beide Dateien enthalten Routingreferenzen, nicht die vollständige produktive Caddy-Allowlist. Sie gehören OP01 und werden nicht geändert.
- `ops/systemd/pg_hba-twitch.conf`, `twitch-runtime-roles.sql`, `category-runtime-roles.sql` und `install-twitch-contest-peer`: versionierte Rollen- und Peergrenzen als Referenzen für SQL- und Handlerreviews. Nichts davon wurde angewandt.

Die vollständige installierte Caddy-Konfiguration ist nicht versioniert in diesem Bestand. Daher ist eine Aussage wie „Caddy verhindert diesen Aufruf“ später gesondert zu belegen. Das ist eine Live-Beleggrenze, keine Lücke im Git-Inventar.

## Vollständige, wiederholbare Zuordnungsregel

Die folgenden Regeln werden in dieser Reihenfolge angewandt:

1. X: `tools/boon`, ein Pfad mit `ai-coach` im Namen, ENV-Pfade oder Endungen `.pem`, `.key`, `.p12`, `.pfx`.
2. G: `rust/.sqlx/` oder Dateiname `package-lock.json` beziehungsweise `Cargo.lock`.
3. S: Verzeichnisse `.tasks/`, `website/.tasks/`, `docs/`, `agent-docs/`, `features/`, `rust/docs/`.
4. S: Endungen `.md`, `.txt`, `.png`, `.jpg`, `.jpeg`, `.mp4`, `.ttf`, `.woff2`, `.gif`, `.svg`, `.ico`, `.pdf`, außer unter `rust/knowledge/`.
5. P: alle übrigen Pfade. Genau ein Paket muss über `paths` in `pakete.json` passen.

Paketverzeichnisse umfassen alle darunterliegenden versionierten Dateien nach diesen Klassifikationsregeln. `{a,b}` ist explizite Alternativenexpansion. `*` und `?` gelten innerhalb genau einer Pfadkomponente, niemals über `/` hinweg. Ein Dateipfad ist exakt; ein existierender Verzeichnispfad ist rekursiv. `reference_paths` begründet keine Eigentumszuordnung.

Die sortierte Zuordnung wird als UTF-8 mit je einer Zeile `Klasse<TAB>Eigentümer<TAB>Pfad<LF>` gehasht. Für P ist Eigentümer die Paket-ID, für andere Klassen die Klasse selbst. SHA-256:

`ae5f99767bbff01c7f04d804140c1f09b5318ecec5ce4919547158abd1bf2967`

Der folgende kurze Prüfaufruf rekonstruiert die vollständige Pfadzuordnung ohne eine Datei zu schreiben und ohne Quellinhalte oder Secrets zu lesen. Er ist ein einmaliges Auditwerkzeug, kein neuer Produktivcode.

```python
import collections, hashlib, json, re, subprocess
repo = '/home/nathanael/.worktrees/tb-vollreview-artefakte'
sha = 'e8801a0202059dbf919863101904d9a152a94367'
manifest = repo + '/.tasks/2026-10-08-twitch-bot-vollreview/pakete.json'
with open(manifest) as handle:
    packages = json.load(handle)
paths = subprocess.check_output(
    ['git', '-C', repo, 'ls-tree', '-r', '--name-only', sha]
).decode().splitlines()

def classify(path):
    if (path == 'tools/boon' or 'ai-coach' in path.lower()
        or re.search(r'(^|/)\.env|\.(pem|key|p12|pfx)$', path, re.I)):
        return 'X'
    if path.startswith('rust/.sqlx/') or path.endswith(('package-lock.json', 'Cargo.lock')):
        return 'G'
    if path.startswith(('.tasks/', 'website/.tasks/', 'docs/', 'agent-docs/', 'features/', 'rust/docs/')):
        return 'S'
    if (path.endswith(('.md', '.txt', '.png', '.jpg', '.jpeg', '.mp4', '.ttf', '.woff2', '.gif', '.svg', '.ico', '.pdf'))
        and not path.startswith('rust/knowledge/')):
        return 'S'
    return 'P'

def expand(pattern):
    match = re.search(r'\{([^{}]+)\}', pattern)
    if match is None:
        return [pattern]
    return [item for value in match.group(1).split(',')
            for item in expand(pattern[:match.start()] + value + pattern[match.end():])]

def matches(path, pattern):
    for value in expand(pattern):
        if '*' not in value and '?' not in value:
            if path == value or path.startswith(value + '/'):
                return True
        else:
            expression = re.escape(value).replace(r'\*', '[^/]*').replace(r'\?', '[^/]')
            if re.fullmatch(expression, path):
                return True
    return False

assignment = []
counts = collections.Counter()
for path in sorted(paths):
    category = classify(path)
    owners = ([package['id'] for package in packages
               if any(matches(path, pattern) for pattern in package['paths'])]
              if category == 'P' else [])
    assert category != 'P' or len(owners) == 1, (path, owners)
    owner = owners[0] if owners else category
    counts[category] += 1
    assignment.append(category + '\t' + owner + '\t' + path + '\n')
assert len(paths) == 3692
assert counts == {'P': 1745, 'S': 1025, 'G': 921, 'X': 1}
assert len(packages) == 108
assert sum(package['file_count'] for package in packages) == 1745
assert sum(package['line_count'] for package in packages) == 597180
assert hashlib.sha256(''.join(assignment).encode()).hexdigest() == \
    'ae5f99767bbff01c7f04d804140c1f09b5318ecec5ce4919547158abd1bf2967'
print('108 Pakete; 3692 Pfade vollständig zugeordnet; keine Eigentumsüberschneidung.')
```

## Arbeitsgrenzen eingehalten

Nur `INVENTAR.md`, `PAKETE.md` und `pakete.json` wurden durch diesen Worker geschrieben. Produktionscode und Hauptcheckout blieben unverändert. Keine Agenten, T3-Threads, Browser, Commits, Pushes, Builds, Tests, Merges, Deploys oder Dienstaktionen. Die Abdeckungsprüfung ist eine reine Metadatenprüfung und kein Anwendungstest.
