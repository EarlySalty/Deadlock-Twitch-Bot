import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { Coaching } from '@/pages/Coaching';
import { AIAnalysis } from '@/pages/AIAnalysis';
import { StreamReports } from '@/pages/StreamReports';
import type { TimeRange } from '@/types/analytics';

interface WasTunProps {
  streamer: string | null;
  days: TimeRange;
  initialMode?: string;
}

export function WasTun({ streamer, days, initialMode }: WasTunProps) {
  const [mode, setMode] = useState(initialMode ?? 'empfehlungen');
  const tabs: SubTabDef[] = [
    {
      id: 'session',
      label: 'Pro Session',
      render: () => <StreamReports streamer={streamer} days={days} />,
    },
    {
      id: 'empfehlungen',
      label: 'Empfehlungen',
      render: () => <Coaching streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'ki',
      label: 'KI-Analyse',
      render: () => <AIAnalysis streamer={streamer} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={mode} onChange={setMode} />;
}
