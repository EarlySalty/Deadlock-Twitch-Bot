import type { ReactNode } from 'react';
import { Sparkles } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

export function DashboardHeader({ title, icon, description, isLoading, children }: {
  title: string;
  icon: ReactNode;
  description: ReactNode;
  isLoading?: boolean;
  children?: ReactNode;
}) {
  const t = useT();
  return (
    <header className="panel-card rounded-2xl p-4 md:p-6 mb-8 min-h-[212px]">
      <div className="flex flex-col xl:flex-row xl:items-center justify-between gap-5">
        <div className="flex items-start gap-4">
          <div className="p-3 rounded-2xl bg-gradient-to-br from-primary/30 to-accent/25 border border-primary/25 shadow-lg shadow-primary/10">
            {icon}
          </div>
          <div>
            <div className="inline-flex items-center gap-2 rounded-full border border-border bg-black/20 px-3 py-1 text-[11px] uppercase tracking-[0.16em] text-text-secondary mb-2">
              <Sparkles className="w-3 h-3 text-accent" />
              {t('Partner Dashboard')}
            </div>
            <h1 className="display-font text-2xl md:text-3xl font-bold text-white flex items-center gap-2">
              {title}
              {isLoading && <span className="w-2 h-2 rounded-full bg-primary animate-pulse" />}
            </h1>
            <p className="text-text-secondary text-sm md:text-base mt-1">{description}</p>
          </div>
        </div>
        {children}
      </div>
    </header>
  );
}
