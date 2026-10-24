# Auftrag: Social-Media-Tokens, echtes Ablaufdatum und Neu-Verbinden-DM

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Genau dieser Auftrag, keine weiteren Threads oder Orchestratoren starten.
Stufe: mittel. Repo: Deadlock-Twitch-Bot. Eigener Worktree `~/.worktrees/tb-social-token-ablauf` von origin/main, Branch `fix/social-token-ablauf`.

## Befund (geprüft am 2026-10-07)

- Dashboard `bot/dashboard_v2/src/pages/SocialMedia.tsx` (um Zeile 1988) zeigt "Zugang läuft am {datum} ab." mit `token_expires_at`, also dem Ablauf des kurzen Access-Tokens. TikTok: 24 h, YouTube: 1 h. Beide werden vom `TokenRefreshWorker` (`rust/crates/tb-social-media/src/refresh_worker.rs`, gespawnt in `rust/bin/tb-bot/src/main.rs` um 1762) rechtzeitig erneuert, live belegt in `social_media_platform_auth` (`last_refreshed_at` läuft mit). Die Anzeige wirkt deshalb wie ein Fehler.
- Die TikTok-Antwort enthält `refresh_expires_in` (365 Tage), `oauth.rs` verwirft den Wert beim Code-Tausch und beim Refresh. Es gibt keine Spalte dafür.
- Instagram hat keinen Refresh-Token, das 60-Tage-Token verlängert sich selbst (Schwelle 7 Tage im Worker). YouTube-Refresh-Tokens laufen im Produktionsmodus nicht ab.
- Refresh-Fehler werden heute nur geloggt (Kommentar im Worker). Der Streamer merkt einen toten Zugang nicht.

## Ziel

1. Neue Migration (nie eine bestehende ändern): Spalte `refresh_expires_at` in `social_media_platform_auth`, dazu ein Zustand für "muss neu verbinden" (z. B. `needs_reauth` plus Zeitpunkt), falls nicht schon etwas Passendes existiert.
2. `oauth.rs`: `refresh_expires_in` beim Verbinden und bei jedem Refresh speichern (TikTok). Für Instagram gilt `token_expires_at` selbst als Verbindungsablauf. YouTube: NULL, also unbefristet.
3. Refresh-Worker: endgültige Fehler (z. B. `invalid_grant`, widerrufener Zugriff, abgelaufener Refresh-Token, Instagram-Token abgelaufen) setzen den Neu-Verbinden-Zustand. Vorübergehende Fehler (Netz, 5xx, Rate-Limit) setzen ihn nicht. Ein erfolgreiches Neu-Verbinden setzt ihn zurück. Ein Zugang, dessen `refresh_expires_at` in weniger als 7 Tagen erreicht ist, zählt ebenfalls als "bald neu verbinden".
4. Discord-DM an den Streamer beim Zustandswechsel, nach dem Vorbild der Twitch-Reauth-DM (Bestand suchen: `rust/crates/tb-raid/src/token_lifecycle.rs` `user_dm_token_error_text`, `rust/bin/tb-bot/src/reauth_reminder.rs`, vorhandenen DM-Versandweg und Discord-Zuordnung wiederverwenden, keinen zweiten bauen). Regeln: genau eine DM je Vorfall, nicht bei jedem Worker-Durchlauf, entprellt; Identität über Twitch-User-ID; Text in natürlichem Deutsch mit echten Umlauten, ohne internes Vokabular (kein Token, Autorisierung, OAuth), keine Gedankenstriche, Plattformname plus ein Link zum Social-Media-Dashboard zum Neu-Verbinden. Text über die Skills `rolle-doku-redakteur` und `no-em-dashes` prüfen.
5. Dashboard: Bei Plattformen mit automatischer Erneuerung steht "Verlängert sich automatisch." statt des Access-Token-Datums. Ein Datum erscheint nur, wenn der Verbindungsablauf (`refresh_expires_at` bzw. Instagram-Ablauf) in weniger als 30 Tagen liegt, als "Bitte bis {datum} neu verbinden." Im Neu-Verbinden-Zustand die bestehende Warnzeile "Zugang abgelaufen, bitte neu verbinden" nutzen. i18n-Wörterbuch nachziehen. API-Feld dafür im bestehenden Status-Endpunkt ergänzen.

## Grenzen

- Secrets nie im Klartext lesen oder loggen. Verschlüsselung (tb-crypto FieldCipher, AAD) unverändert lassen.
- Bot und Dashboard: `.sqlx`-Offline-Dateien und Schema-Snapshot `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` mitziehen.
- Keine Code-Kommentare schreiben.
- Prüfungen: `cargo fmt`, `cargo clippy`, betroffene `cargo test` (Toolchain siehe Memory tb-bot-build-toolchain, Test-DB laut Workspace-Doku), Dashboard-Build. Bestehende rote Tests als Baseline vorher messen.

## Abschluss (Agent 1)

Selbstprüfung mit `gate_hook.py --review`, dann Merge-Gate. Bei ALLOW: Merge nach main (`git push origin HEAD:main`, Einzelschritte), Prod-Migration als `postgres` von Hand (Bot migriert nicht selbst, Rechte an twitchbot und twitchdash, Eintrag in `_sqlx_migrations`), Release über `deploy-twitch-release <sha>`, Neustart Bot und Dashboard, Live-Beweis: TikTok-Zeile hat `refresh_expires_at` nach dem nächsten Refresh, Dashboard zeigt "Verlängert sich automatisch". DM-Pfad nur mit Test oder Wegwerf-Zeile beweisen, nie einen echten Zugang entwerten. Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, dann `t3-thread.py settle --selbst`.
