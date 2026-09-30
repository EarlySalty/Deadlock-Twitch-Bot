import { useEffect, useRef, useState } from 'react';
import { useBlocker } from 'react-router';

type EditorOptions = {
  single: boolean;
  endpoint: (id: string) => string;
  request: (url: string, options?: RequestInit) => Promise<unknown>;
  onDirtyChange: (dirty: boolean) => void;
};

declare global {
  interface Window {
    DdcBotConfigEditor?: { mount: (root: HTMLElement, options: EditorOptions) => () => void };
  }
}

let assetPromise: Promise<void> | undefined;
function loadAssets(): Promise<void> {
  if (window.DdcBotConfigEditor) return Promise.resolve();
  if (assetPromise) return assetPromise;
  assetPromise = new Promise<void>((resolve, reject) => {
    if (!document.getElementById('bot-toml-editor-style')) {
      const style = document.createElement('link');
      style.id = 'bot-toml-editor-style';
      style.rel = 'stylesheet';
      style.href = '/twitch/api/admin/bot-config-editor/css';
      document.head.append(style);
    }
    const script = document.createElement('script');
    script.src = '/twitch/api/admin/bot-config-editor/js';
    script.onload = () => {
      if (window.DdcBotConfigEditor) resolve();
      else { script.remove(); assetPromise = undefined; reject(new Error('Die Konfigurationsoberfläche ist noch nicht verfügbar.')); }
    };
    script.onerror = () => {
      script.remove(); assetPromise = undefined;
      reject(new Error('Die Konfigurationsoberfläche konnte nicht geladen werden. Bitte die Seite neu laden.'));
    };
    document.head.append(script);
  });
  return assetPromise;
}

export default function BotTomlConfig() {
  const root = useRef<HTMLDivElement>(null);
  const [error, setError] = useState('');
  const [dirty, setDirty] = useState(false);
  const blocker = useBlocker(dirty);

  useEffect(() => {
    if (blocker.state !== 'blocked') return;
    if (window.confirm('Den ungespeicherten TOML-Entwurf verwerfen und diese Seite verlassen?')) blocker.proceed();
    else blocker.reset();
  }, [blocker]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    let csrf = '';
    const controller = new AbortController();
    const request = async (url: string, options: RequestInit = {}): Promise<unknown> => {
      const headers = new Headers(options.headers);
      headers.set('Accept', 'application/json');
      if (options.method && options.method.toUpperCase() !== 'GET') {
        if (!csrf) throw new Error('Die Sitzung muss zuerst neu geladen werden. Dein Entwurf bleibt erhalten.');
        headers.set('Content-Type', 'application/json');
        headers.set('X-CSRF-Token', csrf);
      }
      const response = await fetch(url, {
        ...options, headers, credentials: 'same-origin', cache: 'no-store',
        redirect: 'error', signal: controller.signal,
      });
      let data: Record<string, unknown>;
      try { data = await response.json() as Record<string, unknown>; }
      catch { throw new Error('Der Konfigurationsdienst hat keine lesbare Antwort geliefert. Bitte erneut anmelden oder die Seite neu laden.'); }
      if (!response.ok) {
        const message = typeof data.message === 'string' ? data.message : typeof data.error === 'string' ? data.error : 'Die Anfrage konnte nicht abgeschlossen werden.';
        throw new Error(message);
      }
      if (typeof data.csrf_token === 'string' && data.csrf_token) csrf = data.csrf_token;
      return data;
    };
    void loadAssets().then(() => {
      if (disposed || !root.current || !window.DdcBotConfigEditor) return;
      cleanup = window.DdcBotConfigEditor.mount(root.current, {
        single: true,
        endpoint: () => '/twitch/api/admin/bot-config',
        request,
        onDirtyChange: value => { if (!disposed) setDirty(value); },
      });
    }).catch((problem: unknown) => {
      if (!disposed) setError(problem instanceof Error ? problem.message : 'Die Oberfläche konnte nicht geladen werden.');
    });
    return () => { disposed = true; controller.abort(); cleanup?.(); };
  }, []);

  return (
    <section className="space-y-5">
      <div>
        <p className="text-xs font-semibold uppercase tracking-widest text-text-secondary">Betriebseinstellungen</p>
        <h1 className="display-font mt-2 text-3xl font-semibold text-white">Twitch-Bot · TOML</h1>
        <p className="mt-3 max-w-3xl text-sm leading-6 text-text-secondary">Die globale Betriebskonfiguration direkt verwalten. Partnerbezogene Einstellungen bleiben in ihren bisherigen Bereichen.</p>
      </div>
      {error ? <p role="alert" className="rounded-xl border border-red-400/40 bg-red-950/30 p-4 text-red-100">{error}</p> : null}
      <div ref={root} aria-label="Twitch-TOML-Editor" />
    </section>
  );
}
