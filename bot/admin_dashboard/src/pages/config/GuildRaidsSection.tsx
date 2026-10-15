import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchGuildRaidSettings, saveGuildRaidSetting } from '@/api/client';

export function GuildRaidsSection() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: ['guild-raid-settings'], queryFn: fetchGuildRaidSettings });
  const save = useMutation({
    mutationFn: saveGuildRaidSetting,
    onSuccess: () => client.invalidateQueries({ queryKey: ['guild-raid-settings'] }),
  });
  return <section className="panel-card space-y-4 rounded-2xl p-6" aria-labelledby="guild-raids-heading">
    <h2 id="guild-raids-heading" className="text-lg font-semibold text-white">Raids über Servergrenzen</h2>
    <p className="text-sm text-text-secondary">Standardmäßig sind Raids zu anderen Servern derselben Sprachgruppe erlaubt. Beide Server müssen zustimmen. Änderungen gelten sofort.</p>
    {query.isPending && <p role="status">Server werden geladen …</p>}
    {query.error && <p role="alert">Die Server konnten nicht geladen werden.</p>}
    {query.data?.length === 0 && <p>Keine aktiven Server vorhanden.</p>}
    {query.data?.map(guild => <label key={guild.guild_id} className="flex min-h-11 items-center gap-3">
      <input type="checkbox" checked={guild.cross_server_raids} disabled={save.isPending}
        onChange={event => save.mutate({ guild_id: guild.guild_id, cross_server_raids: event.target.checked })} />
      <span>{guild.name || guild.guild_id}: Raids über Servergrenzen erlauben</span>
    </label>)}
    {save.error && <p role="alert">Die Einstellung konnte nicht gespeichert werden.</p>}
    {save.isSuccess && <p role="status">Gespeichert.</p>}
  </section>;
}
