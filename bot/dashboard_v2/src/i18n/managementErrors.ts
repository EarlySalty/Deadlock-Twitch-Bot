import type { TranslateParams } from './dictionary';

type Translator = (text: string, params?: TranslateParams) => string;

export function translateManagementError(message: string, t: Translator): string {
  const serverError = /^Server-Fehler \(HTTP (\d+)\)$/.exec(message);
  if (serverError) return t('Server-Fehler (HTTP {status})', { status: serverError[1] });
  const conflict = /^(\S+) wird in deinem Kanal bereits für (\S+) verwendet\.$/.exec(message);
  if (conflict) return t('{name} wird in deinem Kanal bereits für {command} verwendet.', { name: conflict[1], command: conflict[2] });
  return t(message);
}
