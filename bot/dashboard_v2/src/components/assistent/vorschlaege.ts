import { translate, type Language } from '../../i18n/dictionary';

const VORSCHLAEGE: Record<string, string[]> = {
  home: [
    'Wie liefen meine letzten Streams?',
    'Was macht der Bot für meinen Kanal?',
    'Bin ich als Partner freigeschaltet?',
  ],
  verwaltung: [
    'Ist mein Spam-Schutz an?',
    'Wie schalte ich die Scam-Warnung ein?',
    'Welche Schutzfunktionen habe ich aktiviert?',
  ],
  uplink: [
    'Wie richte ich OBS für den Uplink ein?',
    'Ist meine Uplink-Verbindung aktiv?',
    'Welche Plattformen sind mit meinem Kanal verbunden?',
  ],
  'social-media': [
    'Wie plane ich meine Clips für Social Media?',
    'Welche Plattformen kann ich anbinden?',
    'Wie oft werden meine Clips gepostet?',
  ],
  analyse: [
    'Wie viele Zuschauer hatte ich im Schnitt?',
    'Wie haben sich meine Follower entwickelt?',
    'Wann laufen meine Streams am besten?',
  ],
  standard: [
    'Was kann der Bot für mich tun?',
    'Wie werde ich Partner?',
    'Wo bekomme ich Hilfe, wenn ich nicht weiterkomme?',
  ],
};

function schluesselFuer(page: string): keyof typeof VORSCHLAEGE {
  const basis = page.split('/')[0];
  if (basis === 'analyse') return 'analyse';
  if (basis in VORSCHLAEGE) return basis as keyof typeof VORSCHLAEGE;
  return 'standard';
}

export function vorschlaegeFuer(page: string, language: Language): string[] {
  return VORSCHLAEGE[schluesselFuer(page)].map((text) => translate(language, text));
}
