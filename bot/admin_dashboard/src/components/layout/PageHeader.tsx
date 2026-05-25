import type { ReactNode } from 'react';

interface PageHeaderProps {
  title: string;
  description?: string;
  primaryAction?: ReactNode;
  secondaryChips?: ReactNode;
}

export function PageHeader({ title, description, primaryAction, secondaryChips }: PageHeaderProps) {
  return (
    <header className="panel-card rounded-[1.8rem] p-6 md:p-7">
      <div className="flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between">
        <div className="max-w-3xl">
          <h1 className="text-3xl font-semibold text-white md:text-4xl">{title}</h1>
          {description ? <p className="mt-3 text-sm leading-6 text-text-secondary">{description}</p> : null}
        </div>
        {primaryAction ? <div className="shrink-0">{primaryAction}</div> : null}
      </div>

      {secondaryChips ? <div className="mt-5 flex flex-wrap gap-2 border-t border-white/8 pt-4">{secondaryChips}</div> : null}
    </header>
  );
}
