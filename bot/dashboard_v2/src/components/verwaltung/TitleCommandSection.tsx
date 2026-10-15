import { fetchTitleCommandSettings, toggleTitleCommand } from '@/api/titleCommand';
import { ToggleCommandRow } from './ui';
import { useSettingToggle } from './useSettingToggle';

const load = async () => Boolean((await fetchTitleCommandSettings()).title_command_enabled);
const save = async (next: boolean) => Boolean((await toggleTitleCommand(next)).title_command_enabled);

export function TitleCommandSection() {
  const state = useSettingToggle(load, save);
  return (
    <ToggleCommandRow
      state={state}
      command="title"
      title="Titel-Befehl"
      onText="Du und deine Mods können Titelvorschläge anfordern."
      offText="Der Bot bleibt auf den Befehl stumm."
      label="Titel-Befehl"
    />
  );
}
