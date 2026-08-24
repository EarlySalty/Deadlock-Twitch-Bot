import assert from 'node:assert/strict';
import test from 'node:test';

import {
  leseUplinkDisclosure,
  schreibeUplinkDisclosure,
  uplinkDisclosureKey,
} from '../src/uplinkDisclosure';

class TestSpeicher {
  readonly werte = new Map<string, string>();

  getItem(key: string) {
    return this.werte.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.werte.set(key, value);
  }
}

test('Offen-Zustände überleben einen erneuten Lesevorgang', () => {
  const speicher = new TestSpeicher();

  assert.equal(leseUplinkDisclosure(speicher, 'plattform-twitch', false), false);
  schreibeUplinkDisclosure(speicher, 'plattform-twitch', true);
  assert.equal(leseUplinkDisclosure(speicher, 'plattform-twitch', false), true);
  schreibeUplinkDisclosure(speicher, 'plattform-twitch', false);
  assert.equal(leseUplinkDisclosure(speicher, 'plattform-twitch', true), false);
  assert.equal(speicher.werte.get(uplinkDisclosureKey('plattform-twitch')), '0');
});

test('unbrauchbarer oder gesperrter Browser-Speicher fällt sicher auf den Startwert zurück', () => {
  const warnung = console.warn;
  console.warn = () => undefined;
  const kaputt = {
    getItem: () => 'anderer-wert',
    setItem: () => {
      throw new Error('gesperrt');
    },
  };
  const gesperrt = {
    getItem: () => {
      throw new Error('gesperrt');
    },
    setItem: () => undefined,
  };

  try {
    assert.equal(leseUplinkDisclosure(kaputt, 'hilfe', true), true);
    assert.equal(leseUplinkDisclosure(gesperrt, 'hilfe', false), false);
    assert.doesNotThrow(() => schreibeUplinkDisclosure(kaputt, 'hilfe', true));
  } finally {
    console.warn = warnung;
  }
});

test('der Speichername enthält nur die feste Bereichs-ID', () => {
  assert.equal(uplinkDisclosureKey('obs-2'), 'ddl:uplink:disclosure:obs-2');
});
