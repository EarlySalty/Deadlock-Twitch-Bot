import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description: string;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon: Icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div className={['empty-state flex flex-col items-center justify-center gap-4 text-center', className ?? ''].join(' ').trim()}>
      <div className="rounded-2xl border border-white/10 bg-white/[0.04] p-3 text-white/85">
        <Icon className="h-5 w-5" />
      </div>
      <div className="max-w-md space-y-2">
        <h3 className="text-base font-semibold text-white">{title}</h3>
        <p className="text-sm leading-6 text-text-secondary">{description}</p>
      </div>
      {action ? <div className="pt-1">{action}</div> : null}
    </div>
  );
}
