# Briefing: helix-hinweis-nicht-nachreichen

[Orchestrator] Paket A. Auftrag:
/home/nathanael/.worktrees/tb-helix-hinweis-stale/.tasks/2026-09-20-helix-hinweis-nicht-nachreichen/AUFTRAG.md
(vollständig lesen, dann bauen).

- Worktree: /home/nathanael/.worktrees/tb-helix-hinweis-stale (liegt schon,
  Branch `fix/helix-hinweis-nicht-nachreichen` auf `origin/main` =
  `818e21523b13648a26e76f150aba89ae1ad19a89`)
- Branch: fix/helix-hinweis-nicht-nachreichen
- Intent-Thread: 65d5c809-b313-4fec-b278-c94f43f7453b
- PATH: `/home/nathanael/.rustup/toolchains/*1.97*/bin` vor `/usr/bin`, nie
  `/usr/bin/cargo` (1.75).

## Referenz (wörtlich)

Live-Loop nach fünf stillen Helix-Fehlern, Text und Hinweisablage
`rust/bin/tb-stream-audit/src/main.rs:1133-1209`:

```
Err(fehler) => {
    helix_fehler += 1;
    ...
    if helix_fehler >= MAX_STILLE_VERSUCHE && !helix_gemeldet {
        let text = format!(
            "Coaching-Audit: Twitch-Abfrage scheitert dauerhaft (seit mindestens \
{MAX_STILLE_VERSUCHE} Anlaeufen). ..."
        );
        ...
        match dm_rohtext(&text, &schluessel).await {
            Ok(()) => {
                helix_gemeldet = true;
                hinweis_erledigt(&konfiguration, "helix-ausfall").await;
            }
            Err(...) => {
                hinweis_aufheben(&konfiguration, "helix-ausfall", &schluessel, &text)
                    .await;
            }
        }
    }
    ...
    continue;
}
if helix_fehler > 0 {
    hinweis_erledigt(&konfiguration, "helix-ausfall").await;
}
helix_fehler = 0;
```

Nachreichen beim Start `offene_hinweise_senden` `main.rs:3267-3300`:
`start-*` wird gelöscht ohne Versand. `helix-ausfall.json` läuft in
`dm_rohtext`. Genau das hat um 18:26 den Schlüssel
`20260920T001225Z-225879-helix-ausfall-1` (Vorfall 02:12) zugestellt,
obwohl Helix nach dem Start wieder antwortete.

Vorbild-Test `eine_alte_startmeldung_wird_nicht_verspaetet_nachgereicht`
`main.rs:5636-5652`.

## Scope-Zaun

Exakt dieser Auftrag, kein Refactoring, kein fmt außer den angefassten
Dateien, andere Doku im Repo ignorieren. Nur
`rust/bin/tb-stream-audit/src/main.rs` und der Architektur-Absatz
Fehlerverhalten. Keine Helix-Timeouts, keine Mitschnitte, keine anderen
Hinweisarten umbauen.

## Git

- HEAD Worktree: `818e21523b13648a26e76f150aba89ae1ad19a89` (origin/main)
- Geteilter Checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` steht auf
  einem anderen Branch, nicht anfassen.
- Commit und Push auf `origin HEAD:fix/helix-hinweis-nicht-nachreichen`
  erlaubt. Nicht nach main mergen, nicht force-pushen.

## Beweisziel

`offene_hinweise_senden` löscht einen abgelegten `helix-ausfall` und ruft
den Broker nicht. `cargo test -p tb-stream-audit-bin` im Worktree-`rust/`
mit Zahlen (passed/failed/ignored). Kein Test am Wortlaut der Admin-DM.

## Regeln

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben. Bestehende Kommentare in angefassten
  Dateien löschen, wenn es den Diff nicht aufbläht.
- Nur den eigenen Branch pushen, nie main.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Gedankenstriche.

## Bump-up

Wird das Paket größer als beschrieben, nicht weiterbauen. Nachricht in
diesen Thread, dann stoppen:

```
[Bump-up] Paket A: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-helix-hinweis-stale Offen: ...
```

## Fertigmeldung

Melden hier im Thread: Branch, Commits (SHA), geänderte Dateien, Testlauf
mit Baseline und Endstand (Zahlen), was offen ist. Danach stoppen, kein
Mitlaufen.
