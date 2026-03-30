import { AlertCircle } from 'lucide-react';

import type { RawChatStatus } from '@/types/analytics';

export function RawChatStatusBanner({
  status,
  compact = false,
}: {
  status?: RawChatStatus;
  compact?: boolean;
}) {
  if (!status) {
    return null;
  }
  if (!status.suspectedIngestionIssue && status.available !== false && !status.note) {
    return null;
  }

  return (
    <div
      className={`rounded-2xl border ${
        status.suspectedIngestionIssue
          ? 'border-warning/30 bg-warning/10 text-warning'
          : 'border-white/10 bg-white/[0.04] text-text-secondary'
      } ${compact ? 'mb-4 px-4 py-3 text-sm' : 'px-5 py-4 text-sm'}`}
    >
      <div className="flex items-start gap-3">
        <AlertCircle className={`${compact ? 'mt-0.5 h-4 w-4' : 'mt-0.5 h-5 w-5'} shrink-0`} />
        <div>
          <p className="font-medium text-white">
            {status.suspectedIngestionIssue
              ? 'Roh-Chat-Lücke erkannt'
              : 'Keine Roh-Chat-Nachrichten im Zeitraum'}
          </p>
          <p className="mt-1 leading-5">
            {status.note || 'Message-basierte KPIs und Charts sind für diesen Zeitraum eingeschränkt.'}
          </p>
        </div>
      </div>
    </div>
  );
}
