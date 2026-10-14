import type { TranslateParams } from './dictionary';
import { PAGE_DYNAMIC_EN } from './en/pageDynamic';

type Translator = (text: string, params?: TranslateParams) => string;

const templates = Object.keys(PAGE_DYNAMIC_EN).filter(key => /\{\w+\}/.test(key)).map(key => {
  const names: string[] = [];
  const parts = key.split(/(\{\w+\})/).map(part => {
    if (/^\{\w+\}$/.test(part)) {
      names.push(part.slice(1, -1));
      return '(.+?)';
    }
    return part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  });
  return { key, names, pattern: new RegExp(`^${parts.join('')}$`, 's') };
});

export function translatePageText(t: Translator, text: string, locale?: string): string {
  const translated = t(text);
  if (translated !== text) return translated;
  for (const { key, names, pattern } of templates) {
    if (t(key) === key) continue;
    const match = pattern.exec(text);
    if (!match) continue;
    const params: TranslateParams = {};
    names.forEach((name, index) => {
      const value = match[index + 1];
      params[name] = locale && /^-?\d+(\.\d+)?$/.test(value)
        ? Number(value).toLocaleString(locale, { maximumFractionDigits: 2 })
        : t(value);
    });
    return t(key, params);
  }
  return text;
}

export function translateGoalRoute(t: Translator, text: string, locale: string): string {
  if (t('Noch {steps}') === 'Noch {steps}') return text;
  const match = /^Noch (.+)$/.exec(text);
  if (!match) return translatePageText(t, text, locale);
  const steps = match[1].split(' und ').map(step => {
    const action = /^(\d+) (.+)$/.exec(step);
    return action
      ? `${Number(action[1]).toLocaleString(locale)} ${translatePageText(t, action[2], locale)}`
      : translatePageText(t, step, locale);
  }).join(t(' und '));
  return t('Noch {steps}', { steps });
}
