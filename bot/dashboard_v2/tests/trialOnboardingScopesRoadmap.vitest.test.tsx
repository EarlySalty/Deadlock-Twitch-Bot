import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { translate, type Language } from '../src/i18n/dictionary';
import { RoadmapPanel } from '../src/components/roadmap/RoadmapPanel';
import { FeatureTooltip } from '../src/components/onboarding/FeatureTooltip';
import { TrialBanner } from '../src/components/banners/TrialBanner';
import TrialCallout from '../src/components/pricing/TrialCallout';
import { AuthScopes } from '../src/pages/AuthScopes';
import { ScopeProvider } from '../src/hooks/useScopes';
import { ScopeSummaryBanner } from '../src/components/scopes/ScopeSummaryBanner';

const state = vi.hoisted(() => ({ language: 'en' as Language, loading: false, error: false, empty: false }));

vi.mock('../src/context/LanguageContext', async () => {
  const { translate } = await import('../src/i18n/dictionary');
  return {
    useT: () => (text: string, params?: Record<string, string | number>) => translate(state.language, text, params),
    useLanguage: () => ({ locale: state.language === 'en' ? 'en-US' : 'de-DE' }),
  };
});

vi.mock('../src/context/PlanContext', () => ({
  usePlan: () => ({ tier: 'basic', trialInfo: { isInTrial: true, trialDaysRemaining: 5 } }),
}));

vi.mock('../src/hooks/useAnalytics', () => ({
  useRoadmap: () => ({
    isLoading: state.loading,
    isError: state.error,
    data: {
      planned: state.empty ? [] : [{ id: 1, status: 'planned', title: 'Analyse Routing fix', description: 'Alte Analyseadressen leiten sauber auf /twitch/analyse um.' }],
      in_progress: [],
      done: [],
    },
  }),
}));

beforeEach(() => {
  state.language = 'en';
  state.loading = false;
  state.error = false;
  state.empty = false;
});

describe('trial, onboarding and roadmap language', () => {
  it.each(['de', 'en'] as const)('renders scope cards, changelog and summary in %s', language => {
    state.language = language;
    const html = renderToStaticMarkup(<ScopeProvider><AuthScopes /><ScopeSummaryBanner /></ScopeProvider>);
    for (const text of ['Mods verwalten', 'Sichtbar für Streamer', 'Kritisch fehlen', 'Autorisation im Blick', 'Basis-Auth entschlackt']) {
      expect(html).toContain(translate(language, text));
      if (language === 'en') expect(html).not.toContain(text);
    }
  });
  it.each(['de', 'en'] as const)('renders roadmap titles and descriptions in %s', language => {
    state.language = language;
    const html = renderToStaticMarkup(<RoadmapPanel />);
    expect(html).toContain(translate(language, 'Analyse Routing fix'));
    expect(html).toContain(translate(language, 'Alte Analyseadressen leiten sauber auf /twitch/analyse um.'));
    if (language === 'en') {
      expect(html).not.toContain('Analyse Routing fix');
      expect(html).not.toContain('Alte Analyseadressen');
      expect(html).not.toContain('Geplant');
    }
  });

  it.each([
    ['loading', 'Loading...'],
    ['error', 'Couldn&#x27;t load the roadmap.'],
    ['empty', 'No roadmap entries yet.'],
  ] as const)('localizes the roadmap %s state', (key, expected) => {
    state[key] = true;
    expect(renderToStaticMarkup(<RoadmapPanel />)).toContain(expected);
  });

  it.each(['de', 'en'] as const)('renders onboarding tooltip copy in %s', language => {
    state.language = language;
    const html = renderToStaticMarkup(<FeatureTooltip title="Dein Einstieg" description="Hier richtest du deinen Bot ein" />);
    expect(html).toContain(translate(language, 'Dein Einstieg'));
    expect(html).toContain(translate(language, 'Hier richtest du deinen Bot ein'));
  });

  it.each(['de', 'en'] as const)('renders trial copy and close label in %s', language => {
    state.language = language;
    const html = renderToStaticMarkup(<TrialBanner />);
    expect(html).toContain(translate(language, 'Noch {days} Tage kostenlos testen', { days: 5 }));
    expect(html).toContain(`aria-label="${translate(language, 'Banner schliessen')}"`);
    expect(html).toContain(translate(language, 'Jetzt upgraden'));
  });

  it.each(['de', 'en'] as const)('renders the trial callout in %s', language => {
    state.language = language;
    const html = renderToStaticMarkup(<TrialCallout />);
    expect(html).toContain(translate(language, 'Keine Kreditkarte erforderlich – risikofrei starten'));
    expect(html).toContain(translate(language, 'Einmalig pro Account · Jederzeit kündbar'));
  });
});
