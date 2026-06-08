# website/ (Öffentliche Site) — Architektur & Funktionsreferenz

> Pfad: `website/` · Stand: 2026-06-08 · 50 src-Dateien, ~7.600 Zeilen (React/TypeScript/Vite, mehr-Entry)
>
> Teil der [Architektur-Doku](README.md). Verwandt: [dashboard.md](dashboard.md) (Affiliate-/Legal-Backend), [chat.md](chat.md) (`BotQuestionBox` → Self-Explainer), [frontend-streamer-dashboard.md](frontend-streamer-dashboard.md).

## 1. Zweck & Abgrenzung

`website/` ist die **öffentliche Marketing-/Onboarding-Site**: Landing-Page (Hero, Feature-/Raid-/Clip-Demos), Streamer-Onboarding, Bot-FAQ und das **Affiliate-Portal/Vertriebler**. Sie erklärt den Bot, führt Streamer ins Onboarding und ist der öffentliche Einstieg.

Abgrenzung: Reines Frontend (statisch/prerendert). Dynamik kommt aus wenigen API-Aufrufen (Live-Ban-Feed, Bot-Frage-Box). Das Affiliate-**Backend** liegt in [dashboard.md](dashboard.md).

## 2. Einordnung & Abhängigkeiten

| Aspekt | Detail |
|--------|--------|
| **Stack** | React + TypeScript, Vite (**Multi-Entry** + Prerender), framer-motion, tailwind. |
| **Entries** | `App.tsx` (Haupt-Landing) + separate Mount-Punkte: `affiliate-portal.tsx`, `faq.tsx`, `onboarding.tsx`, `vertriebler.tsx`; `main.tsx` enthält `prerender` (SSG). |
| **API** | wenige öffentliche Endpunkte (Ban-Feed, Self-Explainer-Frage-Box). |
| **Wissensbasis** | `data/twitchKnowledgeBase.ts` (858 Z.) — FAQ-/Onboarding-Inhalte (auch vom FAQ-Bot indiziert). |

## 3. Struktur im Überblick

| Verzeichnis | Inhalt |
|-------------|--------|
| `(root)` | `App.tsx` + Entry-Mounts (`affiliate-portal`, `faq`, `onboarding`, `vertriebler`), `main.tsx` (Prerender). |
| `pages/` | `AffiliatePortal` (1298 Z.), `AffiliateProgramPage`, `BotFaqPage`, `StreamerOnboardingPage`. |
| `components/sections/` | Landing-Abschnitte: `Hero`, `RaidDemo`, `RaidExplainer`, `RaidSystem`, `ClipManager`, `Dashboard`, `Community`, `Features`, `Stats`, `CTA`, `BanFeed`, `BotQuestionBox`, `AffiliateSection`. |
| `components/layout/` | `Navbar`, `Footer`, `PublicInfoHeader`/`PublicInfoFooter`, `AffiliateNavbar`. |
| `components/ui/` | `GlowButton`, `GradientText`, `AnimatedCounter`, `ScrollReveal`, `FeatureCard`, `BrowserMockup`, `SectionHeading`. |
| `components/onboarding/` | `StepCard`, `OnboardingProgress`, `FeatureHighlight`. `components/effects/` | `GlowOrb`. |
| `hooks/` | `useBanFeed` (Live-Ban-Feed), `useCountUp`, `useScrollSpy`. |
| `data/` | `twitchKnowledgeBase`, `externalLinks`, `features`, `stats`, `affiliateFeatures`, `sitePaths`. |

## 4. Datenfluss / Lebenszyklus

1. **Build/Prerender:** Vite baut mehrere Entries; `main.tsx::prerender` erzeugt statisches HTML (schnelle, SEO-freundliche Auslieferung).
2. **Landing:** `App.tsx` setzt die `sections/` zusammen (Hero → Features → Raid-Demos → Clip → Community → CTA), `useScrollSpy` markiert den aktiven Abschnitt, `useCountUp`/`AnimatedCounter` animieren Kennzahlen.
3. **Dynamik:** `BanFeed`/`useBanFeed` zieht den **Live-Ban-Feed** aus einem öffentlichen Endpoint (zeigt die Moderations-Bans des Bots). `BotQuestionBox` schickt Nutzerfragen an den **Self-Explainer-Endpoint** (grounded, Anti-Injection — siehe [chat.md](chat.md)) und zeigt die Antwort direkt.
4. **Onboarding/FAQ:** `StreamerOnboardingPage`/`BotFaqPage` rendern aus `data/twitchKnowledgeBase.ts` (dieselbe Quelle, die der FAQ-Bot nutzt).
5. **Affiliate:** `AffiliatePortal`/`AffiliateProgramPage` (+ `vertriebler`-Entry) bilden das öffentliche Affiliate-Onboarding ab; die eigentliche Abwicklung läuft im [Dashboard-Backend](dashboard.md).

## 5. Referenz (Bereiche & Schlüsseldateien)

### pages/
- `AffiliatePortal` (1298 Z.) — das öffentliche Affiliate-Portal (Programm-Erklärung, Signup-Einstieg).
- `AffiliateProgramPage`, `BotFaqPage`, `StreamerOnboardingPage`.

### components/sections/
Landing-Abschnitte als eigenständige Komponenten: `Hero`, `RaidDemo` (818 Z., interaktive Raid-Demo), `RaidExplainer`, `RaidSystem`, `ClipManager`, `Dashboard` (Dashboard-Vorschau), `Community`, `Features`, `Stats`, `CTA`, `BanFeed`, `BotQuestionBox`, `AffiliateSection`.

### components/layout/ + ui/ + onboarding/
- `layout/` — `Navbar`, `Footer`, `PublicInfoHeader`/`PublicInfoFooter` (rechtliche Public-Info-Seiten), `AffiliateNavbar`.
- `ui/` — wiederverwendbare Bausteine: `GlowButton`, `GradientText`, `AnimatedCounter`, `ScrollReveal`, `FeatureCard`, `BrowserMockup`, `SectionHeading`.
- `onboarding/` — `StepCard`, `OnboardingProgress`, `FeatureHighlight`. `effects/GlowOrb`.

### hooks/
- `useBanFeed` — `BanEntry`/`BanStats`/`BanFeedData`; lädt den Live-Ban-Feed. `useCountUp` — Zähl-Animation. `useScrollSpy` — aktiven Abschnitt erkennen.

### data/
- `twitchKnowledgeBase.ts` — `OnboardingStep`, `ChecklistItem`, `FaqItem`, `FaqSection`, `ONBOARDING_VISUAL_STEPS` (FAQ-/Onboarding-Inhalt).
- `externalLinks.ts` — `DISCORD_INVITE_URL`, `TWITCH_*_URL` etc. `features.ts`/`stats.ts`/`affiliateFeatures.ts`/`sitePaths.ts` — statische Inhaltsdaten.

## 6. Datenbank & externe Schnittstellen

- **API (öffentlich):** Live-Ban-Feed (`useBanFeed`), Self-Explainer-Frage-Box (`BotQuestionBox` → Self-Explainer-Endpoint).
- **Externe Links:** Discord-Invite, EarlySalty-Website, Twitch-Onboarding/FAQ/Login (`data/externalLinks.ts`).
- **Keine** eigene DB.

## 7. Stolperfallen / Besonderheiten

- **Multi-Entry + Prerender:** Es gibt mehrere Einstiegspunkte (Landing/FAQ/Onboarding/Affiliate/Vertriebler), nicht eine SPA mit Router. Wer eine Seite vermisst, prüft das passende Entry + den Prerender.
- **Wissensbasis ist geteilt:** `twitchKnowledgeBase.ts` speist Website-FAQ **und** den FAQ-Bot — Änderungen wirken an beiden Stellen.
- **BotQuestionBox antwortet direkt:** Anders als die Chat-Antworten (Shadow) antwortet die Website-Frage-Box live (grounded + Anti-Injection, siehe [chat.md](chat.md)).
- **Ban-Feed ist öffentlich:** Der Live-Ban-Feed zeigt Moderations-Bans öffentlich — bei Wortwahl/Anzeige die Call-out-Regeln beachten (Hedge statt harter Vorwurf, siehe Memory).
- **Design pragmatisch-neutral:** Funktional/neutral halten (keine identitätspolitischen Tags) — siehe Memory zur Design-Tonalität.
