import type { PlatformStatus, SocialClipMitPosting } from '@/api/socialMedia';
import { useT } from '@/context/LanguageContext';
import { WorkspaceDialog } from './WorkspaceDialog';
import { PlatformStatusPanel } from './PlatformStatusPanel';

export function PlatformStatusDialog({ clip, platforms, loading, error, onClose }: {
  clip: SocialClipMitPosting;
  platforms: PlatformStatus[];
  loading: boolean;
  error: unknown;
  onClose: () => void;
}) {
  const t = useT();
  return (
    <WorkspaceDialog eyebrow={t('Plattformstatus')} title={clip.title} onClose={onClose}>
      <PlatformStatusPanel clip={clip} platforms={platforms} load={error ? 'error' : loading ? 'loading' : 'ready'} />
    </WorkspaceDialog>
  );
}
