import type { ReactNode } from 'react';
import { PipelinePlanControls } from '../../PipelinePlanControls';
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
  onOpenHeute?: () => void;
}

export function ScheduleQueuePanel({ twitchUserId, api = scheduleQueueApi, onChanged, reloadKey = 0, layoutSlot, aiOptionsSlot, onOpenHeute }: ScheduleQueuePanelProps) {
  return <div className="pp-stack">
    <PipelinePlanControls key={twitchUserId ?? 'self'} twitchUserId={twitchUserId} api={api} onChanged={onChanged} reloadKey={reloadKey}
      layoutSlot={layoutSlot} aiOptionsSlot={aiOptionsSlot} onOpenHeute={onOpenHeute} />
  </div>;
}
