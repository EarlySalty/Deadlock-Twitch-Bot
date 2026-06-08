# bot-core / runtime — Architektur & Funktionsreferenz

> Pfad: `bot/` (Top-Level-Dateien) + `bot/runtime/` · Stand: 2026-06-08 · ~20 Dateien, ~5.800 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [ARCHITECTURE.md](../ARCHITECTURE.md) (Split-Runtime-Zielbild), [services.md](services.md) (Service-Entrypoints `bot_service`/`dashboard_service`), [storage.md](storage.md), [api.md](api.md). Dies ist die **Keystone-Doku**: sie erklärt, wie der Bot startet und wie die Subsysteme zusammengesteckt werden.

## 1. Zweck & Abgrenzung

Diese Schicht ist das **Prozess-, Bootstrap- und Runtime-Gerüst**. Sie beantwortet:

- **Wie startet der Bot?** Er ist eine **discord.py-Extension** des Master-Bots (`bot/__init__.py::setup`), kein eigenständiges Programm. Die zwei eigenständig startbaren Services (`bot_service`, `dashboard_service`) wrappen denselben Code — siehe [services.md](services.md).
- **Wie sind die Features verdrahtet?** Über **Mixin-Komposition**: `TwitchStreamCog` (`cog.py`) erbt von 12 Mixins (je ein Subsystem) plus `TwitchBaseCog` (`base.py`) als Fundament.
- **Wie wird der Split-Runtime erzwungen?** Über Rollen/Ports (`runtime_mode.py`) und Single-Instance-PID-Locks (`runtime_lock.py`); der Zustand wird in expliziten Runtime-Containern gehalten (`runtime/`), statt als geteiltes globales Objekt.
- **Querschnitt:** Secrets (`secret_store.py`), Logging (`logging_setup.py`), Hot-Reload einzelner Subsysteme (`reload_manager.py`), globaler Promo-Modus (`promo_mode.py`), Discord-Live-Rolle (`discord_role_sync.py`).

Abgrenzung: Hier liegt **keine** Feature-Logik (kein Raid-Scoring, keine Analytics-Queries). Diese Schicht orchestriert nur Start, Stopp, Reload und stellt die gemeinsamen Bausteine bereit.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Eintritt von außen** | Master-Bot lädt die Extension via `setup(bot)`; alternativ starten `bot_service`/`dashboard_service` den jeweiligen Runtime-Pfad. |
| **Bindet ein** | alle Feature-Mixins (`monitoring`, `raid`, `analytics`, `dashboard`, `chat`, `community`, `highlight_clipper`, …) — siehe `cog.py`. |
| **Nutzt** | `storage` (DB-Bootstrap), `api` (Token-Manager), `chat` (Chat-Bot-Erstellung, IRC-Lurker-Tracker), `internal_api` (Host/Runner), `bot_core.boot_profile` aus dem Master-Bot-Paket (Boot-Profiling, optional). |
| **DB-Tabellen** | `promo_mode`-Konfiguration (über `promo_mode.py`); ansonsten indirekt über die Subsysteme. |
| **Externe Dienste** | Discord (Cog, Rollen, Slash-Commands), Twitch (über `api`/`chat`). |
| **Secret-Namen** | Twitch-Client-ID/-Secret, Bot-Client-ID/-Secret (über `secret_store`/`SharedRuntimeConfig`); keyring-Service aus `KEYRING_SERVICE_NAME`. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `base.py` | 2417 | `TwitchBaseCog` — Lebenszyklus-Fundament: DB-Warmup, interne API, Chat-Bot-Init, Hintergrund-Task-Verwaltung, persistente Views. |
| `runtime_bootstrap.py` | 980 | `BotRuntimeBootstrap` / `DashboardRuntimeBootstrap` — explizite Start-/Stopp-Stages, Port-Bind-Checks, Worker-Verwaltung. |
| `promo_mode.py` | 370 | Globaler Promo-Modus (DB-Konfig laden/speichern). |
| `reload_manager.py` | 263 | Hot-Reload einzelner Subsysteme ohne Cog-Neustart. |
| `runtime_lock.py` | 205 | Single-Instance-PID/File-Lock pro Service+Port. |
| `__init__.py` | 205 | discord.py-Extension-Eintritt (`setup`/`teardown`, `!twl`-Proxy, App-Command-Sync). |
| `discord_role_sync.py` | 199 | Sync der „Live-Streamer“-Discord-Rolle. |
| `runtime_mode.py` | 152 | Rollen-/Port-Härtung für die getrennten Services. |
| `logging_setup.py` | 136 | Log-Verzeichnisse/-Dateien, Test-Runtime-Erkennung. |
| `reload_mixin.py` | 136 | Slash-Commands `/twitch-reload`, `/twitch-status`. |
| `secret_store.py` | 81 | Secret-Lookup aus keyring oder ENV. |
| `runtime_security.py` | 53 | Loopback-Guards (z. B. No-Auth nur lokal). |
| `cog.py` | 47 | `TwitchStreamCog` — Mixin-Komposition aller Subsysteme. |
| `app_keys.py` | 31 | aiohttp-App-Keys zum Durchreichen von Runtime-Objekten. |
| `runtime_state.py` | 11 | Kompatibilitäts-Wrapper auf die Split-Runtime-Contracts. |
| `runtime/bot_runtime.py` | 270 | Bot-Runtime-Contract: Config/Services/State/Container. |
| `runtime/dashboard_runtime.py` | 156 | `DashboardBotService` — dashboard-sichere Sicht auf bot-eigene Dienste. |
| `runtime/contracts.py` | 67 | Fassade über die Runtime-Contracts. |
| `runtime/shared_config.py` | 37 | `SharedRuntimeConfig` — geteilte reine Konfigwerte. |
| `runtime/__init__.py` | 8 | Package der Split-Runtime-Contracts. |

## 4. Datenfluss / Lebenszyklus

**Boot als Extension:**
1. Master-Bot ruft `setup(bot)` (`__init__.py`) → erzeugt `TwitchStreamCog` und fügt ihn dem Bot hinzu, registriert den `!twl`-Proxy-Command.
2. discord.py ruft `TwitchBaseCog.cog_load()` → `BotRuntimeBootstrap.configure_runtime()` + `wire_runtime_dependencies()` bauen Config/State auf, registrieren den Reload-Manager und starten ggf. Social-Media-Worker; `start_runtime()` startet die interne API und die Monitoring-/Loop-Tasks.
3. Nach `on_ready`: `_register_views_after_ready()` registriert persistente Discord-Views (z. B. Raid-Auth-Buttons), `_startup_db_warmup()` wärmt die DB-Verbindung erst **nach** Bot-Ready, um den Start nicht zu blockieren.
4. Laufend: `_scout_deadlock_channels()` sucht periodisch live deutsche Deadlock-Streams und joint sie; Hintergrund-Tasks werden über `_spawn_bg_task`/`_track_bg_task` registriert und bei `cog_unload`/`stop_runtime` via `_cancel_managed_bg_tasks` sauber beendet.

**Split-Runtime-Härtung:** Beim Start eines Service prüft `runtime_mode` die erwartete **Rolle** (`ROLE_TWITCH_WORKER`, `ROLE_DASHBOARD`, `ROLE_MASTER`) und den erwarteten **Port** (`enforce_dashboard_service_runtime`, `enforce_internal_api_runtime`); bei Abweichung gibt es eine klare Fehlermeldung. Parallel verhindert `runtime_lock.RuntimePidLock` (pro Service+Port) einen Doppelstart.

**Runtime-State (kein globales Objekt):** Statt einer geteilten Systemzentrale hält `runtime/bot_runtime.py` einen `BotRuntimeContainer` (Config/Services/State). Die Dashboard-Seite bekommt nur eine **lesende, sichere Sicht** über `DashboardBotService` (`runtime/dashboard_runtime.py`) — dessen Properties (`auth_manager`, `discord_bot`, `chat_bot`, `token_manager`, `clip_manager`) liefern `None`, wenn der jeweilige Dienst im Dashboard-Prozess nicht existiert. `runtime_state.py` ist nur noch ein Kompatibilitäts-Shim für die Bot-Seite.

**Hot-Reload:** `/twitch-reload <subsystem>` (`reload_mixin`) ruft `TwitchReloadManager.reload(name)`: Loops canceln → passende Module aus `sys.modules` purgen → neu importieren → frische `tasks.Loop` aus dem neuen Code an die **lebende** Cog-Instanz rebinden. So lässt sich ein einzelnes Subsystem im laufenden Betrieb aktualisieren.

## 5. Funktionsreferenz pro Datei

### __init__.py
- `setup(bot)` — fügt `TwitchStreamCog` dem Master-Bot hinzu und registriert den `!twl`-Proxy-Command.
- `teardown(bot)` — Aufräumen beim Entladen der Extension.
- `_sync_app_commands_after_ready(bot)` — sorgt dafür, dass Hybrid-/App-Commands nach (Re-)Load synchronisiert werden.
- `_command_payload_hash(bot, guild)` — stabiler Hash des aktuellen App-Command-Payloads (vermeidet unnötige Syncs).
- `_invoke_twitch_leaderboard_callback(leaderboard_cb, ctx, *, filters)` — Brücke für den `!twl`-Proxy zum Leaderboard.
- `_env_bool(name, default=False)` / `_parse_sync_guild_ids(raw) -> list[int]` — ENV-Helfer (u. a. Parsen der komma-/leerzeichengetrennten Guild-ID-Liste für den App-Command-Sync).

### cog.py
- `TwitchStreamCog(HighlightClipperMixin, LegacyTokenAnalyticsMixin, TwitchAnalyticsMixin, TwitchRaidMixin, RaidCommandsMixin, TwitchPartnerRecruitMixin, TwitchDashboardMixin, TwitchLeaderboardMixin, TwitchAdminMixin, TwitchMonitoringMixin, TwitchReloadMixin, TwitchBaseCog)` — die Komposition **ist** die Architektur: Reihenfolge = MRO. `TwitchBaseCog` steht ganz hinten (Fundament), die Feature-Mixins davor.

### base.py
`TwitchBaseCog` — gemeinsame Basis aller Mixins; besitzt Lebenszyklus, DB, Chat-Bot und Hintergrund-Tasks. Bindet `BotRuntimeBootstrap` ein und integriert sich optional in `bot_core.boot_profile` (Boot-Profiling des Master-Bots).
- `cog_load()` / `cog_unload()` — discord.py-Lifecycle-Hooks: Runtime hochfahren bzw. sauber herunterfahren.
- `_startup_db_warmup()` — leichtgewichtiges DB-/Session-Warmup **nach** Bot-Ready.
- `_register_views_after_ready()` / `_register_persistent_raid_auth_views()` — persistente Discord-Views (Raid-Auth-Buttons) erst nach Ready registrieren.
- `_scout_deadlock_channels()` — periodischer Scout für live deutsche Deadlock-Streams (Join-Entscheidung).
- `_prime_monitored_only_sessions(*, streams, logins)` — legt Sessions für frisch entdeckte Monitored-Only-Kanäle an.
- `_start_internal_api()` / `_stop_internal_api()` — interne API hosten/stoppen.
- Hintergrund-Tasks: `_spawn_bg_task(coro, name)`, `_track_bg_task(task)`, `_managed_bg_task_registry()`, `_cancel_managed_bg_tasks()`.
- `set_prefix_command(command)` — merkt sich den dynamisch registrierten Prefix-Command.
- Helfer: `_observability_flow_id(prefix)`, `_observability_sample(values, *, limit=8)` (für strukturierte Entscheidungs-Traces, siehe `insert_observability_event` in [storage.md](storage.md)).

### runtime_bootstrap.py
- `BotRuntimeBootstrap(cog)` — besitzt die Bot-Start-/Stopp-Stages explizit:
  - `configure_runtime()` / `wire_runtime_dependencies()` — Config/State aufbauen und verdrahten.
  - `_configure_dashboard_compat_attrs()` — Legacy-Dashboard-Attribute befüllen (Übergangs-Kompatibilität).
  - `_ensure_social_media_workers()` / `_stop_social_media_workers()` — Clip-/Upload-Worker starten/stoppen.
  - `_register_reload_manager()` — Subsysteme beim Reload-Manager anmelden.
  - `_runtime_lifecycle_lock()` — asyncio-Lock gegen parallele Start/Stopp.
  - `_can_bind_port_async(host, port)` / `_wait_for_port_release(*, host, port, component)` — Port-Bind-Prüfung/-Warten (verhindert „Address already in use“).
  - `_cleanup_runtime_components(*, wait_for_port_release, full_shutdown)`, `start_runtime()`, `stop_runtime()`.
- `DashboardRuntimeBootstrap(dashboard)` mit `configure_runtime()` — Pendant für den Dashboard-Pfad.
- `_install_runtime_managed_fields(owner)` — installiert die runtime-verwalteten Felder auf der Cog-Klasse.
- `_parse_env_bool/int/csv` — ENV-Parsing.

### runtime_mode.py
Rollen: `ROLE_MASTER`, `ROLE_TWITCH_WORKER`, `ROLE_DASHBOARD`. Ports: `DASHBOARD_SERVICE_PORT`, `MASTER_API_RESERVED_PORT`, `INTERNAL_API_PORT`.
- `split_runtime_enforced() -> bool` — ist der Split-Runtime-Modus aktiv?
- `resolve_runtime_role(value=None) -> str` — Rolle aus ENV/Argument normalisieren.
- `enforce_dashboard_service_runtime(*, role=None, port)` / `enforce_internal_api_runtime(*, role=None, port)` — prüfen Rolle+Port und werfen bei Mismatch eine erklärende Meldung (`_enforce_service_runtime`, `_role_error_message`, `_port_error_message`).

### runtime_lock.py
- `RuntimePidLock(service_name, *, port, lock_dir=None)` — Context-Manager, der eine Lock-Datei für die gesamte Prozesslebensdauer hält (`__enter__`/`__exit__`, `acquire`/`release`). Schreibt Metadaten (PID, Port).
- `runtime_pid_lock(service_name, *, port, lock_dir=None)` — Factory; erzwingt **eine** Instanz pro Service+Port.
- `RuntimeInstanceLockError` — wird geworfen, wenn bereits eine Instanz den Lock hält.
- Helfer: `_default_lock_dir`, `_lock_metadata`, `_read/_write_metadata`, `_acquire/_release_file_lock`.

### runtime_security.py
- `is_loopback_host(raw) -> bool` / `host_without_port(raw) -> str` — Host-Analyse.
- `require_noauth_loopback_guard(*, enabled, host)` — wirft, wenn No-Auth aktiviert ist, aber der Host **nicht** loopback ist (verhindert ein offenes, ungeschütztes Dashboard).

### runtime_state.py
Kompatibilitäts-Wrapper, der die Symbole aus `runtime/` re-exportiert (Übergang weg vom alten globalen Container).

### reload_manager.py
- `LoopSpec` / `SubsystemDef` / `SubsystemState` — Beschreibung eines hot-reloadbaren Subsystems (Module + Loops + Zustand).
- `TwitchReloadManager(cog)` — `register(subsystem)`, `get_subsystem(name)`, `get_all_names()`, `get_all_states()`, `reload(name) -> (success, message)`.
  - `_cancel_loops(sub)` — Loops canceln und auf Task-Ende warten.
  - `_purge_modules(sub)` — passende Module aus `sys.modules` entfernen.
  - `_reimport_modules(sub)` — frisch importieren.
  - `_find_fresh_loop_descriptor(...)` / `_rebind_loop(loop_spec, fresh_modules)` — neue `tasks.Loop` bauen und an die lebende Cog binden.

### reload_mixin.py
`TwitchReloadMixin` (Slash-Commands):
- `cmd_twitch_reload(interaction, subsystem)` — `/twitch-reload`: ein Subsystem hot-reloaden.
- `cmd_twitch_status(interaction)` — `/twitch-status`: Laufzeit-Status aller Subsysteme (mit `_STATE_EMOJI`).

### promo_mode.py
- `load_global_promo_mode(conn) -> dict` — globale Promo-Modus-Konfig laden.
- `save_global_promo_mode(conn, *, config, updated_by) -> dict` — Konfig speichern (mit Audit `updated_by`).

### secret_store.py
- `keyring_enabled() -> bool` — keyring auf Windows standardmäßig erlaubt, sonst nur per Opt-in.
- `read_keyring_secret(key) -> str` — getrimmtes Secret aus dem Windows Credential Manager (falls verfügbar).
- `load_secret_value(*keys, prefer_env=False, allow_empty_env_override=False) -> str` — erstes Treffer-Secret aus keyring oder ENV; `prefer_env` dreht die Reihenfolge.
- Konstante `KEYRING_SERVICE_NAME`.

### logging_setup.py
- `project_root()` / `logs_dir()` / `log_path(filename)` — Pfade zu Projekt-Root und Log-Verzeichnis/-Datei.
- `_looks_like_test_runtime()` — erkennt Test-Läufe und schreibt Logs dann in ein Test-Unterverzeichnis. Verwaltet die bekannten Log-Dateinamen (`_MANAGED_TWITCH_LOG_FILENAMES`).

### discord_role_sync.py
- `sync_streamer_role(discord_bot, discord_user_id, *, should_have_role, reason, logger=None) -> bool` — vergibt/entzieht die Live-Streamer-Discord-Rolle (await).
- `schedule_streamer_role_sync(discord_bot, discord_user_id, *, should_have_role, reason, task_name="twitch.streamer_role_sync", logger=None) -> bool` — feuert den Sync als Hintergrund-Task (fire-and-forget).

### app_keys.py
aiohttp-`AppKey`-Konstanten zum Durchreichen von Runtime-Objekten in die Web-App, u. a. `BOT_API_CLIENT_KEY`, `ANALYTICS_DB_FINGERPRINT_KEY`, `…_DETAILS_KEY`, `…_MISMATCH_KEY`, `…_ERROR_KEY` (Fingerprint-Abgleich zwischen den Prozessen).

### runtime/shared_config.py
- `SharedRuntimeConfig` (`@dataclass(slots=True)`) — reine geteilte Konfigwerte: `client_id`, `client_secret`, `twitch_bot_client_id`, `twitch_bot_secret`, `required_marker_default`. Property-Aliase `twitch_client_id`/`twitch_client_secret`.

### runtime/bot_runtime.py
- `BotRuntimeConfig(SharedRuntimeConfig)`, `BotRuntimeServices`, `BotRuntimeState`, `LegacyRuntimeFieldSpec` — typisierte Container für Config/Dienste/Zustand.
- `BotRuntimeContainer` — `assign(**values)`, `get(legacy_name)`, `delete(legacy_name)`, `legacy_snapshot()` (Brücke für alte Attributzugriffe).
- `ensure_bot_runtime_container(owner)` / `build_runtime_state(owner)` — Container an die Cog hängen/aufbauen.

### runtime/dashboard_runtime.py
- `DashboardBotService` — „dashboard-sichere“ Sicht auf bot-eigene Dienste; Properties `auth_manager`, `discord_bot`, `chat_bot`, `token_manager`, `clip_manager` liefern den Dienst **oder `None`** (das Dashboard besitzt nichts davon selbst).

### runtime/contracts.py
Fassade, die die Contract-Typen aus `bot_runtime`/`dashboard_runtime`/`shared_config` an einer Stelle bündelt (für Importe ohne Detailkenntnis der Aufteilung).

## 6. Datenbank & externe Schnittstellen

- **DB:** Promo-Modus-Konfig (`promo_mode.py`); Boot/Runtime selbst hält keinen eigenen großen Tabellensatz.
- **Discord:** Cog-Registrierung, persistente Views, Live-Rolle (`discord_role_sync`), Slash-Commands (`/twitch-reload`, `/twitch-status`).
- **Dateisystem:** Lock-Dateien (`runtime_lock`), Log-Dateien (`logging_setup`).
- **Master-Bot:** `bot_core.boot_profile.log_event` (optional, Boot-Profiling).
- **Secrets:** über `secret_store`/`SharedRuntimeConfig` (Twitch-Client-/Bot-Credentials), keyring/ENV.

## 7. Stolperfallen / Besonderheiten

- **Die MRO ist die Architektur:** Eine falsche Mixin-Reihenfolge in `cog.py` kann Methoden überschreiben. `TwitchBaseCog` muss das letzte Basis-Mixin bleiben.
- **`runtime_state.py` ist nur noch ein Shim:** Neuer Code soll die expliziten Container in `runtime/` nutzen, nicht das alte globale Objekt.
- **Single-Instance ist Service+Port-gebunden:** Zwei Prozesse mit gleicher Rolle **und** gleichem Port kollidieren am PID-Lock — gewollt. Verschiedene Rollen/Ports dürfen koexistieren.
- **No-Auth nur lokal:** `require_noauth_loopback_guard` verhindert ein offenes Dashboard ohne Auth auf nicht-loopback Hosts. Den No-Auth-Schalter (`TWITCH_DASHBOARD_NOAUTH`) nie auf einem öffentlichen Host setzen.
- **Hot-Reload hat Grenzen:** `reload_manager` purgt/reimportiert Module und rebindet `tasks.Loop`s — Zustand in nicht erfassten Modulen oder offene Verbindungen überleben das nicht zwingend. Für tiefe Änderungen bleibt der volle Cog-/Service-Neustart sicherer.
- **DB-Warmup bewusst nach Ready:** `_startup_db_warmup` läuft erst nach `on_ready`, damit ein langsamer DB-Connect den Discord-Login nicht blockiert.
- **Dashboard besitzt keine Bot-Objekte:** Wenn `DashboardBotService`-Properties `None` liefern, ist das **kein Fehler**, sondern der Normalfall im reinen Dashboard-Prozess — Bot-Zustand kommt dort über die interne API (siehe [internal-api.md](internal-api.md)).
