import { useState } from 'react';
import { SocialMedia, type SocialMediaProps } from '@/pages/SocialMedia';
import { ImportPoolPanel } from '@/components/socialmedia/autopilot/import-pool';
import { createImportPoolFixture } from '@/components/socialmedia/autopilot/import-pool/fixtures';
import {
  ScheduleQueuePanel,
  createScheduleQueueFixture,
  type ScheduleQueueApi,
} from '@/components/socialmedia/autopilot/schedule-queue';
import { previewWorkspaceState } from './socialWorkspaceFixtures';

function PreviewAutomation({
  twitchUserId,
}: Pick<SocialMediaProps, 'twitchUserId'>) {
  const [api] = useState<ScheduleQueueApi>(() => {
    const state = previewWorkspaceState(window.location.search, 'automation');
    const fixture = createScheduleQueueFixture();
    if (state === 'loading')
      return { ...fixture.api, load: () => new Promise(() => {}) };
    if (state === 'error')
      return {
        ...fixture.api,
        load: () => Promise.reject(new TypeError('preview_offline')),
      };
    if (state === 'empty')
      return {
        ...fixture.api,
        load: async (owner, signal) => ({
          ...(await fixture.api.load(owner, signal)),
          jobs: [],
          review_count: 0,
          production_hold: 'empty_pool',
        }),
      };
    return fixture.api;
  });
  return <ScheduleQueuePanel twitchUserId={twitchUserId} api={api} />;
}

function PreviewImport({
  twitchUserId,
}: Pick<SocialMediaProps, 'twitchUserId'>) {
  const [fixture] = useState(() => {
    const state = previewWorkspaceState(window.location.search, 'library');
    const value = createImportPoolFixture({
      owner: twitchUserId,
      preferences: {
        clip_date_filter: { kind: 'quarter', year: 2025, quarter: 1 },
        clip_sort: 'oldest_first',
        timezone: 'Europe/Berlin',
      },
      ...(state === 'empty' ? { clips: [] } : {}),
    });
    if (state === 'loading') {
      value.holdNext('getPreferences');
      value.holdNext('getLatestImport');
    } else if (state === 'error') {
      value.failNext('getPreferences');
      value.failNext('getLatestImport');
    }
    return value;
  });
  return <ImportPoolPanel twitchUserId={twitchUserId} api={fixture.api} />;
}

export default function SocialWorkspaces(props: SocialMediaProps) {
  const workspaceSlots = {
    ...props.workspaceSlots,
    libraryImport: props.workspaceSlots?.libraryImport ?? (
      <PreviewImport
        key={props.twitchUserId ?? ''}
        twitchUserId={props.twitchUserId}
      />
    ),
  };
  return (
    <>
      <p
        className="studio-preview-notice"
        data-preview="p3-social-workspace-synthetic-v1"
      >
        Lokale Layoutvorschau mit synthetischen Daten. Änderungen und
        Importaufträge bleiben lokal. Keine KI-Aufrufe oder Veröffentlichungen.
      </p>
      <SocialMedia
        {...props}
        workspaceSlots={{
          ...workspaceSlots,
          automation: props.workspaceSlots?.automation ?? (
            <PreviewAutomation
              key={props.twitchUserId ?? ''}
              twitchUserId={props.twitchUserId}
            />
          ),
        }}
      />
    </>
  );
}
