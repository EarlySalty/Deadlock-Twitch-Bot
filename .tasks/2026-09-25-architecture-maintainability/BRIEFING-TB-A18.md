# TB-A18 — Toolchain-, Dependency- und Brain-Provenienz vereinheitlichen

Priorität: **P3** · Änderungsrisiko: **niedrig**
Status/Owner: [REGISTER.md](REGISTER.md) · Übersicht: [PLAN.md](PLAN.md)
Abhängigkeiten für Implementierung: [TB-A01](BRIEFING-TB-A01.md)
Quelle: [SOURCE.md](SOURCE.md), **R15**. Einstiegspunkte und Befunde vor Codeänderung am aktuellen main bestätigen.

## Ziel

Reproduzierbare Builds mit bewusstem Update-Verfahren statt beweglicher Toolchain-/Image-Bezüge.

## Betroffene Bereiche

rust-toolchain.toml; rust/Cargo.toml; Cargo-Manifeste/Lockfile; CI-DB-Images; Brain-Vertrag

## TODO

- [ ] Tatsächlich verwendete Rust-Toolchain und DB-/Timescale-Images prüfen, auf getestete Version/Digest festlegen und Update-Verfahren dokumentieren; optional future-stable nicht blockierend.
- [ ] Mehrfach verwendete crate-lokale Dependencies sinnvoll in workspace.dependencies zentralisieren, ohne Versionsänderungen mit bloßer Verschiebung zu vermischen.
- [ ] Externe Brain-Abhängigkeit mit Quelle, exaktem Contract-/Client-Stand, gegebenenfalls Schema-Snapshot und Provenienz dokumentieren; kompatibles Update im selben CI-Vertrag prüfen.
- [ ] Abstimmung aus Projektkontext: vorhandene Brain-Core-/Consumer-Arbeit prüfen. Einen bereits abgelösten direkten dbrain-reasoner-/Schema-Pfad nicht erneut einführen.

## Tests und Abnahme

Clean-Checkout-Build und isolierter Schema-Check nutzen dieselben Pins; Lockfile bleibt konsistent; absichtlich inkompatibler Brain-Vertrag wird erkannt.

- [ ] Befund und Base-SHA dokumentiert; bereits gelöste/überholte Punkte mit Beleg statt Doppelimplementierung abgeschlossen.
- [ ] Slice-spezifische Tests tatsächlich ausgeführt und CI-/Review-Ergebnis im Register verlinkt; nicht ausgeführte Runtime-Prüfungen separat offen.
- [ ] Bestehende Auth, Commandsemantik, Limits und Ausgabeformate erhalten, sofern dieser Auftrag nicht ausdrücklich eine Schutzkorrektur verlangt.
- [ ] Rücknahme des Slices und etwaige Daten-/Contract-Auswirkungen dokumentiert; betroffene Architektur-/Cutover-Doku aktualisiert.

## PR-Schnitt

PR A: Rust-/DB-Pins. PR B: Workspace-Dependencies. PR C: bestehender Brain-Vertrag/Provenienz ohne neuen Integrationsnebenweg.

Branch-Vorschlag: `codex/twitch-tb-a18-<kurzer-slice>`; von aktuellem main oder ausdrücklich vereinbartem Vorgänger-Branch. Den Dokumentationsbranch nicht als Produktcode-Basis missverstehen.

## Nicht im Scope

Keine aktuellen Versionsnummern aus dem Review als Upgrade-Ziel übernehmen. Externe ausführbare Runtime-Tools und Release-Smoke gehören zu TB-A19.

Es gilt [CONTRACT.md](CONTRACT.md): Die Erstellung dieses Backlogs implementiert nichts und autorisiert keine Produktionsaktionen.
