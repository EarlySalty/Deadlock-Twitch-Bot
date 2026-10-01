# Contract: Social Studio Integration

## Betroffene Produktdateien (Dashboard `bot/dashboard_v2`)

Geändert:
- `src/pages/SocialMedia.tsx` — Vier-Bereiche-Studio mit Hero, Kennzahlen, Toolbar, Pagination, Dialogen.
- `src/pages/SocialMediaAdmin.tsx` — Kanalwechsel mit `key={streamer}`, Bestätigungsdialog bei ungespeicherten Entwürfen, aria-label.
- `src/components/layout/DashboardShell.tsx` — scoped Studio-Shell nur für Route `social`.
- `src/components/layout/DashboardSidebar.tsx` — D-Logo-Markenblock nur auf Route `social`.
- `src/components/socialmedia/labels.ts` — Tab-Bezeichnungen der vier Bereiche.
- `src/i18n/dictionary.ts` — englische Entsprechungen für neue Texte.
- `src/api/socialMedia.ts` — `fetchClips` nimmt optional `AbortSignal`.
- `tests/socialMediaContract.test.ts` — neue Dateien in der OBERFLAECHE-Prüfung.
- `package.json` — neues Testfile in der Suite.

Neu:
- `src/components/socialmedia/PostingPlanDraft.tsx` — Entwurf/Speichern mit Teilfehlerbehandlung.
- `src/components/socialmedia/WorkspaceDialog.tsx` — fokussierter Dialog mit Fokusfalle und Rückkehr zum Auslöser.
- `src/components/socialmedia/queuePresentation.ts` — Status-Mapping, Kennzahlen, Filter, Bestandsabfrage.
- `src/components/socialmedia/studio.css` — auf vorhandene Marken-Tokens gemappte Materialstile.
- `tests/socialStudioRedesign.test.ts` — 6 Tests zu Status, Kennzahlen, Pagination, Abbruch.

## Unverändert erhalten

- API-Schicht mit Auth-, CSRF- und Fehlerbehandlung; keine zweite fetch-Schicht.
- `LayoutEditor`, `EnrichmentPanel`, `AnalyticsTab`, VOD-Archiv, Sprache.
- Shell und Seitenleiste aller anderen Routen.
- Backend; keine Rust-Änderungen, keine Migration.
