# Legal Access Gate

Diese Doku beschreibt die öffentliche Absicherung der Legal-Seiten unter:

- `/twitch/impressum`
- `/twitch/datenschutz`
- `/twitch/agb`
- `/twitch/legal/access`
- `/twitch/legal/verify`

Das Human-Gate schützt aktuell:

- `/twitch/impressum`
- `/twitch/datenschutz`
- `/twitch/agb`

Ziel ist: Die verpflichtenden Legal-Seiten bleiben für Menschen öffentlich erreichbar, werden aber gegen KI-Crawler und andere Bots abgesichert.

## Architektur

Es gibt zwei getrennte Ebenen:

1. Anwendungsebene in `bot/dashboard/admin/legal_mixin.py`
2. Public Routing in `C:/caddy/Caddyfile`

Der Dashboard-Service selbst läuft lokal auf `127.0.0.1:8765` und liefert die Legal-Gate-Seite aus. Caddy ist für die öffentliche Erreichbarkeit unter `https://deutsche-deadlock-community.de/twitch` zuständig.

## Request-Flow

1. `GET /twitch/impressum`, `GET /twitch/datenschutz` oder `GET /twitch/agb`
2. Ohne gültigen Gate-Cookie folgt ein Redirect auf `/twitch/legal/access?next=...`
3. Auf `/twitch/legal/access` wird die Turnstile-Seite gerendert
4. Das Formular sendet an `POST /twitch/legal/verify`
5. Der Server validiert den Turnstile-Token gegen Cloudflare
6. Bei Erfolg setzt der Server den Cookie `twitch_legal_gate`
7. Danach ist der Zugriff für kurze Zeit auf die Legal-Seiten freigeschaltet

## Erforderliche Secrets

Der Gate-Status ist nur dann `enabled`, wenn alle drei Secrets vorhanden sind:

- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`

Der Loader prüft zuerst Windows Credential Manager `DeadlockBot` und fällt danach auf Environment-Variablen zurück.

### Bedeutung der Werte

- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
  Öffentlicher Site Key aus Cloudflare Turnstile
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
  Geheimer Server-Key aus Cloudflare Turnstile
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`
  Eigenes lokales Secret für die HMAC-Signatur des Cookies `twitch_legal_gate`

`TWITCH_LEGAL_GATE_COOKIE_SECRET` ist kein Browser-Cookie und kein Cloudflare-Wert. Es ist nur ein langes zufälliges Secret, das der Server intern zum Signieren verwendet.

Beispiel zum Erzeugen im Windows-Keyring:

```powershell
python -c "import keyring, secrets; keyring.set_password('DeadlockBot', 'TWITCH_LEGAL_GATE_COOKIE_SECRET', secrets.token_urlsafe(48))"
```

## Caddy-Anforderungen

Damit der öffentliche Flow funktioniert, muss `C:/caddy/Caddyfile` beide Gate-Routen explizit an den Dashboard-Service weiterleiten:

- `GET /twitch/legal/access`
- `POST /twitch/legal/verify`

Wird einer dieser Pfade nicht erlaubt, antwortet Caddy mit dem Catch-all:

- Status: `404`
- Body: `Nicht erlaubt`

Zusätzlich muss die Domain-CSP Turnstile zulassen:

- `script-src ... https://challenges.cloudflare.com`
- `frame-src https://challenges.cloudflare.com`

Ohne diese Freigabe lädt das Turnstile-Widget im Browser nicht korrekt, und der Server sieht später nur:

- `Turnstile verification failed.`

## Lokale Entwicklung

Es gibt zwei verschiedene Wege für lokale Arbeit:

### 1. Echte Gate-Logik testen

Nutze den echten Dashboard-Service auf `127.0.0.1:8765`.

Wichtig:

- der Service braucht alle drei Secrets
- Turnstile prüft serverseitig gegen Cloudflare
- die Hostname-Prüfung muss zum Request-Host passen

Mit Produktiv-Keys funktioniert `localhost` oder `127.0.0.1` häufig nicht sinnvoll, wenn die Turnstile-Site nur für `deutsche-deadlock-community.de/twitch` konfiguriert ist.

### 2. Nur Legal-Inhalte lokal prüfen

Nutze die lokalen Preview-Tools:

- `python scripts/preview_legal_pages.py`
- `python scripts/export_legal_preview.py`

Diese Tools umgehen die produktive Human-Gate-Prüfung absichtlich nur für lokale Vorschau und statischen Export.

## Typische Fehlerbilder

### `Legal access gate is not configured.`

Ursache:

- mindestens eines der drei Secrets fehlt

Prüfen:

```powershell
python -c "from bot.secret_store import load_secret_value; keys=['TWITCH_LEGAL_TURNSTILE_SITE_KEY','TWITCH_LEGAL_TURNSTILE_SECRET_KEY','TWITCH_LEGAL_GATE_COOKIE_SECRET']; [print(k, bool(load_secret_value(k))) for k in keys]"
```

### `Nicht erlaubt` mit `404`

Ursache:

- Caddy blockiert den Pfad vor dem Dashboard-Service
- typischerweise fehlen `/twitch/legal/access` oder `/twitch/legal/verify` in der Allowlist

Prüfen:

```powershell
curl.exe -i "https://deutsche-deadlock-community.de/twitch/legal/access?next=/twitch/impressum"
```

Wenn derselbe Pfad lokal direkt gegen `127.0.0.1:8765` funktioniert, liegt das Problem im Reverse Proxy.

### `Turnstile verification failed.`

Häufige Ursachen:

- Turnstile-Script oder Frame wird durch CSP blockiert
- Site Key und Secret Key gehören nicht zusammen
- `deutsche-deadlock-community.de/twitch` ist in Cloudflare Turnstile nicht als erlaubter Host konfiguriert
- leeres oder ungültiges Formular-Token

Prüfen:

1. Browser-Konsole auf CSP-Fehler
2. Response-Header von `/twitch/legal/access` auf `script-src` und `frame-src`
3. Turnstile-Hostname-Konfiguration in Cloudflare

## Direkte Betriebsprüfung

### Dashboard-Service direkt

```powershell
curl.exe -i "http://127.0.0.1:8765/twitch/legal/access?next=/twitch/impressum"
curl.exe -i "http://127.0.0.1:8765/twitch/impressum"
curl.exe -i "http://127.0.0.1:8765/twitch/agb"
```

Erwartung:

- `/twitch/legal/access?...` -> `200 OK`
- `/twitch/impressum` -> `302 Found` nach `/twitch/legal/access?...`
- `/twitch/agb` -> `302 Found` nach `/twitch/legal/access?...`

### Oeffentliche Domain

```powershell
curl.exe -i "https://deutsche-deadlock-community.de/twitch/legal/access?next=/twitch/impressum"
curl.exe -i "https://deutsche-deadlock-community.de/twitch/impressum"
curl.exe -i "https://deutsche-deadlock-community.de/twitch/agb"
```

Erwartung:

- `/twitch/legal/access?...` -> `200 OK`
- `/twitch/impressum` -> `302 Found` nach `/twitch/legal/access?...`
- `/twitch/agb` -> `302 Found` nach `/twitch/legal/access?...`

## Cache- und Neustart-Hinweise

- Nach Secret-Änderungen den Dashboard-Service neu starten
- Nach Caddy-Änderungen immer `validate` und `reload` ausführen
- Nach CSP-Änderungen Browser hart neu laden (`Ctrl+F5`)

## Relevante Dateien

- `bot/dashboard/admin/legal_mixin.py`
- `bot/dashboard/routes_billing.py`
- `bot/secret_store.py`
- `scripts/preview_legal_pages.py`
- `scripts/export_legal_preview.py`
- `tests/test_dashboard_legal_access.py`
- `tests/test_legal_preview_scripts.py`
- `C:/caddy/Caddyfile`
