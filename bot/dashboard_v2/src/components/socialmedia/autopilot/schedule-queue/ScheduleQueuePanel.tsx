import type { ReactNode } from 'react';
import { globalTimezoneUrl } from '@/utils/socialMediaChannel';
import { useLanguage } from '@/context/LanguageContext';
import { PipelinePlanControls } from '../../PipelinePlanControls';
import { planText } from '../../pipelinePlan';
import { scheduleQueueApi } from './api';
import type { ScheduleQueueApi } from './api';
import './schedule-queue.css';

export interface ScheduleQueuePanelProps {
  twitchUserId?: string;
  api?: ScheduleQueueApi;
  onChanged?: () => void;
  reloadKey?: number;
  layoutSlot?: ReactNode;
  aiOptionsSlot?: ReactNode;
  reviewStatusSlot?: ReactNode;
}

export function ScheduleQueuePanel({ twitchUserId, api = scheduleQueueApi, onChanged, reloadKey = 0, layoutSlot, aiOptionsSlot, reviewStatusSlot }: ScheduleQueuePanelProps) {
  const text = planText(useLanguage().language);
  return <div className="pp-stack">
    <PipelinePlanControls key={twitchUserId ?? 'self'} twitchUserId={twitchUserId} api={api} onChanged={onChanged} reloadKey={reloadKey}
      layoutSlot={layoutSlot} aiOptionsSlot={aiOptionsSlot} reviewStatusSlot={reviewStatusSlot}
      timezoneLink={<a href={globalTimezoneUrl(twitchUserId)}>{text('Zeitzone in Verwaltung ändern')}</a>} />
  </div>;
}
