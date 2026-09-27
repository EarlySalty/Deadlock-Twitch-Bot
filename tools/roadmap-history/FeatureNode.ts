export interface FeatureNode {
  id: string;
  title: string;
  description: string;
  date: string;
  category: 'core' | 'twitch' | 'dashboard' | 'api' | 'community';
  type: 'root' | 'major_feature' | 'update' | 'refactor';
  parentId: string | null;
  commitHash?: string;
  prUrl?: string;
  repository?: 'EarlySalty/Deadlock-Bots' | 'EarlySalty/Deadlock-Twitch-Bot';
  role?: 'root' | 'feature' | 'event';
  featureId?: string;
  commitIds?: string[];
  spanEnd?: string;
  maintenance?: boolean;
  count?: number;
  evidenceCount?: number;
  relation?: { kind: 'editorial' | 'historical'; reason: string; sources?: string[]; commit?: string };
}
