# Quelle, Grenzen und Zuordnung

Basis ist der vom Nutzer am 25.09.2026 bereitgestellte Bericht **„Architektur- und Maintainability-Review: Deadlock-Twitch-Bot“**, Upload `markdown(7).md` (985 nummerierte Zeilen in der bereitgestellten Textansicht). Dieser Quellennachweis enthält eine auftragsbezogene Zusammenfassung, nicht die vollständige Rohdatei.

Der Bericht bezeichnet sich selbst als **statische Repository-Analyse** (L35): keine gestartete Produktionsumgebung und keine lokale Coverage-Messung. Sein konkret analysierter Commit-SHA ist in der bereitgestellten Fassung nicht ausgewiesen. Die im Bericht eingebetteten Chat-Zitatmarker sind hier nicht als eigenständig überprüfte GitHub-Belege übernommen.

Der Dokumentationsbranch dieses Backlogs beginnt bei `cfd4095ba8852122af4acc350bac652de071db7d`. Das ist nur die **Planungsbasis**, kein Nachweis, dass sämtliche Review-Befunde auf diesem Commit weiterhin bestehen. Jeder Implementierungsauftrag muss zuerst aktuellen Code, vorhandene Tests, Issues und PRs abgleichen. Dateigrößen, Dependency-Versionen, Testanzahlen, CodeQL-Prozente und Limits aus dem Review werden nicht ungeprüft als aktuelle Messung behandelt.

Die IDs, PR-Schnitte, Abhängigkeiten und konkreten Abnahme-Checklisten sind die **Arbeitsplanung aus diesem Auftrag**. Die Zuordnung P0–P3 folgt soweit vorhanden der Maßnahmentabelle L677–692. TB-A05 und TB-A19 machen zusätzliche Empfehlungen des Berichts als eigene priorisierte Aufträge ausführbar; TB-A20 bleibt ausdrücklich optional. Die groben Engineering-Day-Schätzungen der Quelle werden nicht auf die kleineren Slices umgerechnet oder zu einer Kalenderzusage addiert.

## Abstimmung mit anderem Bestand

- Issue [#572](https://github.com/EarlySalty/Deadlock-Twitch-Bot/issues/572) enthält einen älteren umfassenden Rust-Review. Dieser Backlog übernimmt daraus keine zusätzlichen ungeprüften Bugs; Überschneidungen sind vor Implementierung zu verknüpfen.
- Die historischen CI-Issues [#738](https://github.com/EarlySalty/Deadlock-Twitch-Bot/issues/738), [#570](https://github.com/EarlySalty/Deadlock-Twitch-Bot/issues/570) und [#633](https://github.com/EarlySalty/Deadlock-Twitch-Bot/issues/633) sind Abgleichpunkte, kein Beleg eines heute weiterhin bestehenden CI-Ausfalls.
- Der Planungs-Checkout enthält die Merge-Commits zu PR [#976](https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/976) und [#977](https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/977), deren Titel Deploy-Preflight betreffen. TB-A19 muss den konkreten vorhandenen Scope prüfen und wiederverwenden; hier wurde deren Implementierung nicht erneut auditiert.
- **Zusätzlicher Abstimmungshinweis aus Projektkontext, nicht aus dem Review:** parallele Brain-Core-/Consumer-Vertragsarbeit respektieren. Die Reasoner-/LlmPort-Beispiele des Reviews sind kein Auftrag, einen bereits ersetzten direkten Modell-/RAG-Zugang neu einzubauen.

## Befundzuordnung

### R01 — Architekturgrenzen/Baseline

Quelle: L33; L182; L283–318; L694–716; L934.

Der Review empfiehlt vor weiteren Umbauten ein cargo-metadata-basiertes Gate mit begründeten temporären Ausnahmen. Horizontale Feature-Kanten werden als Kopplung beschrieben, nicht als tatsächliche Cargo-Zyklen.

### R02 — Rate-Limit-Bypass

Quelle: L543–572.

Der Review beschreibt einen produktiv kompilierten Pentest-Schalter und empfiehlt expliziten Testmodus beziehungsweise Startfehler in Production. Prozesslokale Limits werden separat als Restart-/Multi-Instance-Risiko beschrieben.

### R03 — Workspace-Tests/Coverage

Quelle: L478–521; L888–914.

Der Review unterscheidet vollständigen Build von selektiven Tests und empfiehlt vollständige Workspace-Tests. Coverage soll zuerst gemessen und report-only veröffentlicht, danach begründet geratcheted werden; es liegt keine gemessene Coverage-Prozentzahl vor.

### R04 — Security-PR-Gates

Quelle: L523–599; L932.

Die vorhandene Security-Toolchain soll nicht ersetzt werden. Schnelle PR-Prüfungen ergänzen umfassende wöchentliche Scans. Die im Review erwähnte CodeQL-Analysegrenze ist keine in diesem Auftrag erneut gemessene Kennzahl.

### R05 — AI-State/Session-Store

Quelle: L362–408; L718–749.

Vorgeschlagen sind zwei getrennte Schritte: Singleton zunächst verhaltensneutral injizieren, danach atomare persistente Limits und wiederherstellbare Sessions hinter einem Store-Port. InMemory-Adapter bleibt für Tests.

### R06 — Shared Policies

Quelle: L283–318; L751–774.

Das passive Lurker-Prädikat ist das konkrete Beispiel für ein reines Leaf ohne DB/Tokio/HTTP. Spam-Policy kann später separat folgen; tb-domain ist nur bei tatsächlich reiner Domänenlogik eine Alternative.

### R07 — Wakeups und Cache-Policy

Quelle: L410–429; L684.

Der Review beschreibt die globale Weak<Notify>-Registry und empfiehlt ein explizites Wakeup-Handle. Weitere globale Zustände sollen klassifiziert werden; nicht jeder Cache gehört automatisch in einen persistenten Store.

### R08 — Composition Root/Partner-SQL

Quelle: L320–360; L431–453; L776–793.

main.rs soll Boot, Komposition, Run und Shutdown koordinieren, nicht Tool-Resolution, Listener-Retry und direkte fachliche Partner-Queries vermischen. SQL bleibt beim zuständigen Feature hinter kleinen Ports.

### R09 — Capability-Wiring

Quelle: L795–814.

Chat-Wiring soll nach Token, Subscriptions, Moderation, Commands, Tracking, Promotions, Bans und Adaptern mit explizitem Lifecycle geschnitten werden; kein willkürlicher Dateisplit.

### R10 — Dashboard-API-Boundaries

Quelle: L252–281; L816–843.

Die API besitzt laut Review zu viele vermischte Rollen. Empfohlen wird interne vertikale Modularisierung mit Application-Fassaden vor einer eventuellen Crate-Extraktion.

### R11 — Featurelokale Persistenz

Quelle: L431–453; L688.

Application-/Domain-Logik soll kleine Repository-Ports konsumieren; der SQLx-Adapter und seine Queries bleiben fachlich zugeordnet. Alle Queries nach tb-db zu verschieben wird ausdrücklich nicht empfohlen.

### R12 — Frontend-Testmatrix

Quelle: L499–521; L916–928.

Browser-/Fuzz-/Calendar-/Viewer-Suites sind im Review getrennt vom Default-Testlauf beschrieben, Admin-Tests als schwächer abgesichert. Die Empfehlung ist gezielte CI-Anbindung und Ausbau, kein UI-Redesign.

### R13 — Legacy-/Contract-Abbau

Quelle: L669–671; L690; L934–936.

Der rückrollbare Strangler bleibt das Zielverfahren. Die Proxy-Fläche soll durch Contract-Tests und native Parität pro Route reduziert werden, nicht durch einen sofortigen pauschalen Fallback-Removal.

### R14 — Aktuelle Dokumentation

Quelle: L455–474; L691; L938–983.

Architekturdokumente sollen CURRENT/TRANSITIONAL/LEGACY, Geltungsbereich und validierten Stand ausweisen. Workspace, Runtime-Map und Cutover-Tabelle sollen einen eindeutigen Einstieg bilden.

### R15 — Reproduzierbarkeit/Provenienz

Quelle: L218–229; L653–667; L845–860; L930.

Empfohlen sind getestete Toolchain-/Image-Pins, bewusste Workspace-Dependency-Pflege sowie ein dokumentierter externer Brain-/Schema-Vertrag. Versionsangaben im Bericht sind keine automatische Upgrade-Empfehlung.

### R16 — Runtime-Dependencies/Release-Smoke

Quelle: L521; L631–650.

Externe Tools wie yt-dlp und ffmpeg sollen explizite Runtime-Abhängigkeiten sein. Ein Doctor/Preflight soll Konfiguration, DB/Migrationen, Tools, Pfade, Secret-Präsenz und Ports prüfen; das Release-Artefakt benötigt einen Smoke-Vertrag.

### R17 — Bedingte Inbox-Optimierung

Quelle: L601–629.

Der Review beobachtet sequenzielles spawn-und-await und schlägt Parallelität nur für nicht ordering-sensitive Arbeit vor. Das ist ein bedingter Verbesserungsvorschlag, keine gemessene Engpassdiagnose.
