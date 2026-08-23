/**
 * Fehlerzeile einer Karte, deren Abfrage gescheitert ist. Sie sagt beides:
 * dass der angezeigte Stand nicht der gespeicherte ist, und dass die Karte
 * deshalb gesperrt bleibt. Die Sperrlogik dazu steht in `kartenZustand.ts`.
 */
import { useT } from '../../context/LanguageContext';
import { fehlerText } from './labels';
import { istStandUnbekannt } from './kartenZustand';

export function LadeFehlerHinweis({ fehler }: { fehler: unknown }) {
  const t = useT();
  if (!istStandUnbekannt(fehler)) return null;
  return (
    <div
      role="alert"
      className="rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-xs text-danger space-y-1"
    >
      <div className="font-semibold">{t('Gespeicherter Stand nicht abrufbar')}</div>
      <div>{fehlerText(fehler, t)}</div>
      <div>
        {t(
          'Solange bleibt diese Karte gesperrt, damit nichts Falsches gespeichert wird. Bitte die Seite neu laden.',
        )}
      </div>
    </div>
  );
}
