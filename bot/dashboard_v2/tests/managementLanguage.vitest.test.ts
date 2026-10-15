import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { describe, expect, it } from 'vitest';
import { translateManagementError } from '../src/i18n/managementErrors';
import { translate } from '../src/i18n/dictionary';
import { overlayPreviewTranslations, OVERLAY_PREVIEW_TEXT } from '../src/i18n/overlayPreview';

const html = readFileSync(new URL('../../../rust/crates/tb-dashboard-api/src/handlers/overlay.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];

class Element {
  children: Element[] = [];
  dataset: Record<string, string> = {};
  style = { setProperty() {} };
  classList = { add() {}, remove() {} };
  textContent = '';
  alt = '';
  title = '';
  attributes: Record<string, string> = {};
  offsetWidth = 400;
  offsetHeight = 660;
  get childElementCount() { return this.children.length; }
  appendChild(child: Element) { this.children.push(child); return child; }
  append(...children: Element[]) { this.children.push(...children); }
  replaceChildren(...children: Element[]) { this.children = children; }
  setAttribute(key: string, value: string) { this.attributes[key] = value; }
  querySelectorAll() { return []; }
  remove() {}
  replaceWith() {}
}

const data = {
  ok: true, rank_name: 'Oracle', winrate: 60, wins: 6, losses: 4,
  today_wins: 2, today_losses: 1, streak_len: 2, streak_kind: 'win', kd: 2.5,
  last_result: 'loss', last_kills: 2, last_deaths: 1, last_assists: 3,
  most_played_hero: 'Haze', recent: [{ result: 'win', hero_icon: '/hero.png' }, { result: 'loss' }],
  live: true, hero: 'Haze', minutes: 15,
};

function renderer(layout: string, language: 'en' | 'de') {
  const card = new Element();
  const parent = {};
  let receive: (event: object) => void = () => {};
  const document = {
    documentElement: { lang: 'de' },
    getElementById: () => card,
    createElement: () => new Element(),
    createDocumentFragment: () => new Element(),
  };
  const window = {
    parent, innerWidth: 1920, innerHeight: 1080,
    location: { search: `?streamer=synthetic&editor=1&lang=${language}&layout=${layout}&scene_v=2&lastmatch=1&mostplayed=1`, origin: 'http://localhost' },
    addEventListener: (type: string, callback: (event: object) => void) => { if (type === 'message') receive = callback; },
  };
  const context = {
    window, document, URLSearchParams, Intl,
    ResizeObserver: class { observe() {} },
    fetch: () => new Promise(() => {}), setInterval() {},
  };
  expect(script).toBeTruthy();
  runInNewContext(`${script}\nrender(${JSON.stringify(data)});`, context);
  const send = (origin = window.location.origin) => receive({
    source: parent, origin,
    data: { type: 'ddc-overlay-scene', sources: {}, language, translations: overlayPreviewTranslations(text => translate(language, text)) },
  });
  return { card, document, send };
}

function text(element: Element): string {
  return [element.textContent, element.alt, element.title, ...Object.values(element.attributes), ...element.children.map(text)].join(' ');
}

describe('embedded overlay language', () => {
  it.each(['box', 'bar', 'canvas'])('translates %s previews including results, fallbacks and aria labels', layout => {
    const preview = renderer(layout, 'en');
    expect(preview.card.childElementCount).toBe(0);
    preview.send('http://untrusted.example');
    expect(preview.card.childElementCount).toBe(0);
    preview.send();
    const output = text(preview.card);
    for (const word of ['RANK', 'TODAY', 'STREAK', 'LAST MATCH', 'MOST PLAYED HERO', 'Wins', 'Loss', 'Hero']) expect(output).toContain(word);
    expect(output).not.toMatch(/RANG|HEUTE|SERIE|LETZTES|LIEBLINGSHELD|Spiele|Spielername|Laufendes|Sieg|Niederlage|Held/);
    expect(output).toContain('2.50');
    expect(preview.document.documentElement.lang).toBe('en');
  });

  it.each(['box', 'bar', 'canvas'])('preserves German wording in %s previews', layout => {
    const preview = renderer(layout, 'de');
    const before = text(preview.card);
    preview.send();
    expect(text(preview.card)).toBe(before);
    expect(before).toContain('HEUTE');
    expect(before).toContain('2 Siege');
    expect(before).toContain('Niederlage');
    expect(before).toContain('2,50');
  });

  it('has translations for every preview key while retaining the original German', () => {
    for (const key of OVERLAY_PREVIEW_TEXT) {
      expect(translate('en', key), key).not.toBe(key);
      expect(translate('de', key), key).toBe(key);
    }
  });
});

describe('management API errors', () => {
  it.each([
    ['Server-Fehler (HTTP 503)', 'Server error (HTTP 503)'],
    ['!test wird in deinem Kanal bereits für !raid verwendet.', '!test is already used for !raid in your channel.'],
    ['steamId muss string oder null sein.', 'The Steam ID must be text or empty.'],
  ])('translates %s without changing German wording', (message, english) => {
    expect(translateManagementError(message, (key, params) => translate('en', key, params))).toBe(english);
    expect(translateManagementError(message, (key, params) => translate('de', key, params))).toBe(message);
  });
});
