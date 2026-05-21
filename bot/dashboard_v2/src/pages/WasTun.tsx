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

function GesamtMode({ streamer, days }: { streamer: string | null; days: TimeRange }) {
  return (
    <div className="space-y-8">
      <section className="space-y-4">
        <h2 className="text-lg font-bold text-white">Sofort-Empfehlungen</h2>
        <Coaching streamer={streamer ?? ''} days={days} />
      </section>
      <section className="space-y-4">
        <h2 className="text-lg font-bold text-white">KI-Tiefenanalyse</h2>
        <AIAnalysis streamer={streamer} days={days} />
      </section>
    </div>
  );
}

export function WasTun({ streamer, days, initialMode }: WasTunProps) {
  const [mode, setMode] = useState(initialMode ?? 'gesamt');
  const tabs: SubTabDef[] = [
    {
      id: 'session',
      label: 'Pro Session',
      render: () => <StreamReports streamer={streamer} days={days} />,
    },
    {
      id: 'gesamt',
      label: 'Gesamt',
      render: () => <GesamtMode streamer={streamer} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={mode} onChange={setMode} />;
}
