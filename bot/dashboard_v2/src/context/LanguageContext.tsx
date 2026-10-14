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
  LOCALES,
  readInitialLanguage,
  setActiveLanguage,
  storeLanguage,
  translate,
  type Language,
  type TranslateParams,
} from '../i18n/dictionary';

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

export function LanguageProvider({
  children,
  onLanguageChange,
}: {
  children: ReactNode;
  onLanguageChange?: (next: Language) => void;
}) {
  const [language, setLanguageState] = useState<Language>(() => readInitialLanguage());
  setActiveLanguage(language);

  const setLanguage = useCallback((next: Language) => {
    setActiveLanguage(next);
    setLanguageState(next);
    storeLanguage(next);
    onLanguageChange?.(next);
  }, [onLanguageChange]);

  useEffect(() => {
    document.documentElement.lang = language;
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

export function useT(): LanguageContextValue['t'] {
  return useContext(LanguageContext).t;
}
