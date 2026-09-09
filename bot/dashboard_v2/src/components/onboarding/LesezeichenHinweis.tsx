import { useState } from 'react';
import { Bookmark } from 'lucide-react';
import { BOOKMARK_BROWSERS, erkenneBrowser, lesezeichenAnleitung, type BookmarkBrowser } from '@/utils/browserErkennung';
import { canonicalBookmarkLocation } from './steps';

/** Die Anleitung ist Teil der gemeinsamen Einrichtung, kein eigener Dialog/Browsermarker. */
export function LesezeichenHinweis() {
  const [browser, setBrowser] = useState<BookmarkBrowser>(() => erkenneBrowser(navigator.userAgent, navigator.platform, navigator.maxTouchPoints));
  const canonical = canonicalBookmarkLocation(window.location);
  return <div data-tour-id="onboarding-bookmark" data-tour-ready="true" className="space-y-4">
    <p className="max-w-2xl text-sm leading-relaxed text-text-secondary">Hier verwaltest du deinen Bot und änderst deine Einstellungen. Speichere dir diese Seite jetzt als <strong className="text-white">Lesezeichen</strong>, damit du sie später nicht suchen musst.</p>
    {canonical ? <>
      <div className="rounded-xl border border-primary/35 bg-black/35 p-4">
        <p className="flex items-start gap-3 text-base font-semibold leading-relaxed text-white"><Bookmark className="mt-1 h-5 w-5 shrink-0 text-primary" aria-hidden />{lesezeichenAnleitung(browser)}</p>
        {(browser === 'windows' || browser === 'mac') && <p className="mt-3 pl-8" aria-hidden>
          <kbd className="rounded-md border border-border bg-card px-3 py-1.5 font-semibold text-white">{browser === 'mac' ? '⌘' : 'Strg'}</kbd><span className="px-2 text-text-secondary">+</span><kbd className="rounded-md border border-border bg-card px-3 py-1.5 font-semibold text-white">D</kbd>
        </p>}
      </div>
      <details className="text-sm text-text-secondary"><summary className="w-fit cursor-pointer rounded focus-visible:outline-2 focus-visible:outline-primary">Andere Anleitung auswählen</summary>
        <label className="mt-3 block">Dein Browser<select value={browser} onChange={event => setBrowser(event.target.value as BookmarkBrowser)} className="mt-2 block w-full max-w-sm rounded-lg border border-border bg-bg p-2.5 text-white">
          {BOOKMARK_BROWSERS.map(item => <option key={item.value} value={item.value}>{item.label}</option>)}
        </select></label>
      </details>
    </> : <a href="https://deutsche-deadlock-community.de/twitch/dashboard" className="inline-flex min-h-11 items-center rounded-lg border border-primary/50 px-4 py-2 font-semibold text-primary">Dashboard zum Speichern öffnen</a>}
    <p className="text-sm text-text-secondary">Du findest es auch auf unserer <a href="/" className="text-primary underline underline-offset-4">Website</a> unter <strong className="text-white">Partner-Dashboard</strong>.</p>
  </div>;
}
