# Merge-Gate

## Runde 1 und technischer Wiederholungsversuch

Beide Aufrufe wurden mit Exit 2 beendet. Kein Modell hat geurteilt. Es gibt weder ALLOW noch einen inhaltlichen BLOCK. Merge und Deploy bleiben gesperrt.

Prüfbasis: `origin/main` bei `67786ba2`, Implementierungsbranch `fix/social-token-ablauf`, geprüfter Aufruf für Commit `0ee53ca2`. Das lokale `main` im fremd veränderten Haupt-Checkout steht noch bei `d8284816` und wurde nicht angefasst.

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-social-token-ablauf --base origin/main --head fix/social-token-ablauf

kein Modell der Kette hat geurteilt: gpt-6.1-sol: review_gate: codex failed: bwrap: Creating new namespace failed: Cannot allocate memory | claude-opus-5-5: gate_hook: Kritiker-Wrapper ohne BLOCK: unshare: unshare failed: Cannot allocate memory | grok-4.6: gate_hook: Kritiker-Wrapper ohne BLOCK: unshare: unshare failed: Cannot allocate memory
```

Das ist ein gemeinsamer Hostfehler beim Anlegen der Prüfumgebung, kein Anbieterlimit und kein Codeurteil. Beide vorgesehenen Aufrufe liefen über den unveränderten Hook. Es wurden keine Schutzregeln oder Namespace-Einstellungen geändert. Die lesende Hostprobe zeigte etwa 15 GB verfügbaren Speicher und sechs sichtbare User-Namespaces; daraus folgt keine konkrete Reparaturursache.

Die fachlichen Nachweise stehen in `EVIDENCE.md`, die Zustands- und Versandverträge in `CONTRACT.md`. Nach Wiederherstellung der Prüfumgebung muss der vorhandene Branch regulär durch den Gate. Dieser Blatt-Worker startet keinen neuen Fixer- oder Review-Thread. Der bestehende Hauptthread `d3a1741e-82bc-4a48-865b-2845c663dca7` bekommt den Blocker mit Branch und Nachweispfaden.
