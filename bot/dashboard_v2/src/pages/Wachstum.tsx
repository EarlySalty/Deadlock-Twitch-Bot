import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { useT } from '@/context/LanguageContext';
import { Growth } from '@/pages/Growth';
import { Comparison } from '@/pages/Comparison';
import { Category } from '@/pages/Category';
import { Experimental } from '@/pages/Experimental';
import type { TimeRange } from '@/types/analytics';
import type { TabId } from '@/types/billing';

interface WachstumProps {
  streamer: string | null;
  days: TimeRange;
  initialSub?: string;
  onStreamerSelect: (login: string) => void;
  onNavigate: (tab: TabId) => void;
}

export function Wachstum({ streamer, days, initialSub, onStreamerSelect, onNavigate }: WachstumProps) {
  const t = useT();
  const [sub, setSub] = useState(initialSub ?? 'trends');
  const tabs: SubTabDef[] = [
    {
      id: 'trends',
      label: t('Trends'),
      entitlement: 'analytics',
      render: () => <Growth streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'vergleich',
      label: t('Vergleich'),
      entitlement: 'analytics',
      render: () => <Comparison streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'markt',
      label: t('Markt'),
      render: () => (
        <Category
          streamer={streamer}
          days={days}
          onStreamerSelect={onStreamerSelect}
          onNavigate={onNavigate}
        />
      ),
    },
    {
      id: 'experimentell',
      label: t('Experimentell'),
      entitlement: 'analytics',
      render: () => <Experimental streamer={streamer} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={sub} onChange={setSub} />;
}
