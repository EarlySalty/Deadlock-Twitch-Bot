# Fortsetzung: Matchschutz und angefragte Builds

Stand: 24. September 2026. Intent-Session: dieser ChatGPT-Auftrag über codex-mcp, keine T3-Intent-ID. Im PR-first-Testbetrieb bleiben die vier PRs offen; kein Merge, Deploy, Produktions-Upload, Neustart oder Branch-Cleanup.

## Thread-Register (T3)

| Paket | Thread-ID | Modell | Status | Letzte Meldung |
|---|---|---|---|---|
| Review A+B, erster Start | 709f5813-a8d4-4dac-ace2-051a68fb5507 | claude-opus-4-8 | Anmeldung gescheitert, gesettelt, nicht wieder aufnehmen | OAuth abgelaufen, kein Urteil |
| Review A+B, Rollen-Fallback | 2da9b7e2-865e-477d-aad9-bce32cee9aa3 | claude-opus-4-8 | Anmeldung gescheitert, gesettelt, nicht wieder aufnehmen | Alternative Kontingente gesperrt, erneut kein Urteil |
| Früherer Matchschutz-Worker | a807ebbd-1d35-4955-b92a-6fd967e9305a | nicht bestätigt | keine Session, nicht wieder aufnehmen | read liefert session: None |

Gemeinsamer Review-Ort: `/home/nathanael/.worktrees/tb-admanager-steam-api-20260924`. Der erste Auftrag steht in REVIEW-AUFTRAG.md; es existiert kein erfolgreicher REVIEW-R2.md-Bericht.

## Gesicherte Implementierungen

- Steam-Producer: `7cfacba9d7db068fccb3e05a2de3f598cc1fa244`, PR EarlySalty/Deadlock-Steam-Bot#69.
- Twitch-Verbraucher: `5f92ca7c1aeb0c0ce1c841aac880bf80c4dec446`, PR EarlySalty/Deadlock-Twitch-Bot#958.
- Brain-CLI: `11664151b73e8dd7dd73f6f84e4aed8d9aa57171`, PR EarlySalty/Deadlock-Brain#11.
- Discord-Verbraucher mit Negationshärtung: `961cf15ef50d640e7a674f1d9cd87793f3817c44`, PR EarlySalty/Deadlock-Bots#451.

## Prüfstand

167 ausgewählte Tests erneut bestanden. Die vier Implementierungen sind keine Live-Auslieferung. Twitch hat zwei rote funktionale CI-Jobs; Steam und Discord scheiterten auch beim zweiten Startversuch am GitHub-Abrechnungs-/Ausgabenlimit. Brain hat in diesem Feature-Stand keinen funktionalen GitHub-Workflow. Unabhängiger Review ist durch die Claude-Anmeldung blockiert. Einzelbelege, Run-IDs und Grenzen stehen in CI-NACHPRUEFUNG.md und den EVIDENCE.md-Dateien. Keine Schutzmechanismen oder Abrechnungseinstellungen wurden geändert.
