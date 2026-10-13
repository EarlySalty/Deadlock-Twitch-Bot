import { useState } from 'react';
import { SubTabs, type SubTabDef } from '@/components/layout/SubTabs';
import { useT } from '@/context/LanguageContext';
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
  const t = useT();
  const [sub, setSub] = useState(initialSub === 'chat' ? 'chat-treue' : (initialSub ?? 'ueberblick'));
  const tabs: SubTabDef[] = [
    {
      id: 'ueberblick',
      label: t('Überblick'),
      render: () => <Audience streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'viewer',
      label: t('Einzel-Viewer'),
      entitlement: 'analytics',
      render: () => <Viewers streamer={streamer} days={days} />,
    },
    {
      id: 'chat-treue',
      label: t('Chat-Treue'),
      render: () => <ChatTreue streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'chat-aktivitaet',
      label: t('Chat-Aktivität'),
      render: () => <ChatAktivitaet streamer={streamer ?? ''} days={days} />,
    },
    {
      id: 'chat-tiefe',
      label: t('Chat-Tiefe'),
      render: () => <ChatTiefe streamer={streamer ?? ''} days={days} />,
    },
  ];
  return <SubTabs tabs={tabs} active={sub} onChange={setSub} />;
}
