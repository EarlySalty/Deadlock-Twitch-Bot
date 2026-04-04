# API-Dokumentation

Alle HTTP-Routes des Systems. Zugriffslevel: **A** = Admin only, **S** = Streamer (eingeloggt), **P** = Public.

## Seiten (HTML)

### Auth
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/auth/login` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/callback` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/logout` | S | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/login` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/callback` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/logout` | S | auth/auth_mixin.py |

### Dashboard / Analytics UI
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/` | P | routes_mixin.py |
| GET | `/twitch/dashboard` | S | analytics/api_overview.py |
| GET | `/twitch/dashboard-v2` | S | analytics/api_overview.py |
| GET | `/twitch/dashboard-v2/{path:.*}` | S | analytics/api_overview.py |
| GET | `/twitch/verwaltung` | S | analytics/api_overview.py |
| GET | `/twitch/stats` | A | routes_mixin.py |
| GET | `/twitch/partners` | A | routes_mixin.py |
| GET | `/twitch/market` | A | routes_mixin.py |
| GET | `/twitch/demo` | P | analytics/api_overview.py |
| GET | `/twitch/demo/dashboard-v2/{path:.*}` | P | analytics/api_overview.py |
| GET | `/twitch/dashboards` | P | routes_mixin.py (redirect) |
| GET | `/twitch/dashboads` | P | routes_mixin.py (redirect) |

### Admin-Panel
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/admin` | A | routes_mixin.py |
| GET | `/twitch/live` | A | routes_mixin.py |
| GET | `/twitch/admin/announcements` | A | routes_mixin.py |
| POST | `/twitch/admin/announcements` | A | routes_mixin.py |
| GET | `/twitch/admin/roadmap` | A | routes_mixin.py |
| POST | `/twitch/admin/chat_action` | A | routes_mixin.py |
| POST | `/twitch/admin/manual-plan` | A | routes_mixin.py |
| POST | `/twitch/admin/manual-plan/clear` | A | routes_mixin.py |

### Streamer-Verwaltung (Admin)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| POST | `/twitch/add_any` | A | routes_mixin.py |
| POST | `/twitch/add_url` | A | routes_mixin.py |
| POST | `/twitch/add_login/{login}` | A | routes_mixin.py |
| POST | `/twitch/add_streamer` | A | routes_mixin.py |
| POST | `/twitch/remove` | A | routes_mixin.py |
| POST | `/twitch/verify` | A | routes_mixin.py |
| POST | `/twitch/archive` | A | routes_mixin.py |
| POST | `/twitch/discord_flag` | A | routes_mixin.py |
| POST | `/twitch/discord_link` | A | routes_mixin.py |
| POST | `/twitch/reload` | A | routes_mixin.py |

### Abo / Billing (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/abbo` | S | routes_mixin.py |
| GET | `/twitch/abo` | S | routes_mixin.py (redirect) |
| GET | `/twitch/abos` | S | routes_mixin.py (redirect) |
| GET | `/twitch/abbo/bezahlen` | S | routes_mixin.py |
| POST | `/twitch/abbo/rechnungsdaten` | S | routes_mixin.py |
| GET | `/twitch/abbo/kuendigen` | S | routes_mixin.py |
| POST | `/twitch/abbo/kuendigen` | S | routes_mixin.py |
| GET | `/twitch/abbo/rechnungen` | S | routes_mixin.py |
| GET | `/twitch/abbo/rechnung` | S | routes_mixin.py |
| GET | `/twitch/abbo/stripe-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/promo-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/lurker-tax-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/promo-message` | S | routes_mixin.py |

### Legal (Public)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/legal/access` | P | admin/legal_mixin.py |
| POST | `/twitch/legal/verify` | P | admin/legal_mixin.py |
| GET | `/twitch/impressum` | P | routes_mixin.py |
| GET | `/twitch/datenschutz` | P | routes_mixin.py |
| GET | `/twitch/agb` | P | routes_mixin.py |

Hinweis: Das Human-Gate schuetzt aktuell `/twitch/impressum` und `/twitch/datenschutz`. `/twitch/agb` bleibt oeffentlich ohne Gate. Details zu Secrets, Caddy-Allowlist, CSP und Troubleshooting stehen in [`docs/LEGAL_ACCESS_GATE.md`](LEGAL_ACCESS_GATE.md).

### Raid-Dashboard (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/raid/auth` | S | routes_mixin.py |
| GET | `/twitch/raid/go` | S | routes_mixin.py |
| GET | `/twitch/raid/callback` | P | routes_mixin.py |
| GET | `/twitch/raid/requirements` | S | routes_mixin.py |
| GET | `/twitch/raid/history` | S | routes_mixin.py |
| GET | `/twitch/raid/analytics` | S | routes_mixin.py |

### Live-Announcement (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/live-announcement` | S | routes_mixin.py |

### Affiliate (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/affiliate` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/links` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/stats` | S | affiliate/affiliate_mixin.py |
| POST | `/twitch/affiliate/links` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/click/{id}` | P | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/track` | P | affiliate/affiliate_mixin.py |
| POST | `/twitch/affiliate/settings` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/dashboard` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/export` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/api/summary` | S | affiliate/affiliate_mixin.py |

---

## Analytics API (JSON)

Alle Endpunkte unter `/twitch/api/v2/` erfordern Streamer-Authentifizierung.
Registriert in `bot/analytics/api_overview.py`.

### Uebersicht & Stats
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/overview` | api_overview.py |
| GET | `/twitch/api/v2/monthly-stats` | api_overview.py |
| GET | `/twitch/api/v2/weekly-stats` | api_overview.py |
| GET | `/twitch/api/v2/hourly-heatmap` | api_overview.py |
| GET | `/twitch/api/v2/calendar-heatmap` | api_overview.py |
| GET | `/twitch/api/v2/category-comparison` | api_overview.py |
| GET | `/twitch/api/v2/category-leaderboard` | api_overview.py |
| GET | `/twitch/api/v2/category-timings` | api_overview.py |
| GET | `/twitch/api/v2/category-activity-series` | api_overview.py |
| GET | `/twitch/api/v2/rankings` | api_overview.py |
| GET | `/twitch/api/v2/streamers` | api_overview.py |
| GET | `/twitch/api/v2/session/{id}` | api_overview.py |

### Audience & Viewer
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/viewer-overlap` | api_overview.py |
| GET | `/twitch/api/v2/viewer-timeline` | api_overview.py |
| GET | `/twitch/api/v2/viewer-profiles` | api_overview.py |
| GET | `/twitch/api/v2/viewer-directory` | api_overview.py |
| GET | `/twitch/api/v2/viewer-detail` | api_overview.py |
| GET | `/twitch/api/v2/viewer-segments` | api_overview.py |
| GET | `/twitch/api/v2/audience-insights` | api_overview.py |
| GET | `/twitch/api/v2/audience-demographics` | api_overview.py |
| GET | `/twitch/api/v2/audience-sharing` | api_overview.py |
| GET | `/twitch/api/v2/follower-funnel` | api_overview.py |
| GET | `/twitch/api/v2/lurker-analysis` | api_overview.py |
| GET | `/twitch/api/v2/{streamer}/viewer-timeline` | api_viewer_timeline.py |
| GET | `/twitch/api/v2/{streamer}/viewer-timeline/profile` | api_viewer_timeline.py |

### Chat-Analyse
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/chat-analytics` | api_overview.py |
| GET | `/twitch/api/v2/chat-hype-timeline` | api_overview.py |
| GET | `/twitch/api/v2/chat-content-analysis` | api_overview.py |
| GET | `/twitch/api/v2/chat-social-graph` | api_overview.py |

### Performance & Coaching
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/tag-analysis` | api_overview.py |
| GET | `/twitch/api/v2/tag-analysis-extended` | api_overview.py |
| GET | `/twitch/api/v2/title-performance` | api_overview.py |
| GET | `/twitch/api/v2/watch-time-distribution` | api_overview.py |
| GET | `/twitch/api/v2/coaching` | api_overview.py |
| GET | `/twitch/api/v2/monetization` | api_overview.py |
| GET | `/twitch/api/v2/retention-curve` | api_overview.py |
| GET | `/twitch/api/v2/loyalty-curve` | api_overview.py |

### Raids
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/raid-analytics` | api_overview.py |
| GET | `/twitch/api/v2/raid-retention` | api_overview.py |

### KI
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/ai/analysis` | api_overview.py |
| GET | `/twitch/api/v2/ai/history` | api_overview.py |

### Roadmap
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/roadmap` | api_overview.py |
| POST | `/twitch/api/v2/roadmap` | api_overview.py |
| PATCH | `/twitch/api/v2/roadmap/{id}` | api_overview.py |
| DELETE | `/twitch/api/v2/roadmap/{id}` | api_overview.py |

### Experimental
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/exp/overview` | api_overview.py |
| GET | `/twitch/api/v2/exp/game-breakdown` | api_overview.py |
| GET | `/twitch/api/v2/exp/game-transitions` | api_overview.py |
| GET | `/twitch/api/v2/exp/growth-curves` | api_overview.py |

### Sonstige API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/v2/auth-status` | P | api_overview.py |
| GET | `/twitch/api/v2/internal-home` | A | api_overview.py |
| GET | `/twitch/api/market_data` | A | routes_mixin.py |

### Live-Announcement API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/live-announcement/config` | S | routes_mixin.py |
| POST | `/twitch/api/live-announcement/config` | S | routes_mixin.py |
| POST | `/twitch/api/live-announcement/test` | S | routes_mixin.py |
| GET | `/twitch/api/live-announcement/preview` | S | routes_mixin.py |

### Billing API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/billing/catalog` | S | routes_mixin.py |
| GET | `/twitch/api/billing/readiness` | A | routes_mixin.py |
| POST | `/twitch/api/billing/stripe/webhook` | P | routes_mixin.py |
| POST | `/twitch/api/billing/checkout-preview` | S | routes_mixin.py |
| POST | `/twitch/api/billing/checkout-session` | S | routes_mixin.py |
| POST | `/twitch/api/billing/invoice-preview` | S | routes_mixin.py |
| POST | `/twitch/api/billing/stripe/sync-products` | A | routes_mixin.py |
