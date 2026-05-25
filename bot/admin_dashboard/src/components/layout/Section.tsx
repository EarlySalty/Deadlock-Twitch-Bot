import type { ReactNode } from 'react';

interface SectionProps {
  title: string;
  hint?: string;
  action?: ReactNode;
  children: ReactNode;
}

export function Section({ title, hint, action, children }: SectionProps) {
  return (
    <section className="panel-card rounded-[1.8rem] p-6">
      <div className="flex flex-col gap-4 border-b border-white/8 pb-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="max-w-3xl">
          <h2 className="text-lg font-semibold text-white">{title}</h2>
          {hint ? <p className="mt-2 text-sm leading-6 text-text-secondary">{hint}</p> : null}
        </div>
        {action ? <div className="shrink-0">{action}</div> : null}
      </div>
      <div className="pt-5">{children}</div>
    </section>
  );
}
