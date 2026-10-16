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
}

export function ScheduleQueuePanel({ twitchUserId, api = scheduleQueueApi, onChanged, reloadKey = 0 }: ScheduleQueuePanelProps) {
  const text = planText(useLanguage().language);
  return <div className="pp-stack">
    <PipelinePlanControls key={twitchUserId ?? 'self'} twitchUserId={twitchUserId} api={api} onChanged={onChanged} reloadKey={reloadKey}
      timezoneLink={<a href={globalTimezoneUrl(twitchUserId)}>{text('Zeitzone in Verwaltung ändern')}</a>} />
  </div>;
}
