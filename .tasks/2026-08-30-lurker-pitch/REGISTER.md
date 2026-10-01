status: aktiv
Datum: 2026-10-01

# Register

- Intent-Thread: `6a3e284e-bb7b-4a33-85cd-536fd9b190bd`
- Source-Branch/SHA: `feat/lurker-discord-pitch` / `6ff725ee6808eed2235429ed4cadddfa8f0deead`
- Arbeitsbranch/Start-SHA: `codex/luna-dispatch/deadlock-twitch-bot/feat-lurker-discord-pitch-6ff725ee` / `6ff725ee6808eed2235429ed4cadddfa8f0deead`
- Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-feat-lurker-discord-pitch-6ff725ee`
- Source-Worktree: `/home/nathanael/repos/tb-lurker-pitch`, unveränderte uncommittete/untracked Pfade laut Inventur: keine
- Source-Commit: `wip: lokale Arbeit vor Worktree-Bereinigung`
- Auftrag: `CONTRACT.md`
- Worker-Threads: keine

## Branchgruppe

- Die aktuelle gemeinsame Gruppe umfasst Deadlock-Twitch-Bot PR #1035 und Deadlock-Bots PR #472. Beide PRs sind offen. Abgleich am 2026-10-01: Twitch-Head `03c69ee4990bb33df305cb9b0b3992b9dec0528f`, Bots-Head `c8ab07997dbda1d8b669fa8b35c23d7ee383b8ac`.
- Der Gruppenintegrator ist Twitch-Branch-Luna, T3-Thread `e60a2e14-1b57-4700-bb31-bc0492f4487e`. Die frühere gemeinsame Pfadüberschneidung betrifft `chat_wiring.rs` und `fresh_schema_snapshot.txt`. Dieser Pitch-Branch bleibt separat erhalten; kein Einzelmerge oder Einzel-Gate. Unabhängige Intent-Abnahme und lokales Gate sind auf dem abgestimmten Gruppenstand auszuführen.
- Ein unabhängiger Coaching-Pitch-Branch wurde auf Commit `4d5c899a9da2bb4d6f5215e271736db4fb9426a7` geprüft. Keine gemeinsamen Dateipfade.

## Status

- Ausgangsstatus und Origin-Main geprüft. `origin/main` und Merge-Base: `14bc1f479e32394fe2977f8c8ef85e6c5bff66e1`; `origin/main` ist nachweislich Vorfahr des rebasierten Branches.
- Original-SHA `6ff725ee6808eed2235429ed4cadddfa8f0deead` bleibt im Archiv-Tag `archiv/2026-10-01/lurker-discord-pitch-pre-main-sync-b5185e93` erhalten; der Original-SHA ist Vorfahr dieses Tags. Rebase-Ergebnisse: `fdbc70df`, `a61ab02f`, `c8de263b`. Folgefixes sind im WIP-Commit `7c0c1c9d` gesichert; die Login-Alias-Korrektur ist mit den Folgeänderungen dieses Stands gesichert.
- Merge-Konflikte mit Main wurden aufgelöst, ohne die aktuellen Broadcaster-ID-, Plan-, Scope-, Reward- oder Channel-Points-Gates zu verwerfen. Pitch-Log und Migration verwenden `twitch_user_id`; Chatter-Identität bleibt `id:<id>` oder als Fallback `login:<lowercase_login>`.
- Der Pitch-Log-Key wird vor dem Send atomar beansprucht. Nur eine eindeutige Twitch-4xx-Ablehnung gibt den Claim frei; bei Transportfehlern, 5xx und Prozessabbruch bleibt er bestehen, um mögliche Doppelnachrichten zu verhindern. Ein Abbruch zwischen Claim und Send kann die Ansprache auslassen.
- LFG-Fremdkanäle bleiben vor Judge und Sender still; fehlende eigene Broadcaster-ID ist fail-closed. Dashboard-POSTs ohne Pitch-Feld lassen den bisherigen Pitch-Wert stehen und geben ihn nun auch in der POST-Antwort zurück.
- Unabhängiger Review am `c8de263b` meldete vier HIGH- und einen MEDIUM-Befund. Zwei frische Fixreviews fanden vier zusätzliche Punkte: Testfixture ohne Migration, Alias-Claim-Race, Query-Makro-Vertrag und falsche Unterdrückung bei Login-Neuvergabe. Die Korrekturen sind dokumentiert in `REVIEW.md`; der jüngste unabhängige read-only Review bestätigte die Alias-Korrektur ohne Befund.
- Der erste Cargo-Testlauf kompilierte `tb-chat`, brach dann bei `streamer_scope.rs` wegen des neuen Update-Felds ab. Testmacro und Testschema sind angepasst. Erneute Läufe starteten wegen des belegten `/tmp/deadlock-cargo-release.lock` nicht. Gezielter rustfmt- und Diff-Check nach der letzten Alias-Korrektur waren grün.
- PR #1035 und PR #472 sind offene Mitglieder der aktiven gemeinsamen Gruppe. Kein Einzel-Gate oder -Merge; die gemeinsame Intent-Abnahme und das lokale Gate müssen auf dem abgestimmten Gruppenstand erfolgen. Keine Produktions-DDL, kein Push, Merge, Deploy, Restart oder Live-Cutover in dieser Fortsetzung.
