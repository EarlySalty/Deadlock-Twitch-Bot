import { Schedule } from '@/pages/Schedule';
import type { TimeRange } from '@/types/analytics';

interface PlanungProps {
  streamer: string | null;
  days: TimeRange;
  initialSub?: string;
}

export function Planung({ streamer, days }: PlanungProps) {
  return <Schedule streamer={streamer ?? ''} days={days} />;
}
