import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { useT } from '@/context/LanguageContext';
import { AIAnalysis } from '@/pages/AIAnalysis';
import { StreamReports } from '@/pages/StreamReports';
import { CoachingEmpfehlungen, CoachingFormat, CoachingCommunity } from '@/pages/coachingSubPages';
import type { TimeRange } from '@/types/analytics';

interface WasTunProps {
  streamer: string | null;
  days: TimeRange;
  initialMode?: string;
}

export function WasTun({ streamer, days, initialMode }: WasTunProps) {
  const t = useT();
  const [mode, setMode] = useState(initialMode ?? 'empfehlungen');
  const tabs: SubTabDef[] = [
    {
      id: 'session',
      label: t('Pro Session'),
      render: () => <StreamReports streamer={streamer} days={days} />,
    },
    {
      id: 'empfehlungen',
      label: t('Empfehlungen'),
      render: () => <CoachingEmpfehlungen streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'format',
      label: t('Format & Auffindbarkeit'),
      render: () => <CoachingFormat streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'community',
      label: t('Community & Konkurrenz'),
      render: () => <CoachingCommunity streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'ki',
      label: t('KI-Analyse'),
      render: () => <AIAnalysis streamer={streamer} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={mode} onChange={setMode} />;
}
