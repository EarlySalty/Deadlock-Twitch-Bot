import { useState } from 'react';
import { useT } from '@/context/LanguageContext';
import { SocialMedia, type SocialMediaProps } from '@/pages/SocialMedia';
import { ImportPoolPanel } from '@/components/socialmedia/autopilot/import-pool';
import { createImportPoolFixture } from '@/components/socialmedia/autopilot/import-pool/fixtures';
import {
  createScheduleQueueFixture,
  type ScheduleQueueApi,
} from '@/components/socialmedia/autopilot/schedule-queue';
import { createPublishReviewFixture } from '@/components/socialmedia/autopilot/publish-review/fixtures';
import { previewWorkspaceState } from './socialWorkspaceFixtures';
import { SOCIAL_POSTER } from './fixtures';

function createPreviewScheduleApi(): ScheduleQueueApi {
  const state = previewWorkspaceState(window.location.search, 'automation');
  const fixture = createScheduleQueueFixture();
  if (new URLSearchParams(window.location.search).get('fixture') === 'automation-first-run')
    return {
      ...fixture.api,
      load: async (owner, signal) => ({
        ...(await fixture.api.load(owner, signal)),
        persisted: false,
      }),
    };
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
    const getPool = value.api.getPool;
    value.api.getPool = async (...args) => {
      const page = await getPool(...args);
      return {
        ...page,
        clips: page.clips.map(clip => ({
          ...clip,
          source_metadata: {
            ...(clip.source_metadata && typeof clip.source_metadata === 'object' ? clip.source_metadata : {}),
            thumbnail_url: SOCIAL_POSTER,
          },
        })),
      };
    };
    return value;
  });
  return <ImportPoolPanel twitchUserId={twitchUserId} api={fixture.api} />;
}

export default function SocialWorkspaces(props: SocialMediaProps) {
  const t = useT();
  const [scheduleApi] = useState(createPreviewScheduleApi);
  const [publishFixture] = useState(createPublishReviewFixture);
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
        {t('Lokale Layoutvorschau mit synthetischen Daten. Änderungen und Importaufträge bleiben lokal. Keine KI-Aufrufe oder Veröffentlichungen.')}
      </p>
      <SocialMedia
        {...props}
        scheduleApi={props.scheduleApi ?? scheduleApi}
        publishClient={props.publishClient ?? publishFixture.client}
        workspaceSlots={workspaceSlots}
      />
    </>
  );
}
