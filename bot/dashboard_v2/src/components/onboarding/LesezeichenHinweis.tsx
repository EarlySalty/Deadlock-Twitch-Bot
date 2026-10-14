import { Fragment, useState, type ReactNode } from 'react';
import { Bookmark } from 'lucide-react';
import { useT } from '../../context/LanguageContext';
import { bookmarkBrowsers, erkenneBrowser, lesezeichenAnleitung, type BookmarkBrowser } from '../../utils/browserErkennung';
import { canonicalBookmarkLocation } from './steps';

function withElements(text: string, elements: Record<string, ReactNode>): ReactNode[] {
  return text.split(/(\{\w+\})/).map((part, index) => {
    const name = /^\{(\w+)\}$/.exec(part)?.[1];
    return <Fragment key={index}>{name && name in elements ? elements[name] : part}</Fragment>;
  });
}

export function LesezeichenHinweis() {
  const t = useT();
  const [browser, setBrowser] = useState<BookmarkBrowser>(() => erkenneBrowser(navigator.userAgent, navigator.platform, navigator.maxTouchPoints));
  const canonical = canonicalBookmarkLocation(window.location);
  return <div data-tour-id="onboarding-bookmark" data-tour-ready="true" className="space-y-4">
    <p className="max-w-2xl text-sm leading-relaxed text-text-secondary">{withElements(t('Hier verwaltest du deinen Bot und änderst deine Einstellungen. Speichere dir diese Seite jetzt als {bookmark}, damit du sie später nicht suchen musst.'), { bookmark: <strong className="text-white">{t('Lesezeichen')}</strong> })}</p>
    {canonical ? <>
      <div className="rounded-xl border border-primary/35 bg-black/35 p-4">
        <p className="flex items-start gap-3 text-base font-semibold leading-relaxed text-white"><Bookmark className="mt-1 h-5 w-5 shrink-0 text-primary" aria-hidden />{lesezeichenAnleitung(browser)}</p>
        {(browser === 'windows' || browser === 'mac') && <p className="mt-3 pl-8" aria-hidden>
          <kbd className="rounded-md border border-border bg-card px-3 py-1.5 font-semibold text-white">{browser === 'mac' ? '⌘' : t('Strg')}</kbd><span className="px-2 text-text-secondary">+</span><kbd className="rounded-md border border-border bg-card px-3 py-1.5 font-semibold text-white">D</kbd>
        </p>}
      </div>
      <details className="text-sm text-text-secondary"><summary className="w-fit cursor-pointer rounded focus-visible:outline-2 focus-visible:outline-primary">{t('Andere Anleitung auswählen')}</summary>
        <label className="mt-3 block">{t('Dein Browser')}<select value={browser} onChange={event => setBrowser(event.target.value as BookmarkBrowser)} className="mt-2 block w-full max-w-sm rounded-lg border border-border bg-bg p-2.5 text-white">
          {bookmarkBrowsers().map(item => <option key={item.value} value={item.value}>{item.label}</option>)}
        </select></label>
      </details>
    </> : <a href="https://deutsche-deadlock-community.de/twitch/dashboard" className="inline-flex min-h-11 items-center rounded-lg border border-primary/50 px-4 py-2 font-semibold text-primary">{t('Dashboard zum Speichern öffnen')}</a>}
    <p className="text-sm text-text-secondary">{withElements(t('Du findest es auch auf unserer {website} unter {section}.'), { website: <a href="/" className="text-primary underline underline-offset-4">{t('Website')}</a>, section: <strong className="text-white">{t('Partner-Dashboard')}</strong> })}</p>
  </div>;
}
