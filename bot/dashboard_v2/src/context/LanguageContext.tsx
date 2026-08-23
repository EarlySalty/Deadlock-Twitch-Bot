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
  readStoredLanguage,
  storeLanguage,
  translate,
  type Language,
  type TranslateParams,
  // Relativ statt ueber den @-Alias: die Node-Tests laufen ohne Vite und
  // koennen den Alias nicht aufloesen.
} from '../i18n/dictionary';

export interface LanguageContextValue {
  language: Language;
  setLanguage: (next: Language) => void;
  /** Uebersetzt einen deutschen Text; ohne Eintrag bleibt er auf Deutsch. */
  t: (text: string, params?: TranslateParams) => string;
  /** Passendes Locale fuer toLocaleString & Co. */
  locale: string;
}

/**
 * Der Default ist bewusst kein `null`: `useLanguage()` darf auch ausserhalb
 * des Providers nichts kaputt machen. Seiten, die noch nicht umgestellt sind
 * oder in Tests einzeln gerendert werden, laufen dann auf Deutsch weiter.
 */
const LanguageContext = createContext<LanguageContextValue>({
  language: DEFAULT_LANGUAGE,
  setLanguage: () => {},
  t: (text, params) => translate(DEFAULT_LANGUAGE, text, params),
  locale: LOCALES[DEFAULT_LANGUAGE],
});

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>(() => readStoredLanguage());

  const setLanguage = useCallback((next: Language) => {
    setLanguageState(next);
    storeLanguage(next);
  }, []);

  // Screenreader und Browser-Uebersetzer richten sich nach dem lang-Attribut.
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

/** Kurzform fuer Komponenten, die nur uebersetzen wollen. */
export function useT(): LanguageContextValue['t'] {
  return useContext(LanguageContext).t;
}
