export type BrowserName =
  | 'brave'
  | 'chrome'
  | 'edge'
  | 'firefox'
  | 'opera'
  | 'vivaldi'
  | 'safari'
  | 'unbekannt';

export type MobilArt = 'android' | 'ios' | null;

export interface BrowserEingabe {
  userAgent: string;
  brave: boolean;
  platform: string;
  mobile: boolean;
}

export interface BrowserErkennung {
  browser: BrowserName;
  mobil: MobilArt;
  mac: boolean;
}

export type HinweisPosition = 'links' | 'rechts' | 'menue' | 'unbekannt';

export type HinweisSymbol = 'stern' | 'herz' | 'teilen' | 'menue' | null;

export type Tastenkombi = ['Strg', 'D'] | ['⌘', 'D'];

export interface LesezeichenAnleitung {
  position: HinweisPosition;
  symbol: HinweisSymbol;
  tastenkombi: Tastenkombi;
  hinweis: string;
}

function erkenneMobil(userAgent: string, platform: string): MobilArt {
  const text = `${userAgent} ${platform}`;
  if (/iPhone|iPad|iPod/i.test(text)) {
    return 'ios';
  }
  if (/Android/i.test(userAgent)) {
    return 'android';
  }
  return null;
}

function erkenneName(userAgent: string, brave: boolean): BrowserName {
  if (brave) {
    return 'brave';
  }
  if (/Edg\//.test(userAgent)) {
    return 'edge';
  }
  if (/OPR\//.test(userAgent)) {
    return 'opera';
  }
  if (/Vivaldi\//.test(userAgent)) {
    return 'vivaldi';
  }
  if (/Firefox\//.test(userAgent)) {
    return 'firefox';
  }
  if (/Chrome\//.test(userAgent)) {
    return 'chrome';
  }
  if (/Safari\//.test(userAgent)) {
    return 'safari';
  }
  return 'unbekannt';
}

export function erkenneBrowser(eingabe: BrowserEingabe): BrowserErkennung {
  const mobil = erkenneMobil(eingabe.userAgent, eingabe.platform);
  const mac =
    mobil === 'ios'
      ? false
      : /Mac/i.test(eingabe.platform) || /Macintosh/i.test(eingabe.userAgent);
  return {
    browser: erkenneName(eingabe.userAgent, eingabe.brave),
    mobil,
    mac,
  };
}

export function lesezeichenAnleitung(erkennung: BrowserErkennung): LesezeichenAnleitung {
  const tastenkombi: Tastenkombi = erkennung.mac ? ['⌘', 'D'] : ['Strg', 'D'];

  if (erkennung.mobil === 'android') {
    return {
      position: 'menue',
      symbol: 'menue',
      tastenkombi,
      hinweis: 'Öffne das Menü mit den drei Punkten oben rechts und tippe auf den Stern.',
    };
  }

  if (erkennung.mobil === 'ios') {
    return {
      position: 'menue',
      symbol: 'teilen',
      tastenkombi,
      hinweis: 'Tippe auf das Teilen-Symbol und dann auf "Zum Home-Bildschirm".',
    };
  }

  switch (erkennung.browser) {
    case 'brave':
      return {
        position: 'links',
        symbol: 'stern',
        tastenkombi,
        hinweis: 'Klicke auf den Stern links neben der Adresse.',
      };
    case 'opera':
      return {
        position: 'rechts',
        symbol: 'herz',
        tastenkombi,
        hinweis: 'Klicke auf das Herz rechts in der Adressleiste.',
      };
    case 'safari':
      return {
        position: 'rechts',
        symbol: 'teilen',
        tastenkombi,
        hinweis: 'Klicke oben rechts auf den Teilen-Knopf und dann auf "Lesezeichen hinzufügen".',
      };
    case 'chrome':
    case 'edge':
    case 'firefox':
    case 'vivaldi':
      return {
        position: 'rechts',
        symbol: 'stern',
        tastenkombi,
        hinweis: 'Klicke auf den Stern rechts in der Adressleiste.',
      };
    default:
      return {
        position: 'unbekannt',
        symbol: null,
        tastenkombi,
        hinweis: '',
      };
  }
}
