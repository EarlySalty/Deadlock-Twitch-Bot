export type BookmarkBrowser = 'windows' | 'mac' | 'ios-safari' | 'ios-chrome' | 'android-chrome' | 'android-firefox' | 'other';
export const BOOKMARK_BROWSERS: { value: BookmarkBrowser; label: string }[] = [
  {value: 'windows', label: 'Computer: Windows oder Linux'}, {value: 'mac', label: 'Computer: Mac'},
  {value: 'ios-safari', label: 'iPhone / iPad: Safari'}, {value: 'ios-chrome', label: 'iPhone / iPad: Chrome'},
  {value: 'android-chrome', label: 'Android: Chrome'}, {value: 'android-firefox', label: 'Android: Firefox'},
  {value: 'other', label: 'Anderer Browser'},
];
export function erkenneBrowser(userAgent: string, platform: string, touchPoints = 0): BookmarkBrowser {
  if (/iPhone|iPad|iPod/.test(userAgent) || (/Mac/.test(platform) && touchPoints > 1)) {
    if (/CriOS/.test(userAgent)) return 'ios-chrome';
    return /Safari/.test(userAgent) && !/FxiOS|EdgiOS|OPiOS/.test(userAgent) ? 'ios-safari' : 'other';
  }
  if (/Android/.test(userAgent)) {
    if (/Firefox/.test(userAgent)) return 'android-firefox';
    return /Chrome/.test(userAgent) && !/SamsungBrowser|EdgA|OPR/.test(userAgent) ? 'android-chrome' : 'other';
  }
  if (/Mac/.test(platform)) return 'mac';
  return /Win|Linux|X11/.test(platform + userAgent) ? 'windows' : 'other';
}
// Herstelleranleitungen geprüft am 09.09.2026; absichtlich keine feste Symbolposition.
export function lesezeichenAnleitung(browser: BookmarkBrowser): string {
  switch (browser) {
    case 'windows': return 'Drücke Strg + D und bestätige das Lesezeichen im Browser.';
    case 'mac': return 'Drücke ⌘ + D und bestätige das Lesezeichen im Browser.';
    case 'ios-safari': return 'Öffne in Safari das Menü „Mehr“ und wähle „Lesezeichen hinzufügen“. Je nach Safari-Ansicht findest du die Aktion auch im Teilen-Menü. Bestätige mit „Sichern“.';
    case 'ios-chrome': return 'Öffne in Chrome das Teilen-Menü und wähle „Zu Lesezeichen hinzufügen“.';
    case 'android-chrome': return 'Öffne in Chrome das Menü mit den drei Punkten und tippe auf „Zu Lesezeichen hinzufügen“ (Stern).';
    case 'android-firefox': return 'Öffne das Firefox-Menü und tippe auf „Lesezeichen hinzufügen“ (Stern).';
    default: return 'Öffne das Menü deines Browsers und wähle „Lesezeichen hinzufügen“ oder „Zu Favoriten hinzufügen“.';
  }
}
