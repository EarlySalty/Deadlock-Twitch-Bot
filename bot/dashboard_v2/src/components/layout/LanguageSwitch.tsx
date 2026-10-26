import { useLanguage } from '@/context/LanguageContext';
import { LANGUAGES, LANGUAGE_LABELS, type Language } from '@/i18n/dictionary';

export function LanguageSwitch({ className = '' }: { className?: string }) {
  const { language, setLanguage, t } = useLanguage();
  return (
    <div
      className={`flex shrink-0 items-center rounded-xl border border-border bg-background/70 p-1 ${className}`}
      role="group"
      aria-label={t('Sprache')}
      data-language-switch
    >
      {LANGUAGES.map((option: Language) => (
        <button
          key={option}
          type="button"
          lang={option}
          aria-pressed={language === option}
          title={LANGUAGE_LABELS[option]}
          onClick={() => setLanguage(option)}
          className={`min-h-8 rounded-lg px-2.5 py-1 text-xs font-semibold transition-colors ${
            language === option
              ? 'bg-gradient-to-r from-primary to-accent text-[#0D0806]'
              : 'text-text-secondary hover:text-white'
          }`}
        >
          {option.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
