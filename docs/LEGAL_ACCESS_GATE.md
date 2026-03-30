# Legal Access Gate

Diese Doku beschreibt die oeffentliche Absicherung der Legal-Seiten unter:

- `/twitch/impressum`
- `/twitch/datenschutz`
- `/twitch/agb`
- `/twitch/legal/access`
- `/twitch/legal/verify`

Wichtig: Das Human-Gate schuetzt aktuell nur:

- `/twitch/impressum`
- `/twitch/datenschutz`

`/twitch/agb` ist oeffentlich, aber derzeit nicht an das Gate gebunden.

Ziel ist: Die verpflichtenden Legal-Seiten bleiben fuer Menschen oeffentlich erreichbar, werden aber gegen KI-Crawler und andere Bots abgesichert.

## Architektur

Es gibt zwei getrennte Ebenen:

1. Anwendungsebene in `bot/dashboard/admin/legal_mixin.py`
2. Public Routing in `C:/caddy/Caddyfile`

Der Dashboard-Service selbst laeuft lokal auf `127.0.0.1:8765` und liefert die Legal-Gate-Seite aus. Caddy ist fuer die oeffentliche Erreichbarkeit unter `https://twitch.earlysalty.com` zustaendig.

## Request-Flow

1. `GET /twitch/impressum` oder `GET /twitch/datenschutz`
2. Ohne gueltigen Gate-Cookie folgt ein Redirect auf `/twitch/legal/access?next=...`
3. Auf `/twitch/legal/access` wird die Turnstile-Seite gerendert
4. Das Formular sendet an `POST /twitch/legal/verify`
5. Der Server validiert den Turnstile-Token gegen Cloudflare
6. Bei Erfolg setzt der Server den Cookie `twitch_legal_gate`
7. Danach ist der Zugriff fuer kurze Zeit auf die Legal-Seiten freigeschaltet

## Erforderliche Secrets

Der Gate-Status ist nur dann `enabled`, wenn alle drei Secrets vorhanden sind:

- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`

Der Loader prueft zuerst Windows Credential Manager `DeadlockBot` und faellt danach auf Environment-Variablen zurueck.

### Bedeutung der Werte

- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
  Oeffentlicher Site Key aus Cloudflare Turnstile
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
  Geheimer Server-Key aus Cloudflare Turnstile
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`
  Eigenes lokales Secret fuer die HMAC-Signatur des Cookies `twitch_legal_gate`

`TWITCH_LEGAL_GATE_COOKIE_SECRET` ist kein Browser-Cookie und kein Cloudflare-Wert. Es ist nur ein langes zufaelliges Secret, das der Server intern zum Signieren verwendet.

Beispiel zum Erzeugen im Windows-Keyring:

```powershell
python -c "import keyring, secrets; keyring.set_password('DeadlockBot', 'TWITCH_LEGAL_GATE_COOKIE_SECRET', secrets.token_urlsafe(48))"
```

## Caddy-Anforderungen

Damit der oeffentliche Flow funktioniert, muss `C:/caddy/Caddyfile` beide Gate-Routen explizit an den Dashboard-Service weiterleiten:

- `GET /twitch/legal/access`
- `POST /twitch/legal/verify`

Wird einer dieser Pfade nicht erlaubt, antwortet Caddy mit dem Catch-all:

- Status: `404`
- Body: `Nicht erlaubt`

Zusetzlich muss die Domain-CSP Turnstile zulassen:

- `script-src ... https://challenges.cloudflare.com`
- `frame-src https://challenges.cloudflare.com`

Ohne diese Freigabe laedt das Turnstile-Widget im Browser nicht korrekt, und der Server sieht spaeter nur:

- `Turnstile verification failed.`

## Lokale Entwicklung

Es gibt zwei verschiedene Wege fuer lokale Arbeit:

### 1. Echte Gate-Logik testen

Nutze den echten Dashboard-Service auf `127.0.0.1:8765`.

Wichtig:

- der Service braucht alle drei Secrets
- Turnstile prueft serverseitig gegen Cloudflare
- die Hostname-Pruefung muss zum Request-Host passen

Mit Produktiv-Keys funktioniert `localhost` oder `127.0.0.1` haeufig nicht sinnvoll, wenn die Turnstile-Site nur fuer `twitch.earlysalty.com` konfiguriert ist.

### 2. Nur Legal-Inhalte lokal pruefen

Nutze die lokalen Preview-Tools:

- `python scripts/preview_legal_pages.py`
- `python scripts/export_legal_preview.py`

Diese Tools umgehen die produktive Human-Gate-Pruefung absichtlich nur fuer lokale Vorschau und statischen Export.

## Typische Fehlerbilder

### `Legal access gate is not configured.`

Ursache:

- mindestens eines der drei Secrets fehlt

Pruefen:

```powershell
python -c "from bot.secret_store import load_secret_value; keys=['TWITCH_LEGAL_TURNSTILE_SITE_KEY','TWITCH_LEGAL_TURNSTILE_SECRET_KEY','TWITCH_LEGAL_GATE_COOKIE_SECRET']; [print(k, bool(load_secret_value(k))) for k in keys]"
```

### `Nicht erlaubt` mit `404`

Ursache:

- Caddy blockiert den Pfad vor dem Dashboard-Service
- typischerweise fehlen `/twitch/legal/access` oder `/twitch/legal/verify` in der Allowlist

Pruefen:

```powershell
curl.exe -i "https://twitch.earlysalty.com/twitch/legal/access?next=/twitch/impressum"
```

Wenn derselbe Pfad lokal direkt gegen `127.0.0.1:8765` funktioniert, liegt das Problem im Reverse Proxy.

### `Turnstile verification failed.`

Haeufige Ursachen:

- Turnstile-Script oder Frame wird durch CSP blockiert
- Site Key und Secret Key gehoeren nicht zusammen
- `twitch.earlysalty.com` ist in Cloudflare Turnstile nicht als erlaubter Host konfiguriert
- leeres oder ungueltiges Formular-Token

Pruefen:

1. Browser-Konsole auf CSP-Fehler
2. Response-Header von `/twitch/legal/access` auf `script-src` und `frame-src`
3. Turnstile-Hostname-Konfiguration in Cloudflare

## Direkte Betriebspruefung

### Dashboard-Service direkt

```powershell
curl.exe -i "http://127.0.0.1:8765/twitch/legal/access?next=/twitch/impressum"
curl.exe -i "http://127.0.0.1:8765/twitch/impressum"
```

Erwartung:

- `/twitch/legal/access?...` -> `200 OK`
- `/twitch/impressum` -> `302 Found` nach `/twitch/legal/access?...`

### Oeffentliche Domain

```powershell
curl.exe -i "https://twitch.earlysalty.com/twitch/legal/access?next=/twitch/impressum"
curl.exe -i "https://twitch.earlysalty.com/twitch/impressum"
```

Erwartung:

- `/twitch/legal/access?...` -> `200 OK`
- `/twitch/impressum` -> `302 Found` nach `/twitch/legal/access?...`

## Cache- und Neustart-Hinweise

- Nach Secret-Aenderungen den Dashboard-Service neu starten
- Nach Caddy-Aenderungen immer `validate` und `reload` ausfuehren
- Nach CSP-Aenderungen Browser hart neu laden (`Ctrl+F5`)

## Relevante Dateien

- `bot/dashboard/admin/legal_mixin.py`
- `bot/dashboard/routes_billing.py`
- `bot/secret_store.py`
- `scripts/preview_legal_pages.py`
- `scripts/export_legal_preview.py`
- `tests/test_dashboard_legal_access.py`
- `tests/test_legal_preview_scripts.py`
- `C:/caddy/Caddyfile`
