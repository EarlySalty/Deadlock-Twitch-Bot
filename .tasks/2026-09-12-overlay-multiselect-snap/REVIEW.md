# Review: behaupteter fehlender Caster-Overlay-Grant

Prüfung: 12.09.2026, ca. 22:11–22:12 Uhr MESZ. Ergebnis: Beide BLOCKING-Befunde sind Falschpositive. Kein SQL-Fix erforderlich.

## Scope und Codebelege

Geprüfter HEAD: `84cdc1bfda34f1785cc4bfae916d8637db821f2e`.
Vergleichsbasis: `origin/main` = `3c80da06cd51f7f4ac9eb30245e9a12e70d8950f`.
Der Diff umfasst ausschließlich die vier Overlay-Dateien `OverlayBuilderSection.tsx`, `overlaySelection.ts`, `overlay.html` und `overlay.rs`. Rollenmatrix, Caster-Migration und `caster_overlay.rs` sind gegenüber origin/main unverändert. Der veraltete lokale main (`27d9482b`) wurde nicht als Basis verwendet; der gemeinsame Checkout wurde nicht angefasst.

- `ops/systemd/twitch-runtime-roles.sql:60–65`: Auf den anfänglichen Rechteentzug folgt in Zeile 62–63 `GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO twitchbot, twitchdash`. Das schließt die vorhandene Tabelle `twitch_caster_overlay` ein.
- Sämtliche späteren Entzüge geprüft: Zeile 71–76 betrifft nur `twitchlegacy`; 86–107 private Tabellen nur für `twitchbot`; 118–123 ausschließlich `twitch_sub_reminders`; 127–141 die fünf EventSub-Tabellen; 153–166 Feedback-Tabellen und deren Sequenzen; 174–186 Migrations-, Ownership- und Backup-Tabellen. Nichts davon entzieht Caster-Rechte.
- Zeile 192–195 regelt Default-Rechte für künftig erstellte Objekte, entzieht keine bereits erteilten Tabellenrechte. Zeile 199–204 betrifft ausschließlich `twitch_community_announcements`. Ein zusätzlicher namensbezogener Caster-Grant ist daher kein fehlender Bestandteil der Matrix.
- `ops/systemd/deadlock-twitch-migrate.service:13–14`: Nach `sqlx migrate run` wendet `ExecStartPost` die Rollenmatrix an. Die installierte Unit enthält denselben ExecStartPost. `twitch-runtime-roles.sql:190–191` dokumentiert diesen Ablauf.
- `rust/migrations/20260912190000_caster_overlay.sql:2–7` erstellt Tabelle und Singleton. Das Fehlen eines eigenen GRANT am Ende ist in diesem Ablauf kein Berechtigungsfehler. Die bestehende Migration bleibt unverändert.
- `rust/crates/tb-dashboard-api/src/handlers/caster_overlay.rs:69–74` liest die Tabelle; der öffentliche Handler ruft diesen Pfad in Zeile 114–115 auf. Der Schreibpfad nutzt UPDATE mit RETURNING in Zeile 104.

## Livebelege, ausschließlich lesend

`GET https://deutsche-deadlock-community.de/twitch/api/v2/public/caster-overlay` lieferte **HTTP 200**, Body: `{"revision":0,"slots":[null,null]}`.

Die aktive Dashboard-System-Unit meldet `User=twitchdash`, `ActiveState=active`, `SubState=running`.

Unabhängige Gegenprüfung: Laut vom Auftraggeber übermitteltem Peer-1999-Befund vom 12.09.2026, 20:12:10 UTC, besitzt `twitchdash` auf `public.twitch_caster_overlay` ebenfalls `SELECT=true`, `UPDATE=true`, `INSERT=true`. Die abschließende SQL-Herkunftsprüfung dieses Peers war zum Zeitpunkt der Mitteilung noch offen. Die untenstehenden SELECT-/UPDATE-Belege wurden in dieser Session selbst erhoben.

Lokaler Peer-Zugang ohne Secrets:

```sh
sudo -n -u twitchdash psql -X -w -h /var/run/postgresql -d twitch_analytics -v ON_ERROR_STOP=1
```

In `BEGIN READ ONLY` geprüft und mit `ROLLBACK` beendet:

```sql
SELECT current_user,
       has_table_privilege(current_user, 'public.twitch_caster_overlay', 'SELECT') AS can_select,
       has_table_privilege(current_user, 'public.twitch_caster_overlay', 'UPDATE') AS can_update;
-- twitchdash | t | t
SELECT id, revision FROM public.twitch_caster_overlay;
-- t | 0
```

Zusätzlich war `EXPLAIN (COSTS OFF) UPDATE twitch_caster_overlay SET scene = '{"roster":[],"slots":[null,null]}'::jsonb, revision = revision + 1 WHERE id = true AND revision = 0 RETURNING revision` unter derselben Rolle und in einer READ-ONLY-Transaktion erfolgreich: `Update on twitch_caster_overlay`, `Index Scan using twitch_caster_overlay_pkey`. Ohne ANALYZE, daher keine Ausführung und keine Datenänderung. Dies belegt die SQL-Berechtigungsprüfung des Schreibpfads; ein authentifizierter HTTP-Schreibtest wurde nicht durchgeführt.

Abgrenzung: Die installierte Migrations-Unit meldet für ihren letzten Lauf um 20:57:24 MESZ `Result=exit-code`, `ExecMainStatus=1`; ExecStartPost wurde dabei nicht gestartet. Daraus wird ausdrücklich kein erfolgreicher aktueller Migrationslauf abgeleitet. Ursache dieses gesonderten Laufstatus wurde außerhalb des eng begrenzten Auftrags nicht untersucht. Die tatsächlich vorhandenen Caster-Rechte und der erfolgreiche Live-Lesepfad sind unmittelbar geprüft.

## Konsequenz für die Gate-Nachprüfung

Beide Meldungen beruhen auf der falschen Annahme, nur ein explizit tabellenspezifischer GRANT könne die Rechte erteilen. Die vollständige Matrix und die Live-Rechte widerlegen die behauptete Ursache `permission denied`.

Dieser Commit ergänzt ausschließlich Review-Evidenz. Keine Änderung an Migrationen, Rollenmatrix, Produktcode oder Gatezustand; kein Push, Deploy, Neustart oder Releasebuild. Bestehende Overlay-Tests und UI-Suites wurden für diese Dokumentationsänderung nicht erneut ausgeführt. Reguläre Gate-Nachprüfung soll die beiden Meldungen anhand dieser Belege neu bewerten; diese Datei ersetzt keine Gate-Freigabe.
