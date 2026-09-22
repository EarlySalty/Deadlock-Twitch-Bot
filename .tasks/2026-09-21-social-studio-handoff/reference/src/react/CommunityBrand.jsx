import React from 'react';
import { COMMUNITY_LOGO } from '../brand.js';

/** Existing community mark; the data URL keeps the preview independent of image requests. */
export function CommunityBrand({ compact = false }) {
  if (compact) {
    return <img className="brand-logo h-7 w-7 shrink-0 lg:hidden" src={COMMUNITY_LOGO} width={28} height={28} alt="Deutsche Deadlock Community" />;
  }
  return <>
    <img className="brand-logo h-9 w-9 shrink-0" src={COMMUNITY_LOGO} width={36} height={36} alt="" />
    <span className="brand-wordmark"><span>Deutsche Deadlock</span><span className="brand-wordmark-subtitle">COMMUNITY</span></span>
  </>;
}
