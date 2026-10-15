import type { ReactNode } from 'react';
import type { SocialPlatform } from '@/types/socialMedia';
import { platformIconUrl } from './platformIcon';

export function PlatformCheck({ platform, children }: { platform: SocialPlatform; children: ReactNode }) {
  return <div className="studio-platform-check" data-platform={platform}><img src={platformIconUrl(platform)} width="28" height="28" alt="" aria-hidden /><div className="studio-platform-check-content">{children}</div></div>;
}
