import { useState } from 'react';

export interface UplinkDisclosureSpeicher {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const UPLINK_DISCLOSURE_PREFIX = 'ddl:uplink:disclosure:';

export function uplinkDisclosureKey(bereich: string) {
  return `${UPLINK_DISCLOSURE_PREFIX}${bereich}`;
}

export function leseUplinkDisclosure(
  speicher: UplinkDisclosureSpeicher,
  bereich: string,
  startwert: boolean,
) {
  try {
    const wert = speicher.getItem(uplinkDisclosureKey(bereich));
    if (wert === '1') return true;
    if (wert === '0') return false;
  } catch (fehler) {
    console.warn('[Uplink] Offen-Zustand konnte nicht gelesen werden.', fehler);
  }
  return startwert;
}

export function schreibeUplinkDisclosure(
  speicher: UplinkDisclosureSpeicher,
  bereich: string,
  offen: boolean,
) {
  try {
    speicher.setItem(uplinkDisclosureKey(bereich), offen ? '1' : '0');
  } catch (fehler) {
    console.warn('[Uplink] Offen-Zustand konnte nicht gespeichert werden.', fehler);
  }
}

function browserSpeicher(): UplinkDisclosureSpeicher | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage;
  } catch (fehler) {
    console.warn('[Uplink] Browser-Speicher ist nicht verfügbar.', fehler);
    return null;
  }
}

export function useUplinkDisclosure(bereich: string, startwert: boolean) {
  const [offen, setOffen] = useState(() => {
    const speicher = browserSpeicher();
    return speicher ? leseUplinkDisclosure(speicher, bereich, startwert) : startwert;
  });

  function setOffenUndSpeichern(neuerWert: boolean) {
    setOffen(neuerWert);
    const speicher = browserSpeicher();
    if (speicher) schreibeUplinkDisclosure(speicher, bereich, neuerWert);
  }

  return [offen, setOffenUndSpeichern] as const;
}
