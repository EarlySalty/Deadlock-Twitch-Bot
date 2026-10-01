status: aktiv | Datum: 2026-10-01

# Auftrag: Promo-Delete-Detection abschließen

## Ziel

Nach Twitch-akzeptierten Promo-Ankündigungen zuverlässig erkennen, wenn Twitch die Nachricht löscht, und den bestehenden Bot-Log-Kanal informieren. Die Änderung bleibt auf den ursprünglichen Branchauftrag `promo-delete-detection-20260915` beschränkt.

## Stand

- Source-Head: `364ad1c8d92c6701163b69d7608675ad08e50270`
- Arbeitsbranch: `codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
- Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8`
- `main` und der offene PR #1035 enthalten die Promo-Delete-Detection semantisch nicht.

## Nötige Arbeit

1. Alle fünf offenen Gate-Befunde aus `REVIEW.md` beheben, ohne den Promo-Umfang zu erweitern.
2. Event-Reihenfolge, Hook-Weiterleitung, Alert-Zustellung und Subscription-Erwartungen mit gezielten Regressionstests absichern.
3. Vollständigen Eigenanteil gegen den ursprünglichen Promo-Löschintent prüfen, unabhängige Intent-Abnahme einholen und das aktuelle Review-Gate auf einem gesicherten Commit ausführen. Keine schwere Suite parallel laufen lassen.

## Grenzen

- Nur Rust-Code und auftragsbezogene Task-Artefakte in diesem Worktree ändern.
- Keine fremden Worktrees, uncommitteten Dateien, Secrets oder ENV-Dateien lesen oder verändern.
- Keine neuen Codekommentare, kein Refactoring und kein Formatierungsdurchlauf außerhalb der nötigen Änderungen.
- Eigenen WIP per Commit auf diesem Arbeitsbranch sichern. Kein Force-Push und keine Änderungen an `main`.
- Keine schwere Suite parallel starten.
- Keine Fremdbranch- oder TokenDB-Integration übernehmen. Gemeinsame Integration, Merge und Produktionsschritte nur gemäß Gruppenverantwortung und nach der dort erforderlichen Owner-Abnahme.
- PR #1035 und TokenDB-Branchüberschneidungen werden ausschließlich gemeinsam über `BRANCHGRUPPEN.md` koordiniert.

## Abnahme

- Gate-Befunde geschlossen und erneutes Gate-Urteil ALLOW.
- Unabhängige Intent-Abnahme und gemeinsame Gruppenabnahme mit benanntem Integrator organisiert.
- Produktionscutover bleibt bis zur TokenDB-Ownerfreigabe gesperrt.
