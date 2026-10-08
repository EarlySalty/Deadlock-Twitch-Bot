import { test } from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ArchivedVod } from '../src/api/socialMedia';
import { LanguageProvider } from '../src/context/LanguageContext';
import { translate } from '../src/i18n/dictionary';

(globalThis as { React?: typeof React }).React = React;
Object.assign(globalThis, { window: { location: { pathname: '/', hostname: 'localhost' }, localStorage: { getItem: () => null, setItem: () => {} } } });
const { VodArchiveEntry, VOD_UPLOAD_PARTS_LABEL } = await import('../src/components/socialmedia/VodArchiveTab');

const base: ArchivedVod = {
  id: 1, twitch_id: 'synthetic', channel: 'testkanal', twitch_user_id: '42',
  title: 'Synthetischer Stream', duration_sec: 3600, recorded_at: null,
  discovered_at: '2026-09-01T12:00:00Z', status: 'archived',
  display_status: 'youtube_uploaded', status_label: 'YouTube-Upload abgeschlossen',
  youtube_complete: true, drive_complete: false, confirmed_parts: 1, total_parts: 1,
  can_retry: false, can_drive: false, reason: null, drive_url: null, drive_requested: false,
  uploaded_at: '2026-09-01T13:00:00Z', last_attempt_at: null,
  parts: [{ index: 0, status: 'done', youtube_video_id: 'synthetic' }], needs_connection: false,
};

function render(changes: Partial<ArchivedVod> = {}) {
  return renderToStaticMarkup(<LanguageProvider><VodArchiveEntry vod={{ ...base, ...changes }} pending={false} onAction={() => assert.fail('Render must not perform actions')} /></LanguageProvider>);
}

test('historical confirmation renders its completion time without inventing an attempt', () => {
  const html = render();
  assert.match(html, /<time dateTime="2026-09-01T13:00:00Z">/);
  assert.match(html, /lucide-circle-check/);
  assert.ok(html.indexOf(base.title) < html.indexOf(base.status_label));
  assert.equal((html.match(/<button/g) ?? []).length, 1);
  assert.doesNotMatch(html, /studio-button/);
  assert.doesNotMatch(render({ uploaded_at: null }), /<time/);
});

test('unknown historical state keeps links unconfirmed and suppresses retry', () => {
  const html = render({ display_status: 'unknown', status_label: 'Abschluss unklar', youtube_complete: false, confirmed_parts: 0, uploaded_at: null, parts: [{ index: 0, status: 'pending', youtube_video_id: 'synthetic' }] });
  assert.match(html, /lucide-triangle-alert/);
  assert.match(html, /watch\?v=synthetic/);
  assert.match(html, /text-warning/);
  assert.equal((html.match(/<button/g) ?? []).length, 1);
  assert.doesNotMatch(html, /<time/);
});

test('Drive completion does not count pending YouTube parts as confirmed', () => {
  const html = render({ display_status: 'drive_uploaded', status_label: 'Auf Drive gesichert', youtube_complete: false, drive_complete: true, confirmed_parts: 0, total_parts: 2, drive_url: 'https://drive.google.com/drive/folders/synthetic', parts: [{ index: 0, status: 'pending', youtube_video_id: null }, { index: 1, status: 'pending', youtube_video_id: null }] });
  assert.match(html, /lucide-hard-drive/);
  assert.match(html, /drive\.google\.com/);
  assert.doesNotMatch(html, /watch\?v=/);
  assert.doesNotMatch(html, /YouTube:/);
});

test('partial, failed, running and waiting states have distinct icons and timestamps', () => {
  for (const [display_status, icon] of [
    ['partial', 'circle-dashed'], ['failed', 'circle-x'],
    ['uploading', 'loader-circle'], ['downloading', 'download'], ['waiting', 'clock'],
  ] as const) {
    const html = render({ display_status, status: 'upload_failed', youtube_complete: false, can_retry: ['partial', 'failed', 'waiting'].includes(display_status), can_drive: ['partial', 'failed', 'waiting'].includes(display_status), uploaded_at: null, last_attempt_at: '2026-10-01T13:00:00Z', total_parts: 2 });
    assert.ok(html.includes(`lucide-${icon}`), display_status);
    assert.match(html, /<time dateTime="2026-10-01T13:00:00Z">/);
    assert.equal((html.match(/<button/g) ?? []).length, ['partial', 'failed', 'waiting'].includes(display_status) ? 3 : 1);
  }
});

test('current private proof renders checked time and an independent debounced read action', () => {
  const checked = '2026-10-08T01:00:00Z';
  const proof = { state: 'confirmed', complete: true, pending: false, can_request: false, error: null, last_attempt_at: checked, last_success_at: checked, observations: [{ video_id: 'matched', part_index: null, part_total: null, state: 'processed', privacy: 'private', observed_at: checked }] };
  const html = render({ display_status: 'youtube_confirmed', status_label: 'Auf YouTube bestätigt', can_check_youtube: true, youtube_complete: false, youtube_verified_complete: true, parts: [], uploaded_at: null, youtube_check: proof });
  assert.match(html, /watch\?v=matched/);
  assert.match(html, /<time dateTime="2026-10-08T01:00:00Z">/);
  assert.match(html, /disabled=""/);
  assert.equal((html.match(/<button/g) ?? []).length, 2);
  assert.doesNotMatch(html, /dateTime="2026-09-01/);
  const error = render({ display_status: 'youtube_error', youtube_check: { ...proof, state: 'error', complete: false, error: 'connection' } });
  assert.match(error, /oauth\/start\/youtube/);
  assert.match(error, /watch\?v=matched/);
});

test('YouTube reconnect errors keep their recovery link alongside independent Drive success', () => {
  for (const error of ['connection', 'channel_changed']) {
    const html = render({ display_status: 'drive_uploaded', status_label: 'Auf Drive gesichert', drive_requested: true, drive_complete: true, drive_url: 'https://drive.google.com/drive/folders/synthetic', youtube_complete: false, youtube_check: { state: 'error', complete: false, pending: false, error, last_attempt_at: null, last_success_at: null, observations: [] } });
    assert.match(html, /oauth\/start\/youtube/);
    assert.match(html, /drive\.google\.com/);
    assert.match(html, /lucide-hard-drive/);
    assert.equal((html.match(/<button/g) ?? []).length, 1);
  }
  assert.doesNotMatch(render({ drive_requested: true, needs_connection: true, can_retry: true }), /oauth\/start\/youtube/);
  assert.doesNotMatch(render({ twitch_user_id: null, youtube_check: { state: 'error', complete: false, pending: false, error: 'connection', last_attempt_at: null, last_success_at: null, observations: [] } }), /oauth\/start\/youtube/);
});

test('current multipart proof and historical upload coverage remain explicitly separate', () => {
  const checked = '2026-10-08T01:00:00Z';
  const proof = { state: 'confirmed', complete: true, pending: false, error: null, last_attempt_at: checked, last_success_at: checked, observations: [0, 1].map((part_index) => ({ video_id: `matched-${part_index}`, part_index, part_total: 2, state: 'processed', privacy: 'private', observed_at: checked })) };
  const html = render({ display_status: 'youtube_confirmed', status_label: 'Auf YouTube bestätigt', youtube_complete: false, youtube_verified_complete: true, confirmed_parts: 0, total_parts: 2, parts: [], uploaded_at: null, youtube_check: proof });
  assert.ok(html.includes(VOD_UPLOAD_PARTS_LABEL.replace('{done}', '0').replace('{total}', '2')));
  assert.match(html, /lucide-circle-check/);
  assert.equal((html.match(/watch\?v=matched-/g) ?? []).length, 2);
  assert.doesNotMatch(html, /dateTime="2026-09-01/);
  assert.notEqual(translate('en', VOD_UPLOAD_PARTS_LABEL), VOD_UPLOAD_PARTS_LABEL);
});

test('terminal recovery exposes retry and Drive independently', () => {
  for (const [can_retry, can_drive, count] of [[false, false, 1], [true, false, 2], [false, true, 2], [true, true, 3]] as const) {
    const html = render({ can_retry, can_drive, display_status: 'failed' });
    assert.equal((html.match(/<button/g) ?? []).length, count);
  }
});

test('archive status and new UI copy have translations', () => {
  for (const text of ['YouTube-Upload abgeschlossen', 'Auf Drive gesichert', 'Teilweise auf YouTube', 'Abschluss unklar', 'Status unklar', 'YouTube-Upload abgelehnt', 'Auf YouTube öffnen', 'unbestätigt', 'Drive-Sicherung läuft.', 'Auf YouTube bestätigt', 'YouTube verarbeitet das Video', 'Bei YouTube nicht abrufbar', 'Prüfung gerade nicht möglich', 'Bei YouTube prüfen', 'Nicht gelistet', 'Öffentlich', 'Die lokale Kopie fehlt. Prüfe das vorhandene Video und die YouTube-Verbindung; ein erneuter Upload ist derzeit nicht möglich.', 'Das vorhandene YouTube-Video bleibt unverändert. Du kannst die lokale Kopie auf Drive sichern.', 'Du kannst abgelehnte Teile ausdrücklich erneut hochladen oder die lokale Kopie auf Drive sichern. Bereits erfolgreiche Teile bleiben erhalten.']) {
    assert.notEqual(translate('en', text), text);
  }
});
