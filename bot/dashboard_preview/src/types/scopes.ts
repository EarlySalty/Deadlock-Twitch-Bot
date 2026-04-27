export type ScopeImportance = 'critical' | 'required' | 'optional';

export interface ScopeDefinition {
  id: string;
  label: string;
  description: string;
  why: string;
  importance: ScopeImportance;
  addedAt?: string;
}

export interface ScopeStatus extends ScopeDefinition {
  status: 'granted' | 'missing';
}

export interface ScopeSummary {
  total: number;
  granted: number;
  missing: ScopeStatus[];
  criticalMissing: ScopeStatus[];
  coverage: number;
  lastChange?: string;
}

export interface ScopeChangelogEntry {
  date: string; // ISO-8601
  title: string;
  items: string[];
  tags?: string[];
}
