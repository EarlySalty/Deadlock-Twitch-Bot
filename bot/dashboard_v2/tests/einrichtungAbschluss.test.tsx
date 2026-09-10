import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { EinrichtungCard } from '../src/components/onboarding/EinrichtungCard';
import { OnboardingContext, type OnboardingContextValue } from '../src/components/onboarding/onboardingState';
import type { OnboardingStatus } from '../src/api/onboarding';

(globalThis as typeof globalThis & { React: typeof React }).React = React;

const LANDING = readFileSync(join(import.meta.dirname, '..', 'src', 'pages', 'InternalHomeLanding.tsx'), 'utf8');

const status = (over: Partial<OnboardingStatus>): OnboardingStatus => ({
  current_step: 0,
  completed: false,
  active_step: 'bookmark',
  completed_step_ids: [],
  paused: true,
  discord_linked: false,
  steam_linked: false,
  discord_status: 'missing',
  steam_status: 'missing',
  ...over,
});

const contextValue = (statusValue: OnboardingStatus | undefined): OnboardingContextValue => ({
  enabled: true,
  status: statusValue,
  loading: false,
  pending: false,
  error: null,
  refresh: () => {},
  save: async () => true,
  open: async () => {},
  pause: async () => {},
  advance: async () => {},
});

const renderCard = (help = false, statusValue?: OnboardingStatus) =>
  renderToStaticMarkup(
    <OnboardingContext.Provider value={contextValue(statusValue)}>
      <EinrichtungCard help={help} />
    </OnboardingContext.Provider>,
  );

const ALLE_SCHRITTE = ['bookmark', 'discord', 'steam', 'chat', 'bot', 'overlay', 'advertising', 'feedback'] as const;

test('Abgeschlossener Rundgang schließt die Einstiegskarte auf der Startseite', () => {
  const html = renderCard(false, status({
    completed: true,
    paused: true,
    completed_step_ids: [...ALLE_SCHRITTE],
  }));
  assert.equal(html, '');
});

test('Erneut geöffneter Rundgang zeigt die Karte und die Station wieder', () => {
  const oldWindow = globalThis.window;
  globalThis.window = {
    location: {
      origin: 'https://deutsche-deadlock-community.de',
      pathname: '/twitch/dashboard',
      search: '',
      hash: '',
    },
  } as Window & typeof globalThis;
  try {
    const html = renderCard(false, status({completed: true, paused: false, completed_step_ids: [...ALLE_SCHRITTE]}));
    assert.match(html, /Hier richtest du deinen Bot ein/);
    assert.match(html, /Dein Rundgang/);
    assert.match(html, /Dashboard als Lesezeichen speichern/);
  } finally {
    globalThis.window = oldWindow;
  }
});

test('Pausierter, nicht abgeschlossener Rundgang zeigt die Karte mit Fortsetzen weiter', () => {
  const html = renderCard(false, status({completed: false, paused: true, completed_step_ids: ['bookmark']}));
  assert.match(html, /Hier richtest du deinen Bot ein/);
  assert.match(html, /Rundgang fortsetzen/);
});

test('Hilfe-Panel bietet den Neustart auch nach Abschluss weiterhin an', () => {
  const html = renderCard(true, status({completed: true, paused: true, completed_step_ids: [...ALLE_SCHRITTE]}));
  assert.match(html, /Tour erneut zeigen/);
  assert.match(html, /Offene Punkte ansehen/);
});

test('Updates-Box steht über der Kritik-und-Wünsche-Box in der rechten Spalte', () => {
  const changelogIndex = LANDING.indexOf('id="changelog"');
  const feedbackIndex = LANDING.indexOf('id="feedback"');
  assert.ok(changelogIndex >= 0, 'Changelog-Anker fehlt');
  assert.ok(feedbackIndex >= 0, 'Feedback-Anker fehlt');
  assert.ok(changelogIndex < feedbackIndex, 'Feedback-Box muss unter der Updates-Box liegen');
});
