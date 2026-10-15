import { useT } from '../../context/LanguageContext';
import { useCommandNames } from './CommandNameSection';
import { CommandRow } from './ui';

export function OtherCommandNames({ excluded }: { excluded: string[] }) {
  const t = useT();
  const { commands, loading, error } = useCommandNames();
  const remaining = commands.filter(row => !excluded.includes(row.command));
  if (loading) return <p className="text-sm text-text-secondary">{t("Weitere Befehle werden geladen …")}</p>;
  if (error) return <p role="alert" className="text-sm text-danger">{t(error)}</p>;
  if (!remaining.length) return null;
  return (
    <div className="grid gap-x-8 md:grid-cols-2">
      {remaining.map(row => (
        <CommandRow
          key={row.command}
          command={row.command}
          description={
            <>
              {t(row.summary).split(row.default_name).join(row.effective_name)}
              {row.command === 'discord' && <span className="mt-1 block text-primary">{t("Wenn du diesen Namen änderst, hilft der Bot bei !dldc mit „Meinst du")} {row.effective_name}{t("?“ weiter.")}</span>}
            </>
          }
        />
      ))}
    </div>
  );
}
