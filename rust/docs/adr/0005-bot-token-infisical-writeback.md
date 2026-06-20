# ADR 0005 — Bot-Token-Write-Back nach Infisical

- **Status:** akzeptiert (2026-06-20)
- **Kontext-Doku:** [`0003-crypto-interop-or-reauth.md`](0003-crypto-interop-or-reauth.md), [`02-db-contract.md`](../02-db-contract.md)

## Kontext

Der `BotTokenManager` (`tb-chat/src/token.rs`) hält den Bot-User-Token
(`deutschedeadlockcommunity`) nach einem Refresh **nur in-memory**. Der
Access-Token-Snapshot in Infisical (`TWITCH_BOT_TOKEN`) wird nie aktualisiert;
der Refresh-Token (`TWITCH_BOT_REFRESH_TOKEN`) ebenso wenig. Das ist ein
1:1-Port des alten Python-Verhaltens — Python schrieb den Bot-Token **automatisch
gar nicht** zurück (der Write lief nur über das manuelle, interaktive
Browser-Reauth-Skript `scripts/reauth_bot_token.py`).

Beobachtete Folgen:

1. **Boot-Lärm:** Bei jedem Neustart liest der Bot den veralteten Infisical-Snapshot,
   `validate()` liefert HTTP 401, und es wird `WARN tb_chat::token:
   Env-Access-Token ungültig — boote über Refresh-Token` geloggt. Über 72 h:
   9 Neustarts → 9 dieser WARNs (1:1 zu den Restarts). Kein echter Fehlschlag —
   Self-Healing über den Refresh-Token funktioniert jedes Mal.
2. **Latentes echtes Risiko:** Twitch kann bei einem Refresh auch den
   Refresh-Token rotieren (`token.rs` behält ihn dann in-memory, persistiert ihn
   aber nicht). Solange Twitch den alten Refresh-Token gültig lässt, passiert
   nichts; widerruft Twitch ihn je nach Rotation, bootet der Bot nach einem
   Restart mit totem Refresh-Token → **echter Ausfall**.

Zur Abgrenzung: Die **Streamer/User-Token** werden bereits enterprise-sauber
persistiert — `tb-raid/src/token_refresher.rs` verschlüsselt Access- und
Refresh-Token (AAD-gebunden, `enc_kid='v1'`) und schreibt sie per
`UPDATE twitch_raid_auth` zurück. Diese Hälfte ist **fertig** und nicht Teil
dieser Entscheidung.

## Entscheidung

Der Bot persistiert seinen eigenen Token nach jedem erfolgreichen Refresh zurück
nach Infisical (`TWITCH_BOT_TOKEN` + `TWITCH_BOT_REFRESH_TOKEN`). Damit findet der
nächste Boot einen frischen Env-Token (kein 401-WARN) und Refresh-Token-Rotationen
gehen nicht verloren.

Technisch:

- **`SecretSink`-Trait** in `tb-chat`: `persist_bot_tokens(access, refresh)`.
  `BotTokenManager` kennt nur das Trait — kein HTTP/Infisical → unit-testbar
  (No-op-Impl im Test). Spiegelt das Trait-Muster von `token_refresher`
  (`HelixTokenClient`).
- **`InfisicalWriter`** implementiert `SecretSink`, gespiegelt 1:1 von
  `reauth_bot_token.py::_infisical_set`: `PATCH→POST`-Fallback auf
  `/api/v3/secrets/raw/{name}`, `Authorization: Bearer <write-token>`,
  Payload `{workspaceId, environment, secretPath, secretValue}`.
- **Einhängen** am Ende von `refresh_with()` (nach gesetztem State).
  **Best-effort**: Schreibfehler kippt den Chat nicht (Token lebt in-memory
  weiter), nur Error-Log mit HTTP-Status. Access-Token immer schreiben,
  Refresh-Token nur bei Änderung (minimiert Infisical-Versionen).
- **Graceful Degradation:** Fehlt das Write-Token/Config, ist der Sink `None`;
  der Bot loggt einmal `Token-Write-Back deaktiviert` und verhält sich **exakt
  wie heute**. Der Code geht dormant live und wird scharf, sobald das Token da ist.
- **Sicherheit:** Niemals Token-Werte loggen. Das `INFISICAL_WRITE_TOKEN` ist eine
  eigene Machine Identity, die **nur** auf `TWITCH_BOT_TOKEN` +
  `TWITCH_BOT_REFRESH_TOKEN` im richtigen Environment/Path schreiben darf
  (Least-Privilege, kleiner Blast-Radius). Standing-Read-Token bleibt read-only.

### Env-Variablen

| Variable | Zweck |
|---|---|
| `INFISICAL_API_URL` | Basis-URL (vorhanden) |
| `INFISICAL_PROJECT_ID` | workspaceId (vorhanden) |
| `INFISICAL_ENV` | Environment-Slug (vorhanden) |
| `INFISICAL_SECRET_PATH` | Secret-Pfad, Default `/` (vorhanden) |
| `INFISICAL_WRITE_TOKEN` | **neu** — scoped read+write; fehlt → Sink disabled |

## Alternativen

- **B — Entkoppelter Sidecar-Writer:** Bot POSTet Token an einen lokalen
  privilegierten Prozess, der nach Infisical flusht. Sauberste
  Least-Privilege-Trennung, aber ein weiterer Dienst für 2 Secrets — Overkill.
- **C — Nur Refresh-Token persistieren + WARN→INFO downstufen:** Minimaler
  Footprint, schließt das echte Rotations-Risiko, aber der kurzlebige
  Access-Token bleibt bewusst un-persistiert (Boot macht weiter 1 stillen
  Refresh). Verworfen, weil der explizite Wunsch ist, den Bot-Token
  zurückzuschreiben.

## Konsequenzen

- Boot-WARN verschwindet (nach erstem scharfen Lauf), Logs spiegeln echte Fehler.
- Refresh-Token-Rotation kann den Bot nicht mehr aussperren.
- Der Bot hält ein (eng gescoptes) Write-Token — bewusste, mitigierte Abwägung.
- ~6–8 Infisical-Writes/Tag (Access-Token-Lebensdauer ≈ 4 h). Vernachlässigbar.
