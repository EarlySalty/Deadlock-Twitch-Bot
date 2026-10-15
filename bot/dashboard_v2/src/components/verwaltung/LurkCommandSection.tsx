import { fetchLurkCommandSettings, toggleLurkCommand } from '@/api/lurkCommand';
import { ToggleCommandRow } from './ui';
import { useSettingToggle } from './useSettingToggle';

const load = async () => Boolean((await fetchLurkCommandSettings()).lurk_command_enabled);
const save = async (next: boolean) => Boolean((await toggleLurkCommand(next)).lurk_command_enabled);

export function LurkCommandSection() {
  const state = useSettingToggle(load, save);
  return (
    <ToggleCommandRow
      state={state}
      command="lurk"
      title="Lurk-Befehl"
      onText="Dein Chat kriegt darauf die Standard-Antwort."
      offText="Der Bot bleibt auf den Befehl stumm."
      label="Lurk-Befehl"
    />
  );
}
