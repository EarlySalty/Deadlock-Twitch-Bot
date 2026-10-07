# Merge-Gate

## Statusfehler-Fix durch weiteren frischen Fixer

Der bestätigte zweite Blocking-Fund ist umgesetzt: SQL-Fehler im Metadata-Leser und seinem vorgeschalteten Credential-Leser werden bis zur vorhandenen API-Fehlerantwort weitergegeben. Ein erfolgreicher leerer Lesezugriff bleibt von einem SQL-Fehler unterscheidbar. Der vorherige Ablauf-Fix und die Migration bleiben erhalten. PostgreSQL-Proben prüfen alle drei Plattformen und die beiden SQL-Fehlerstellen. Die neue Bildprobe `metadata-read-error.png` zeigt den bereits vorhandenen unbekannten Zustand ohne Verbinden- oder Trennaktionen; die vier Ablaufproben sind erneut geprüft. Details stehen oben in `EVIDENCE.md`.

Die nächste unveränderte Runde von `gate_hook.py --review` steht aus; das Modell muss `gpt-6.1-sol` bleiben. Ein weiterer BLOCK geht an einen neuen Fixer durch den Haupt-Orchestrator. Dieser Blatt-Fixer startet keinen zusätzlichen Worker oder Review-Thread. Kein Merge, keine Produktionsmigration und kein Deploy vor ALLOW.

## Zweites Codeurteil nach dem Ablauf-Fix

Aktuelles Urteil: **BLOCK**, Exit 1, unverändertes Modell `gpt-6.1-sol`, geprüfter HEAD `260bdfdc`. Aufruf über den unveränderten `gate_hook.py --review --repo /home/nathanael/.worktrees/tb-social-token-ablauf --base origin/main --head fix/social-token-ablauf`. Der Branch wurde vorher regulär auf `origin/main` bei `e0b0dbaf` rebased.

```text
[gpt-6.1-sol] BLOCK: The new status query hides database failures as disconnected accounts.

1. rust/crates/tb-social-media/src/credentials.rs:212 | BLOCKING: Metadata-query errors become `None`, producing `connected: false` for TikTok, YouTube, and Instagram. `platforms_status_handler` then returns HTTP 200 (`social_media.rs:1905`). The dashboard consequently offers “Connect” instead of showing an unknown state. Propagate this query failure to the API error response.
2. bot/dashboard_v2/src/pages/SocialMedia.tsx:1929 | NIT: Connection dates and renewal instructions visibly change, but no images accompany this review. Inspect the screenshots named in `EVIDENCE.md` to confirm the rendered states.
```

Befund 1 ist bestätigt. Der neue Metadata-Lesepfad macht bei `Err(error)` trotz Log aus dem Zustand `None`; daraus wird `connected: false`. Der API-Handler nimmt die Statusliste ohne Fehlerzweig und liefert HTTP 200. Ein weiterer frischer Fixer muss den Fehler bis zur API durchreichen und den vorhandenen unbekannten UI-Zustand nutzen. Dieser Ablauf-Fixer schreibt keinen eigenen Folgefix und startet keine weiteren Worker. Übergabe: `BRIEFING-FIXER-STATUS.md`.

Die Bildproben waren beim Gate-Aufruf unversionierte Dateien und damit nicht Teil des Diffs. Sie sind jetzt geprüft und werden mit dem Übergabecommit versioniert. `instagram-20-days.png` zeigt den verlangten Hinweis bei gesunder Instagram-Verbindung. `ui-proof.json` enthält vier DOM-Proben am nach dem Rebase gebauten Artefakt `index-C1sEecg0.js`.

Kein ALLOW, kein Merge, keine Produktionsmigration, kein Deploy oder Selbst-Settle. Die ursprünglichen Abschlussbedingungen in `TODO.md` bleiben bestehen.

## Folgefix im frischen Fixer

Der blockierende Befund ist im bestehenden Branch behoben. Der gemeinsame Sweep verwendet das Access-Ende bei Instagram und bei TikTok/YouTube ohne verschlüsseltes Refresh-Feld. Verlängerbare TikTok-/YouTube-Verbindungen behalten ihre Anbieter-Refresh-Frist. Der Statusleser berechnet `reauth_soon` nach derselben Fallunterscheidung, und die Verbindungskarte zeigt für nicht erneuerbare Zugänge das Access-Datum. `refresh_expires_at` bleibt das tatsächliche Anbieterfeld.

PostgreSQL-Proben mit fester Uhr prüfen beide nicht erneuerbaren Plattformen: bei genau sieben Tagen keine Vorwarnung, bei sechs Tagen ein Vorfall und eine DM, beim Ablauf Eskalation mit unverändertem Vorfalls- und Versandzeitpunkt. Auch ein Erstlauf direkt beim Ablauf wird geprüft. Verlängerbare Fälle mit abgelaufenem Access-Ende bleiben gesund. Der echte Bot-Adapter wurde mit demselben Sweep gegen PostgreSQL und den lokalen Broker geprüft: zwei Plattformen, je ein Request, keine Wiederholung nach Neustart oder Eskalation.

Die nicht blockierende Instagram-Anmerkung ist per gebautem Dashboard und Chromium geprüft. `instagram-20-days.png` und `ui-proof.json` zeigen eine gesunde Verbindung mit Hinweis zum 27. Oktober 2026 bei fester Uhr am 7. Oktober 2026. Das ausdrücklich beauftragte Fenster von weniger als 30 Tagen bleibt unverändert. Eine zweite Probe prüft die Grenze von genau 30 Tagen ohne Datumshinweis. Es wurden keine Produktionszugänge für eine DM-Probe verwendet.

Das erste Codeurteil von `gpt-6.1-sol` bleibt unten erhalten. Die Folgerunde steht oben; sie hat wegen des neuen Statusfehler-Befunds erneut BLOCK geurteilt.

## Erstes Codeurteil nach Host-Reparatur

Aktuelles Urteil: **BLOCK**, Exit 1, Modell `gpt-6.1-sol`, Branch-HEAD `a062d624`. Fetch und Rebase auf aktuelles `origin/main` waren erfolgreich; der Rebase hatte keine Änderungen. Der Orchestrator hat den Hostfehler als erschöpftes Container-Mount-Limit behoben. Die frühere Namespace-Meldung ist kein aktueller Blocker mehr.

```text
[gpt-6.1-sol] BLOCK: Reauth handling misses connections that have no refresh token.

1. rust/crates/tb-social-media/src/reauth.rs:67 | BLOCKING: Both TikTok and YouTube connections with neither a refresh token nor a refresh deadline are omitted from expiry handling. The sweep ignores their access expiry; refresh_worker.rs:74 also excludes them from refresh processing. These supported states can expire without creating a reauth incident or sending a DM, although the status code reports them as expired.
2. bot/dashboard_v2/src/pages/SocialMedia.tsx:1955 | NIT: Healthy Instagram connections show Please reconnect starting 30 days before expiry, while automatic refresh deliberately starts seven days before expiry. Users receive an unnecessary reconnect instruction for roughly 23 days. An image should cover this state.
```

Befund 1 ist bestätigt: Der Sweep liest das Refresh-Ende statt des endgültigen Access-Endes einer Verbindung ohne Refresh-Möglichkeit. Die Worker-Auswahl kann diese Zeilen nicht verlängern. Der Statusleser kennt den abgelaufenen Zustand dagegen bereits. Der Fix gehört in einen frischen Fixer-Thread aus der Pyramide; dieser Implementierer schreibt keinen eigenen Folgefix.

Befund 2 ist eine nicht blockierende Produktanmerkung. Die beauftragte Spezifikation verlangt das bekannte Verbindungsende bei weniger als 30 Tagen. Dieses Fenster wird nicht eigenmächtig geändert. Eine Bild- oder DOM-Probe soll den Instagram-Zustand abdecken, wenn die Prüfumgebung dies ermöglicht.

Die Übergabe steht in `BRIEFING-FIXER.md`. Weitere Gate-Runden müssen dasselbe Modell wie dieses erste tatsächliche Codeurteil verwenden. Vor ALLOW gibt es keinen Merge, keine Produktionsmigration und keinen Deploy.

## Runde 1 und technischer Wiederholungsversuch

Beide Aufrufe wurden mit Exit 2 beendet. Kein Modell hat geurteilt. Es gibt weder ALLOW noch einen inhaltlichen BLOCK. Merge und Deploy bleiben gesperrt.

Prüfbasis: `origin/main` bei `67786ba2`, Implementierungsbranch `fix/social-token-ablauf`, geprüfter Aufruf für Commit `0ee53ca2`. Das lokale `main` im fremd veränderten Haupt-Checkout steht noch bei `d8284816` und wurde nicht angefasst.

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-social-token-ablauf --base origin/main --head fix/social-token-ablauf

kein Modell der Kette hat geurteilt: gpt-6.1-sol: review_gate: codex failed: bwrap: Creating new namespace failed: Cannot allocate memory | claude-opus-5-5: gate_hook: Kritiker-Wrapper ohne BLOCK: unshare: unshare failed: Cannot allocate memory | grok-4.6: gate_hook: Kritiker-Wrapper ohne BLOCK: unshare: unshare failed: Cannot allocate memory
```

Das ist ein gemeinsamer Hostfehler beim Anlegen der Prüfumgebung, kein Anbieterlimit und kein Codeurteil. Beide vorgesehenen Aufrufe liefen über den unveränderten Hook. Es wurden keine Schutzregeln oder Namespace-Einstellungen geändert. Die lesende Hostprobe zeigte etwa 15 GB verfügbaren Speicher und sechs sichtbare User-Namespaces; daraus folgt keine konkrete Reparaturursache.

Die fachlichen Nachweise stehen in `EVIDENCE.md`, die Zustands- und Versandverträge in `CONTRACT.md`. Nach Wiederherstellung der Prüfumgebung muss der vorhandene Branch regulär durch den Gate. Dieser Blatt-Worker startet keinen neuen Fixer- oder Review-Thread. Der bestehende Hauptthread `d3a1741e-82bc-4a48-865b-2845c663dca7` bekommt den Blocker mit Branch und Nachweispfaden.
