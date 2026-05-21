import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { Schedule } from '@/pages/Schedule';
import { TitleGenerator } from '@/pages/TitleGenerator';
import type { TimeRange } from '@/types/analytics';

interface PlanungProps {
  streamer: string | null;
  days: TimeRange;
  initialSub?: string;
}

export function Planung({ streamer, days, initialSub }: PlanungProps) {
  const [sub, setSub] = useState(initialSub ?? 'zeitplan');
  const tabs: SubTabDef[] = [
    {
      id: 'zeitplan',
      label: 'Zeitplan',
      render: () => <Schedule streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'titel',
      label: 'Titel-Generator',
      render: () => <TitleGenerator streamer={streamer} />,
    },
  ];
  return <SubTabs tabs={tabs} active={sub} onChange={setSub} />;
}
