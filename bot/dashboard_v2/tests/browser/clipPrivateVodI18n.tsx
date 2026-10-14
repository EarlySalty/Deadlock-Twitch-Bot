import { createRoot } from 'react-dom/client';
import { LanguageProvider, useLanguage } from '../../src/context/LanguageContext';
import { ClipCommandSection } from '../../src/components/verwaltung/ClipCommandSection';
import { CommandNamesProvider } from '../../src/components/verwaltung/CommandNameSection';
import type { PrivateVodJob } from '../../src/api/clipCommand';
import '../../src/index.css';

const fixture = {
  calls: [] as Array<{ path: string; method: string; body: unknown; language: string | null }>,
  failDuration: false,
  failVod: false,
  failList: false,
  empty: false,
  settings: { clip_command_enabled: true, clip_command_duration_seconds: 60, clip_command_duration_configured: false },
  enabled: false,
};
Object.assign(window, { clipFixture: fixture });
Date.now = () => Date.parse('2026-10-13T12:00:00Z');

const states: Array<[PrivateVodJob['status'], string | null, string]> = [
  ['ready', null, '2030-10-14T12:34:00Z'],
  ['queued', null, '2030-10-14T12:34:00Z'],
  ['running', null, '2030-10-14T12:34:00Z'],
  ['failed', 'unavailable', '2030-10-14T12:34:00Z'],
  ['failed', 'bounds', '2030-10-14T12:34:00Z'],
  ['failed', 'timeout', '2030-10-14T12:34:00Z'],
  ['failed', 'interrupted', '2030-10-14T12:34:00Z'],
  ['failed', 'unknown', '2030-10-14T12:34:00Z'],
  ['expired', null, '2030-10-14T12:34:00Z'],
  ['ready', null, '2020-10-14T12:34:00Z'],
];
const jobs: PrivateVodJob[] = states.map(([status, error_code, expires_at], index) => ({
  id: `00000000-0000-4000-8000-${String(index + 1).padStart(12, '0')}`,
  video_id: 'fixture-recording', start_seconds: 3300 + index * 10, end_seconds: 3480 + index * 10,
  status, error_code, expires_at, file_bytes: status === 'ready' ? 1024 * 1024 * 12.5 : null,
}));

window.fetch = async (input, init = {}) => {
  const path = String(input);
  const method = init.method ?? 'GET';
  const body = init.body ? JSON.parse(String(init.body)) : undefined;
  fixture.calls.push({ path, method, body, language: new Headers(init.headers).get('X-Dashboard-Language') });
  const response = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status });
  if (path.endsWith('/command-names')) return response({ commands: ['clip', 'vodclip'].map(command => ({ command, default_name: `!${command}`, custom_name: null, effective_name: `!${command}`, effective_aliases: [], summary: '' })) });
  if (path.endsWith('/clip-command-settings')) {
    if (method === 'POST') {
      if (fixture.failDuration) return response({ error: 'Die Clip-Dauer muss zwischen 5 und 60 Sekunden liegen, in Schritten von 0,1 Sekunden.' }, 400);
      Object.assign(fixture.settings, body);
      fixture.settings.clip_command_duration_configured = true;
    }
    return response({ ...fixture.settings, ok: true });
  }
  if (path.endsWith('/private-vod-clips')) {
    if (method === 'POST') {
      if (fixture.failVod) return response({ error: 'Die Einstellung konnte nicht gespeichert werden.' }, 503);
      fixture.enabled = body.enabled;
      return response({ enabled: fixture.enabled });
    }
    if (fixture.failList) return response({ error: 'Die VOD-Ausschnitte sind gerade nicht abrufbar.' }, 503);
    return response({ enabled: fixture.enabled, jobs: fixture.empty ? [] : jobs });
  }
  throw new Error(`Unexpected fixture request: ${path}`);
};

export function Surface() {
  const { setLanguage } = useLanguage();
  return (
    <main className="mx-auto max-w-5xl p-4 md:p-8">
      <nav aria-label="Fixture language" className="mb-4 flex gap-4 text-white">
        <button type="button" lang="de" onClick={() => setLanguage('de')}>Deutsch</button>
        <button type="button" lang="en" onClick={() => setLanguage('en')}>English</button>
      </nav>
      <CommandNamesProvider><ClipCommandSection /></CommandNamesProvider>
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<LanguageProvider><Surface /></LanguageProvider>);
