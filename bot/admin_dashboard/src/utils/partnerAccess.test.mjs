import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { findPartnerAccessEntry, resolvePartnerGranted } from './partnerAccess.ts';

const ENTRIES = [
  { streamer_login: 'earlysalty', granted: true, granted_by: 'admin', granted_at: '2026-08-06T10:00:00Z' },
  { streamer_login: 'zweiter_streamer', granted: false, granted_by: null, granted_at: '2026-08-05T10:00:00Z' },
];

describe('resolvePartnerGranted', () => {
  it('liest den Freigabestatus des passenden Streamers', () => {
    assert.equal(resolvePartnerGranted(ENTRIES, 'earlysalty'), true);
    assert.equal(resolvePartnerGranted(ENTRIES, 'zweiter_streamer'), false);
  });

  it('matcht case-insensitive, weil das Backend über LOWER() vergleicht', () => {
    assert.equal(resolvePartnerGranted(ENTRIES, 'EarlySalty'), true);
    assert.equal(resolvePartnerGranted(ENTRIES, '  EARLYSALTY  '), true);
  });

  it('ist fail-closed: unbekannter Streamer und leere Eingaben gelten als nicht freigegeben', () => {
    assert.equal(resolvePartnerGranted(ENTRIES, 'niemand'), false);
    assert.equal(resolvePartnerGranted(ENTRIES, ''), false);
    assert.equal(resolvePartnerGranted(ENTRIES, undefined), false);
    assert.equal(resolvePartnerGranted([], 'earlysalty'), false);
    assert.equal(resolvePartnerGranted(undefined, 'earlysalty'), false);
  });
});

describe('findPartnerAccessEntry', () => {
  it('liefert den ganzen Eintrag, damit die Anzeige granted_by und granted_at zeigen kann', () => {
    const entry = findPartnerAccessEntry(ENTRIES, 'EarlySalty');
    assert.equal(entry?.granted_by, 'admin');
    assert.equal(entry?.granted_at, '2026-08-06T10:00:00Z');
  });

  it('liefert undefined für unbekannte Streamer', () => {
    assert.equal(findPartnerAccessEntry(ENTRIES, 'niemand'), undefined);
    assert.equal(findPartnerAccessEntry(undefined, 'earlysalty'), undefined);
  });
});
