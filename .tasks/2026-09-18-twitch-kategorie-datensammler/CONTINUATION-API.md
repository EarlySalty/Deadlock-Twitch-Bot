# Fortsetzung: isolierte Admin-API

18.09.2026, laufende Folge-Sitzung nach „weiter“.
Im bestehenden Feature-Arbeitsbaum werden parallel noch Änderungen geschrieben (u.a. channel_policy-Test). Deshalb keine fremden WIP-Dateien überschreiben.

Diese Folge-Sitzung besitzt jetzt ausschließlich die neue Admin-API `rust/crates/tb-dashboard-api/src/handlers/category_collector.rs` samt ihren Tests und die nötigen zwei Module-/Router-Einträge. Umsetzung in eigenem Worktree `/home/nathanael/repos/tb-category-admin-api-20260918`, Branch `feat/category-admin-api-20260918`, Basis `1c8dcb85`. API-Vertrag aus API-CONTRACT.md wird eingehalten. Integration per geprüftem Commit; keine Doppelimplementierung dieses Moduls im alten Worktree.

A/B und bestehende Orchestrator-Änderungen an Collector, Transport, Sendeschutz und Frontend bleiben unverändert. Noch kein Produktionsdeploy behauptet.
