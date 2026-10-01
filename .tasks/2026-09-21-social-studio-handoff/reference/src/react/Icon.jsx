import React from 'react';
import { ICON_PATHS } from '../icons.js';
/** Only static, application-owned SVG paths are rendered here, never user input. */
export function Icon({ name, className = 'h-4 w-4' }) {
    return <svg aria-hidden="true" className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" dangerouslySetInnerHTML={{ __html: ICON_PATHS[name] || ICON_PATHS.film }}/>;
}
