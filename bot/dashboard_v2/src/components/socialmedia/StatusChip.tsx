import type { ReactNode } from 'react';

export function StatusChip({ tone, children }: { tone: 'neutral' | 'success' | 'warning' | 'danger' | 'info'; children: ReactNode }) {
  return <span className="studio-status-chip" data-tone={tone}>{children}</span>;
}
