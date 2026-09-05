import { AlertCircle, Info } from 'lucide-react';

import type { RawChatStatus } from '@/types/analytics';
import { formatDateFull } from '@/utils/formatters';

export function RawChatStatusBanner({
  status,
  compact = false,
  windowStart,
}: {
  status?: RawChatStatus;
  compact?: boolean;
  windowStart?: Date;
}) {
  if (!status) {
    return null;
  }
  const coverageStart = status.coverageStart ? new Date(status.coverageStart) : null;
  const partialCoverage =
    !status.suspectedIngestionIssue &&
    coverageStart !== null &&
    !Number.isNaN(coverageStart.getTime()) &&
    windowStart !== undefined &&
    coverageStart > windowStart;
  if (!partialCoverage && !status.suspectedIngestionIssue && status.available !== false && !status.note) {
    return null;
  }
  const date = partialCoverage ? formatDateFull(status.coverageStart as string) : null;
  const StatusIcon = status.suspectedIngestionIssue ? AlertCircle : Info;

  return (
    <div
      className={`rounded-2xl border ${
        status.suspectedIngestionIssue
          ? 'border-warning/30 bg-warning/10 text-warning'
          : 'border-white/10 bg-white/[0.04] text-text-secondary'
      } ${compact ? 'mb-4 px-4 py-3 text-sm' : 'px-5 py-4 text-sm'}`}
    >
      <div className="flex items-start gap-3">
        <StatusIcon className={`${compact ? 'mt-0.5 h-4 w-4' : 'mt-0.5 h-5 w-5'} shrink-0 ${partialCoverage ? 'text-primary' : ''}`} />
        <div>
          <p className="font-medium text-white">
            {status.suspectedIngestionIssue
              ? 'Chat-Lücke erkannt'
              : partialCoverage
                ? `Chat-Daten ab ${date}`
                : 'Keine Chat-Nachrichten im Zeitraum'}
          </p>
          <p className="mt-1 leading-5">
            {partialCoverage
              ? `Chat-Nachrichten werden erst seit ${date} erfasst. Kennzahlen aus dem Chat beziehen sich auf den Zeitraum ab diesem Datum.`
              : status.suspectedIngestionIssue
                ? 'Für einige Streams in diesem Zeitraum liegen keine Chat-Nachrichten vor. Kennzahlen aus dem Chat können deshalb zu niedrig ausfallen.'
                : 'Für diesen Zeitraum liegen keine Chat-Nachrichten vor. Kennzahlen aus dem Chat sind eingeschränkt.'}
          </p>
        </div>
      </div>
    </div>
  );
}
