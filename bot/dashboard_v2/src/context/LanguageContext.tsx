import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import {
  DEFAULT_LANGUAGE,
  isLanguage,
  LOCALES,
  readInitialLanguage,
  storeLanguage,
  translate,
  type Language,
  type TranslateParams,
} from '../i18n/dictionary';

import { mainDomainOAuthUrl } from '../i18n/oauthLinks';

export interface LanguageContextValue {
  language: Language;
  setLanguage: (next: Language) => void;
  t: (text: string, params?: TranslateParams) => string;
  locale: string;
}

const LanguageContext = createContext<LanguageContextValue>({
  language: DEFAULT_LANGUAGE,
  setLanguage: () => {},
  t: (text, params) => translate(DEFAULT_LANGUAGE, text, params),
  locale: LOCALES[DEFAULT_LANGUAGE],
});

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>(() => readInitialLanguage());

  const setLanguage = useCallback((next: Language) => {
    setLanguageState(next);
    storeLanguage(next);
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
    const url = new URL(window.location.href);
    const queryLanguage = url.searchParams.get('lang');
    if (isLanguage(queryLanguage)) {
      storeLanguage(queryLanguage);
      url.searchParams.delete('lang');
      window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}${url.hash}`);
    }
  }, [language]);

  const value = useMemo<LanguageContextValue>(
    () => ({
      language,
      setLanguage,
      t: (text: string, params?: TranslateParams) => translate(language, text, params),
      locale: LOCALES[language],
    }),
    [language, setLanguage],
  );

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>;
}

export function useLanguage(): LanguageContextValue {
  return useContext(LanguageContext);
}

export function useOAuthLink(): (url: string) => string {
  const { language } = useLanguage();
  return useCallback((url: string) => mainDomainOAuthUrl(url, language), [language]);
}

export function useT(): LanguageContextValue['t'] {
  return useContext(LanguageContext).t;
}
