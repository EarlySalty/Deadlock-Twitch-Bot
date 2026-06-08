# Architektur-Funktionsreferenz

Diese Sammlung dokumentiert den **Deadlock-Twitch-Bot pro Subsystem auf Funktions-Ebene** — was jedes Modul tut, wie es aufgebaut ist, welche Funktionen/Klassen es hat und wie die Daten fließen. Sie ist die **Tiefen-Schicht** und ergänzt die bestehenden Überblicks-Dokumente:

| Bestehende Doku | Ebene |
|-----------------|-------|
| [../ARCHITECTURE.md](../ARCHITECTURE.md) | System-Zielbild: Split-Runtime, Service-Vertrag |
| [../MODULES.md](../MODULES.md) | jede Datei in einem Satz + Zugriffslevel (A/S/I) |
| [../DATABASE.md](../DATABASE.md) | alle DB-Tabellen mit Spalten |
| [../API.md](../API.md) | alle HTTP-Routen mit Methode + Zugriffslevel |
| **dieser Ordner** | **funktionsgenaue Architektur je Subsystem** |

> Ziel: Code soll als Architektur lesbar sein. Wer ein Subsystem anfassen muss, findet hier zuerst das „Wie funktioniert das“, bevor er sich in 2.000+ Zeilen Code stürzt.

## System-Gesamtbild

Der Bot läuft als **Split-Runtime** aus zwei logisch getrennten Diensten, die sich **nur** über PostgreSQL und die interne API koordinieren — keine geteilten In-Memory-Objekte (Details: [../ARCHITECTURE.md](../ARCHITECTURE.md)):

```
            ┌─────────────────────────────┐        ┌──────────────────────────────┐
            │  BotRuntime (Twitch-Worker) │        │  DashboardRuntime            │
            │  bot_service/app.py         │        │  dashboard_service/app.py    │
            ├─────────────────────────────┤        ├──────────────────────────────┤
            │  monitoring/ (EventSub)     │        │  dashboard/ (aiohttp, Auth)  │
            │  raid/  chat/  community/   │        │  analytics/ (API v2)         │
            │  social_media/ clipper      │        │  Bot-/Internal-API-Clients   │
            │  internal_api/ (Host :8776) │◄──────►│  (HTTP, loopback)            │
            └──────────────┬──────────────┘  HTTP  └───────────────┬──────────────┘
                           │                                       │
                           └──────────────► PostgreSQL ◄───────────┘
                                     (gemeinsame Wahrheit)
```

Im Kern ist der Bot eine **discord.py-Extension**: `TwitchStreamCog` (`bot/cog.py`) wird aus 12 Feature-**Mixins** über `TwitchBaseCog` komponiert. Jedes Mixin ist ein Subsystem in diesem Verzeichnis. Start, Stopp, Rollen-/Port-Härtung und Hot-Reload liegen im [bot-core/runtime-Layer](bot-core.md).

## Doku-Konventionen

Jede Subsystem-Doku folgt demselben 7-Abschnitt-Template:

1. **Zweck & Abgrenzung** — was macht es, wo hört es auf.
2. **Einordnung & Abhängigkeiten** — Aufrufer, Aufgerufenes, DB-Tabellen, externe Dienste, Secret-Namen.
3. **Dateien im Überblick** — Tabelle Datei / Zeilen / Rolle.
4. **Datenfluss / Lebenszyklus** — Hauptabläufe mit Bedingungen, Schwellen, Reihenfolge.
5. **Funktionsreferenz pro Datei** — Klassen/Funktionen mit Signatur + Verhalten.
6. **Datenbank & externe Schnittstellen** — Tabellen, Routen, externe APIs, Secrets (nur Namen).
7. **Stolperfallen / Besonderheiten** — Races, Locks, Caches, bekannte Workarounds.

Sprache: Deutsch, echte Umlaute. Secret-**Werte** kommen nirgends vor — nur Namen.

## Modul-Index

Status: ✅ fertig · 🔜 geplant.

### Fundament

| Subsystem | Doku | Status | Inhalt |
|-----------|------|:------:|--------|
| `bot/` + `bot/runtime/` | [bot-core.md](bot-core.md) | ✅ | Prozessmodell, Bootstrap, Cog-Komposition, Hot-Reload, Locks, Secrets, Logging |
| `bot/storage/` | [storage.md](storage.md) | ✅ | Postgres-Layer: Pooling, Schema, Transaktionen, Partner-Lifecycle, Sessions |
| `bot/core/` | [core.md](core.md) | ✅ | Geteilte Helfer: Konstanten, Partner-Gate, HTTP-Client, LLM-Provider, Login-Normalisierung |
| `bot/api/` | [api.md](api.md) | ✅ | Twitch-Helix-Wrapper, Bot-Token-Manager, Token-Fehler-Lebenszyklus |

### Echtzeit-Layer

| Subsystem | Doku | Status | Inhalt |
|-----------|------|:------:|--------|
| `bot/monitoring/` | [monitoring.md](monitoring.md) | ✅ | EventSub (WS + Webhook), Stream-Sessions, Go-Live-Embeds |
| `bot/chat/` | [chat.md](chat.md) | ✅ | IRC/Chat-Bot, Moderation, Promos, Scam-Warnung, Lurker-Tracking |
| `bot/raid/` | [raid.md](raid.md) | ✅ | Auto-Raids, Partner-Scoring, OAuth, Blacklist, Recruitment |
| `bot/live_announce/` | [live-announce.md](live-announce.md) | ✅ | Live-Ankündigungs-Template-Engine |

### Analytics & Dashboard

| Subsystem | Doku | Status | Inhalt |
|-----------|------|:------:|--------|
| `bot/analytics/` | [analytics.md](analytics.md) | ✅ | Analytics-API v2, Coaching-Engine, Demo-Daten, Backend-Queries |
| `bot/dashboard/` | [dashboard.md](dashboard.md) | ✅ | aiohttp-App, Auth, Billing, Affiliate, Live (Backend); erweitert [dashboard/README.md](dashboard/README.md) |

### Feature-Module

| Subsystem | Doku | Status | Inhalt |
|-----------|------|:------:|--------|
| `bot/engagement/` | [engagement.md](engagement.md) | ✅ | MiniMax-Chat-Engagement, Threads, Persona, Wiki-Grounding |
| `bot/community/` | [community.md](community.md) | ✅ | Leaderboard, Partner-Recruit, Voice-Reaction (Claude) |
| `bot/social_media/` | [social-media.md](social-media.md) | ✅ | Clip-Pipeline, Uploads (TikTok/Instagram/YouTube), Approval, Enrichment |
| `bot/highlight_clipper/` | [highlight-clipper.md](highlight-clipper.md) | ✅ | Highlight-Erkennung, VOD-Analyse, Clip-Erstellung |
| `bot/title_generator/` | [title-generator.md](title-generator.md) | ✅ | KI-Titelgenerierung, Steam-Lookup |
| `bot/stream_coaching_audit/` | [stream-coaching-audit.md](stream-coaching-audit.md) | ✅ | Slur-/Coaching-Audit via Transkription |
| `bot/entitlements/` | [entitlements.md](entitlements.md) | ✅ | Plan-/Feature-Berechtigungen |

### Infrastruktur / Service-Layer

| Subsystem | Doku | Status | Inhalt |
|-----------|------|:------:|--------|
| `bot/internal_api/` | internal-api.md | 🔜 | Interne API (:8776): App, Routen, Policy, Contracts |
| `bot/bot_service/` + `bot/dashboard_service/` | services.md | 🔜 | Eigenständige Service-Entrypoints, EventSub-Bridge |
| `bot/migrations/` | migrations.md | 🔜 | Schema-/Daten-Migrationen |
| `bot/compat/` | compat.md | 🔜 | Kompatibilitäts-Shims |

### Frontends (React/TypeScript)

| Bereich | Doku | Status | Inhalt |
|---------|------|:------:|--------|
| `bot/dashboard_v2/` (+ `dashboard_preview/`) | frontend-streamer-dashboard.md | 🔜 | Streamer-Dashboard-SPA (Analytics-Views) |
| `bot/admin_dashboard/` | frontend-admin-dashboard.md | 🔜 | Admin-Frontend |
| `website/` | frontend-website.md | 🔜 | Öffentliche Landing-/Onboarding-Site |
