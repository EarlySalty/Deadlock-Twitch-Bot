export type RecordingDecision = 'granted' | 'declined';

export interface RecordingConsent {
  eligible: boolean;
  decision: RecordingDecision | null;
}

export function isSocialMediaPath(pathname: string): boolean {
  return pathname === '/twitch/social-media' || pathname.startsWith('/twitch/social-media/');
}

export function shouldRecord(consent: RecordingConsent | undefined, active: boolean, pathname: string): boolean {
  return active && Boolean(consent?.eligible) && consent?.decision === 'granted' && isSocialMediaPath(pathname);
}

export function shouldAskForConsent(consent: RecordingConsent | undefined, active: boolean): boolean {
  return active && Boolean(consent?.eligible) && consent?.decision === null;
}

export const RECORDING_TEXT = {
  title: 'Hilf uns, den Social-Media-Manager einfacher zu machen',
  body: 'Wir würden gern sehen, wie du dich durch diese Seite klickst und scrollst. Aufgezeichnet wird nur diese Seite, was du eintippst, bleibt verborgen. Nach 30 Tagen löschen wir die Aufnahmen. Du kannst das unten auf der Seite jederzeit wieder ausschalten.',
  allow: 'Aufzeichnung erlauben',
  decline: 'Nein, danke',
  statusOn: 'Du hilfst gerade mit: Wir zeichnen auf, wie du diese Seite nutzt.',
  statusOff: 'Aufzeichnung für Verbesserungen ist aus.',
  turnOff: 'Ausschalten und Aufnahmen löschen',
  turnOn: 'Einschalten',
  saveFailed: 'Das hat nicht geklappt. Versuch es gleich noch einmal.',
} as const;
