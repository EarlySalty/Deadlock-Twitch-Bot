# Briefing: partner-signup-tag-blocks

[Orchestrator] Paket partner-signup-tag-blocks. Auftrag: /home/nathanael/Documents/Deadlock-Twitch-Bot/.tasks/2026-09-14-partner-signup-tag-blocks/AUFTRAG.md
(vollständig lesen, dann bauen). Dieselbe Datei liegt auch im Worktree unter
.tasks/2026-09-14-partner-signup-tag-blocks/AUFTRAG.md.

- Worktree: /home/nathanael/.worktrees/tb-partner-signup-tag-blocks
- Branch: feat/partner-signup-tag-blocks
- Basis-SHA: 18fa335581fb25067b2615c0e88782a4248c02b9 (origin/main)
- Intent: Orchestrator-Session Grok. Bump-up und Fertigmeldung in DIESEM Thread.

## Pflichtteile

1. Referenzauszüge stehen im Auftrag unter Fundstellen, Schema, API, UI-Texte,
   Durchsetzung. Die bestehenden Module
   `tb_analytics::partner_signup_block` und
   `admin_partner_signup_block.rs` sind die Vorlage, kein paralleler Zustand.
2. Scope-Zaun: exakt dieser Auftrag, kein Refactoring, kein fmt, keine anderen
   Admin-Seiten. Andere Doku im Repo ignorieren.
3. Nur Branch `feat/partner-signup-tag-blocks` committen und nach origin pushen,
   nie main. Der Worktree ist sauber von origin/main. Uncommitteter Zustand im
   Documents-Checkout ist fremd, nicht anfassen.
4. Beweisziel: Admin-CRUD für Tags auf der bestehenden Partneraufnahme-Seite;
   Live-Stream mit gesperrtem Tag landet in `twitch_partner_signup_denylist`;
   Promote mit diesem Tag wird abgelehnt. Keine Testpflicht, bestehende Suites
   nicht zerbrechen.
5. Bump-up-Format wörtlich, dann stoppen:

```
[Bump-up] Paket partner-signup-tag-blocks: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-partner-signup-tag-blocks Offen: ...
```

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst; bestehende
  Kommentare in angefassten Dateien löschen, wenn es den Diff nicht aufbläht.
- Nur den eigenen Branch pushen, nie main. Nichts nach main mergen.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Em-Dashes.
- AUFTRAG.md ins Worktree-`.tasks/` übernehmen und mitcommitten.

## Fertigmeldung

Melden hier im Thread: Branch, Commits (SHA), geänderte Dateien, was geprüft
wurde, was offen ist. Danach stoppen, kein Mitlaufen.
