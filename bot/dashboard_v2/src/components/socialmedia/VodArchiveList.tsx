import type { ComponentProps } from 'react';
import { VodArchiveTab } from './VodArchiveTab';

export function VodArchiveList(props: ComponentProps<typeof VodArchiveTab>) {
  return <div className="studio-vod-list"><VodArchiveTab {...props} /></div>;
}
