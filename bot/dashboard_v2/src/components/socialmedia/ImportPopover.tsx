import { useId, type ReactNode } from 'react';
import { SlidersHorizontal } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

export function ImportPopover({ children }: { children: ReactNode }) {
  const t = useT();
  const id = useId();
  return <div className="studio-import-filter"><button type="button" className="studio-button studio-import-filter-button" popoverTarget={id}><SlidersHorizontal size={16} aria-hidden />{t('Zeitraum & Reihenfolge')}</button><div id={id} popover="auto" className="studio-import-popover">{children}</div></div>;
}
