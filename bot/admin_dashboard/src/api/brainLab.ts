export interface BrainRequest {
  hero: string;
  combat_window_seconds: number;
  channel_uptime: number;
  incoming_weapon_share: number;
  incoming_pressure_dps: number | null;
}
export interface BrainCatalog {
  heroes: { id: number; name: string }[];
  patch_tag: string;
  sources: { source: string; entity_type: string; fetched_at: number | null }[];
  mode: string;
}
export interface BrainEvidence { kind: string; detail: string }
export interface BrainItem {
  item_id: number; name: string; tier: number; buy_phase: string; why: string;
  confidence: string; imbue_target: number | null; sell_priority: number | null;
  sources: BrainEvidence[];
}
export interface BrainFamily {
  id: string; label: string; player_matches: number; distinct_players: number;
  post_patch_player_matches: number | null; cohesion: number; limitations: string[];
  skill_order_support: number | null;
}
export interface BrainBuild {
  hero_id: number; hero_name: string; patch_tag: string; name: string;
  core: BrainItem[];
  situations: { label: string; optional: boolean; kind: unknown; items: BrainItem[] }[];
  ability_order: { ability_id: number; currency_type: number; delta: number }[];
  confidence: string; rationale: string; family: BrainFamily | null; variants: BrainBuild[];
}
export interface BrainScore {
  item: { item_id: number; name: string; cost: number; slot: string; tier: number; imbueable: boolean; properties: Record<string, number> };
  score: { total: number; combat_value: number; per_slot_value: number; per_soul_value: number; purchase_bonus_value: number; condition_factor: number; active_value: number; passive_value: number; meta_support: number };
  confidence: string; buy_phase: string; sources: BrainEvidence[];
}
export interface BrainReport {
  schema_version: number; mode: string; publish_allowed: boolean; model_fingerprint: string;
  config: Omit<BrainRequest, 'hero'> & { patch_tag: string; bracket: string };
  sources: { source: string; oldest_fetched_at: number | null; newest_fetched_at: number | null; fields: number; fields_without_timestamp: number }[];
  warnings: string[]; build: BrainBuild;
  hero: { hero_id: number; name: string; abilities: { ability_id: number; name: string }[] };
  scored: BrainScore[]; variant_scores: Record<string, BrainScore[]>;
  patch_deltas: { target: unknown; mechanic: string; sign: number; magnitude: number; note: string; application: unknown | null }[];
}
