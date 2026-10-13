/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "en" baut die englische Fassung fuer dead-lock.de, sonst Deutsch. */
  readonly VITE_SITE_LANG?: string;
}
