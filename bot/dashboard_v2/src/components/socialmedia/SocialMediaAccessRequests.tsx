import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ShieldAlert, UserMinus, UserPlus, Users } from 'lucide-react';
import { useLanguage, useT } from '@/context/LanguageContext';
import {
  fetchAccessRequests,
  rejectAccessRequest,
  requestSocialMediaAccess,
  setPartnerAccess,
  type SocialMediaAccessRequest,
} from '@/api/socialMedia';

const REQUESTS_KEY = ['social-media-access-requests'];

function anfrageZeit(roh: string, locale: string): string {
  const datum = new Date(roh);
  if (Number.isNaN(datum.getTime())) return roh;
  return new Intl.DateTimeFormat(locale, { dateStyle: 'short', timeStyle: 'short' }).format(datum);
}

export function SocialMediaAccessRequestCard({ requested }: { requested: boolean }) {
  const t = useT();
  const queryClient = useQueryClient();
  const anfrage = useMutation({
    mutationFn: requestSocialMediaAccess,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['social-media-access'] }),
  });
  const gesendet = requested || anfrage.isSuccess;

  return (
    <div className="panel-card rounded-2xl p-8 text-center">
      <ShieldAlert className="w-12 h-12 text-warning mx-auto mb-4" />
      <h2 className="text-xl font-bold text-white mb-2">{t('Noch nicht freigeschaltet')}</h2>
      <p className="text-text-secondary">
        {gesendet
          ? t('Deine Anfrage ist eingegangen. Sobald EarlySalty sie freigibt, kannst du hier loslegen.')
          : t('Social Media wird für deinen Kanal erst nach Freigabe aktiv. Frag den Zugang hier an, EarlySalty schaltet dich dann frei.')}
      </p>
      <button
        type="button"
        disabled={gesendet || anfrage.isPending}
        onClick={() => anfrage.mutate()}
        className="mt-5 inline-flex min-h-10 items-center justify-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-60"
      >
        {gesendet ? t('Anfrage gesendet') : anfrage.isPending ? t('Wird gesendet …') : t('Zugang anfragen')}
      </button>
      {anfrage.isError ? (
        <p role="alert" className="mt-3 text-xs text-warning">{t('Die Anfrage hat nicht geklappt. Versuch es gleich noch einmal.')}</p>
      ) : null}
    </div>
  );
}

function entfernen(queryClient: ReturnType<typeof useQueryClient>, twitchUserId: string) {
  queryClient.setQueryData<SocialMediaAccessRequest[]>(REQUESTS_KEY, (alt) =>
    (alt ?? []).filter((eintrag) => eintrag.twitch_user_id !== twitchUserId),
  );
  queryClient.invalidateQueries({ queryKey: REQUESTS_KEY });
}

export function AdminSocialMediaAccessRequests() {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const anfragen = useQuery({
    queryKey: REQUESTS_KEY,
    queryFn: fetchAccessRequests,
    retry: false,
    refetchOnWindowFocus: true,
  });
  const freischalten = useMutation({
    mutationFn: (twitchUserId: string) => setPartnerAccess(twitchUserId, true),
    onSuccess: (_antwort, twitchUserId) => {
      entfernen(queryClient, twitchUserId);
      queryClient.invalidateQueries({ queryKey: ['social-media-access-list'] });
    },
  });
  const ablehnen = useMutation({
    mutationFn: rejectAccessRequest,
    onSuccess: (_antwort, twitchUserId) => entfernen(queryClient, twitchUserId),
  });
  const eintraege = anfragen.data ?? [];
  const beschaeftigt = freischalten.isPending || ablehnen.isPending;

  if (anfragen.isLoading || (!anfragen.isError && eintraege.length === 0)) return null;

  return (
    <section
      data-section="social-media-access-requests"
      className="panel-card card-glow card-glow-warning space-y-4 rounded-2xl border-warning/25 p-4 md:p-5"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-warning/35 bg-warning/10 text-warning">
            <Users aria-hidden="true" className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-warning">{t('Verwaltung')}</div>
            <h2 className="text-base font-bold text-white">{t('Social-Media-Anfragen')}</h2>
            <p className="mt-1 text-xs text-text-secondary">
              {t('Freischalten gibt dem Streamer Zugang zum Social-Media-Manager und entfernt die Anfrage aus der Liste.')}
            </p>
          </div>
        </div>
        <span className="rounded-full border border-warning/30 bg-warning/10 px-2.5 py-1 text-xs font-semibold text-warning">
          {eintraege.length}
        </span>
      </div>

      {anfragen.isError ? (
        <p role="alert" className="rounded-xl border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
          {t('Die Anfragen sind gerade nicht erreichbar.')}
        </p>
      ) : null}

      {eintraege.length > 0 ? (
        <ul className="max-h-72 space-y-2 overflow-y-auto pr-1" aria-label={t('Social-Media-Anfragen')}>
          {eintraege.map((eintrag) => {
            const wirdFreigeschaltet = freischalten.isPending && freischalten.variables === eintrag.twitch_user_id;
            const wirdAbgelehnt = ablehnen.isPending && ablehnen.variables === eintrag.twitch_user_id;
            return (
              <li
                key={eintrag.twitch_user_id}
                className="flex flex-col gap-3 rounded-xl border border-border bg-background/55 p-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-white">{eintrag.twitch_login}</p>
                  <p className="mt-0.5 text-[11px] text-text-secondary">
                    {t('Twitch-ID {id} · Anfrage {zeit}', { id: eintrag.twitch_user_id, zeit: anfrageZeit(eintrag.requested_at, locale) })}
                  </p>
                </div>
                <div className="flex shrink-0 flex-wrap items-center gap-2">
                  <button
                    type="button"
                    disabled={beschaeftigt}
                    onClick={() => freischalten.mutate(eintrag.twitch_user_id)}
                    className="inline-flex min-h-10 shrink-0 items-center justify-center gap-2 rounded-xl bg-primary px-3 py-2 text-xs font-semibold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <UserPlus aria-hidden="true" className="h-3.5 w-3.5" />
                    {wirdFreigeschaltet ? t('Wird freigeschaltet …') : t('Freischalten')}
                  </button>
                  <button
                    type="button"
                    disabled={beschaeftigt}
                    onClick={() => ablehnen.mutate(eintrag.twitch_user_id)}
                    className="inline-flex min-h-10 shrink-0 items-center justify-center gap-2 rounded-xl border border-border bg-background/70 px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <UserMinus aria-hidden="true" className="h-3.5 w-3.5" />
                    {wirdAbgelehnt ? t('Wird abgelehnt …') : t('Ablehnen')}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}

      {freischalten.isError ? (
        <p role="alert" className="text-xs text-warning">{t('Die Freischaltung hat nicht geklappt.')}</p>
      ) : null}
      {ablehnen.isError ? (
        <p role="alert" className="text-xs text-warning">{t('Das Ablehnen hat nicht geklappt.')}</p>
      ) : null}
    </section>
  );
}
