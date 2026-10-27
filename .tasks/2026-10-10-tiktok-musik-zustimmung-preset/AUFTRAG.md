# Auftrag: TikTok-Musikzustimmung einmal im Standard speichern

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession). Rolle: Blatt-Worker, keine weiteren T3-Threads.

## Nutzerwunsch (wörtlich)
"Wir müssen nicht immer bestätigen, dass du den TT-Musikdingern zustimmst. Das kann man als Preset speichern, wenn man das einmal bewusst, und zwar beim Preset bauen, drückt."

Seite: https://deutsche-deadlock-community.de/twitch/social-media, Dialog "TikTok-Veröffentlichung".

## Bestand (origin/main 9ff9d2b3c)
- Frontend: `bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx`. State `consent`, Checkbox vor "Clip mit TikTok einplanen", `valid` verlangt `consent`, `change()` und `applyChoices()` setzen `consent` bei jeder Änderung zurück.
- Backend Presets: `rust/crates/tb-dashboard-api/src/handlers/social_media_tiktok_editor.rs`, `TikTokDraft` mit `deny_unknown_fields`, Schlüssel `tiktok_defaults:{twitch_user_id}`; Test `choices_exclude_consent_and_validate_incomplete_drafts` schließt Consent bisher bewusst aus.
- Queue: `rust/crates/tb-social-media/src/clip_queue.rs` verlangt `tiktok_post_options.consent = true`. Bleibt so, das Frontend schickt weiter `consent: true`.
- Texte: `bot/dashboard_v2/src/i18n/dictionary.ts`.

## Soll
1. Beim Speichern der Standardwerte ("Als Standard speichern") gibt es eine eigene, nicht vorausgewählte Checkbox: "Der Musiknutzung von TikTok für alle meine Clips zustimmen" mit Link auf die Bestätigung zur Musiknutzung. Nur wenn sie dort bewusst angehakt ist, speichert der Standard `music_consent: true` plus Zeitpunkt (`music_consent_at`, serverseitig gesetzt). Entwürfe (`kind=draft`) speichern keine Zustimmung.
2. "Meine Standardwerte übernehmen" mit gespeicherter Zustimmung setzt die Zustimmung für den Post. Änderungen an Beschreibung, Sichtbarkeit oder Interaktionen setzen eine Preset-Zustimmung nicht mehr zurück.
3. Ist die Zustimmung aus dem Standard aktiv, entfällt die Pflicht-Checkbox unten. Der Hinweissatz "Mit der Veröffentlichung stimmst du TikToks Bestätigung zur Musiknutzung zu." bleibt sichtbar über dem Einplanen-Knopf (die TikTok-Richtlinie verlangt diese Erklärung vor dem Posten), dazu ein kurzer Vermerk, dass die Zustimmung im Standard gespeichert ist. Widerruf: Standard ohne Haken neu speichern entfernt sie.
4. Ohne gespeicherte Zustimmung bleibt das bisherige Verhalten (Checkbox je Post).
5. Markenpartner-Fall (`partnerBrand`, Richtlinie für Markeninhalte) bleibt immer eine Zustimmung je Post und wird nicht im Standard gespeichert.
6. Kontowechsel: die Zustimmung im Standard an die TikTok-`platform_user_id` binden; passt sie nicht zum aktuellen Konto, gilt sie nicht.
7. Bestehende Standards ohne Feld bleiben lesbar (`#[serde(default)]`), Test anpassen statt löschen.

## Regeln
- Keine Code-Kommentare. Echte Umlaute in allen UI-Texten, keine Em-Dashes. Texte kurz.
- Eigener Worktree `~/.worktrees/tb-tiktok-musik-preset`, Branch `fix/tiktok-musik-zustimmung-preset` von origin/main. Den geteilten Checkout nicht umstellen. Build-Slot per `cargo-slot`, Toolchain rustc 1.97.
- Prüfen: cargo fmt, clippy, betroffene Tests in tb-dashboard-api; Frontend-Build und Lint in `bot/dashboard_v2`.
- Selbstprüfung mit `gate_hook.py --review`; bei BLOCK je Runde ein frischer Fixer-Subagent bis ALLOW.
- Abschluss nach Coding-Ablauf: Merge nach main (HEAD:main, Einzelschritte), Deploy über `deploy-twitch-release <sha>` inklusive Dashboard-Frontend, Neustart, Live-Prüfung im Dialog (Sichtprüfung per Moli, nie Brave), Branch und Worktree löschen, `t3-thread.py settle --selbst`.
- Bericht: Commit-SHA, Deploy-SHA, Live-Beweis, MERGEPROTOKOLL-Zeile.
