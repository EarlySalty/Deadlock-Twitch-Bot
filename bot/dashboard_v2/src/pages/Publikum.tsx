import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { Audience } from '@/pages/Audience';
import { Viewers } from '@/pages/Viewers';
import { ChatAktivitaet, ChatTiefe, ChatTreue } from '@/pages/chatSubPages';
import type { TimeRange } from '@/types/analytics';

interface PublikumProps {
  streamer: string | null;
  days: TimeRange;
  initialSub?: string;
}

export function Publikum({ streamer, days, initialSub }: PublikumProps) {
  const [sub, setSub] = useState(initialSub === 'chat' ? 'chat-treue' : (initialSub ?? 'ueberblick'));
  const tabs: SubTabDef[] = [
    {
      id: 'ueberblick',
      label: 'Überblick',
      render: () => <Audience streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'viewer',
      label: 'Einzel-Viewer',
      entitlement: 'analytics.extended',
      render: () => <Viewers streamer={streamer} days={days} />,
    },
    {
      id: 'chat-treue',
      label: 'Chat-Treue',
      render: () => <ChatTreue streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'chat-aktivitaet',
      label: 'Chat-Aktivitaet',
      render: () => <ChatAktivitaet streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'chat-tiefe',
      label: 'Chat-Tiefe',
      render: () => <ChatTiefe streamer={streamer ?? ''} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={sub} onChange={setSub} />;
}
