import { fetchGreetingSettings, toggleGreeting } from '@/api/greeting';
import { ToggleCommandRow } from './ui';
import { useSettingToggle } from './useSettingToggle';

const load = async () => Boolean((await fetchGreetingSettings()).greeting_reply_enabled);
const save = async (next: boolean) => Boolean((await toggleGreeting(next)).greeting_reply_enabled);

export function GreetingSection() {
  const state = useSettingToggle(load, save);
  return (
    <div tabIndex={-1} className="border-b border-border/60" data-tour-id="onboarding-chat" data-tour-ready={!state.loading && state.enabled !== null}>
      <ToggleCommandRow
        state={state}
        title="Begrüßung"
        onText="Der Bot grüßt zurück, höchstens einmal alle 10 Minuten."
        offText="Der Bot grüßt niemanden mehr zurück."
        label="Begrüßung"
      />
    </div>
  );
}
