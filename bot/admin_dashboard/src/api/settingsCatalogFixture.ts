import type { SettingsCatalog, SettingsField } from './settingsCatalog.ts';

const base = { nullable: false, restart_required: true, min: null, max: null, choices: [], min_items: null, max_items: null };

const fields: SettingsField[] = [
  { ...base, path: 'twitch.bot_user_id', group: 'twitch', label: 'Bot-Konto-ID', help: 'Numerische Twitch-ID des Bot-Kontos.', kind: 'string' },
  { ...base, path: 'twitch.language_filters', group: 'twitch', label: 'Sprachfilter', help: 'Stream-Sprachen in Kleinbuchstaben, zum Beispiel de oder de-at.', kind: 'string_list', nullable: true, min_items: 1, max_items: 16 },
  { ...base, path: 'bot.runtime_enforce', group: 'bot', label: 'Laufzeitsperre erzwingen', help: 'Verhindert, dass zwei Prozesse dieselbe Rolle übernehmen.', kind: 'boolean', nullable: true },
  { ...base, path: 'bot.legacy_internal_api_port', group: 'bot', label: 'Port der alten internen API', help: 'Nur für den Übergang; leer lassen, wenn nicht gebraucht.', kind: 'integer', nullable: true, min: '1', max: '65535' },
  { ...base, path: 'bot.auto_unraid_window_seconds', group: 'bot', label: 'Auto-Unraid-Fenster', help: 'Sekunden, in denen ein Raid bei Rückkehr der Quelle abgebrochen wird.', kind: 'number', nullable: true, min: '0' },
  { ...base, path: 'discord.raid_oauth.allowed_guild_ids', group: 'discord', label: 'Erlaubte Server für Raid-Freigabe', help: 'Leer lassen schaltet die Prüfung ab, eine leere Liste erlaubt niemanden.', kind: 'integer_list', nullable: true, min: '1', max: '9223372036854775807', min_items: 0, max_items: 512 },
  { ...base, path: 'social_ai.provider.kind', group: 'social_ai_provider', label: 'Anbieter', help: 'KI-Anbieter des Schnitt-Editors.', kind: 'choice', choices: ['anthropic'] },
  { ...base, path: 'social_ai.provider.effort', group: 'social_ai_provider', label: 'Denkaufwand', help: 'Aufwandsstufe je Anfrage.', kind: 'choice', choices: ['low', 'medium', 'high', 'xhigh', 'max'] },
  { ...base, path: 'social_ai.provider.max_input_tokens', group: 'social_ai_provider', label: 'Größte Eingabe', help: 'Eingabe-Tokens je Anfrage.', kind: 'integer', min: '1', max: '128000' },
  { ...base, path: 'social_ai.provider.input_price_micro_usd_per_mtok', group: 'social_ai_provider', label: 'Eingabepreis', help: 'Mikro-US-Dollar je Million Eingabe-Tokens.', kind: 'integer', min: '1', max: '100000000' },
];

export const settingsCatalogFixture: SettingsCatalog = {
  version: 1,
  revision: 'a'.repeat(64),
  groups: [
    { id: 'twitch', label: 'Twitch' },
    { id: 'bot', label: 'Bot-Betrieb' },
    { id: 'discord', label: 'Discord' },
    { id: 'social_ai_provider', label: 'Social AI: Anbieter und Preise' },
  ],
  fields: [...fields, fields[2]],
  values: {
    'twitch.bot_user_id': '123456789',
    'twitch.language_filters': ['de', 'de-at'],
    'bot.runtime_enforce': true,
    'bot.legacy_internal_api_port': null,
    'bot.auto_unraid_window_seconds': 90,
    'discord.raid_oauth.allowed_guild_ids': ['1289721245281292288'],
    'social_ai.provider.kind': 'anthropic',
    'social_ai.provider.effort': 'high',
    'social_ai.provider.max_input_tokens': '64000',
    'social_ai.provider.input_price_micro_usd_per_mtok': '3000000',
  },
};
