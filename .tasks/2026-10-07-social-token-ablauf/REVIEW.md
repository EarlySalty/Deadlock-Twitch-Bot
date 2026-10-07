# Merge-Gate

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
