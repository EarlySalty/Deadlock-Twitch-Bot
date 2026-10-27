import { FlaskConical } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import { feedbackHref } from '@/api/feedback';
import '@/components/feedback/feedback.css';

const FEEDBACK_AREA = 'Social Media';

const SECTIONS: Array<{ title: string; text: string }> = [
  { title: 'Heute', text: 'Zeigt, was gerade ansteht, was zuletzt gepostet wurde und wo etwas hängt.' },
  { title: 'Clips', text: 'Hier landen deine Twitch-Clips. Auf Wunsch schneidet die KI daraus ein Hochformat-Video mit Untertiteln, das du vorher ansehen kannst.' },
  { title: 'Planen', text: 'Du legst fest, wann und auf welcher Plattform ein Video erscheint.' },
  { title: 'VOD-Archiv', text: 'Sichert deine Streams, wenn du das Archiv einschaltest.' },
  { title: 'Einstellungen', text: 'Verbindungen zu TikTok, YouTube und Instagram sowie die Einstellungen für die KI-Bearbeitung.' },
];

export function SocialMediaBetaNotice() {
  const t = useT();
  return <section className="panel-card rounded-2xl p-5" aria-labelledby="social-beta-title">
    <div className="flex items-start gap-3">
      <FlaskConical aria-hidden="true" className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
      <div className="min-w-0 space-y-2">
        <h2 id="social-beta-title" className="text-base font-semibold text-white">{t('Beta: Hier kann noch einiges haken')}</h2>
        <p className="text-sm leading-6 text-text-secondary">{t('Der Social-Media-Manager macht aus deinen Twitch-Clips Hochformat-Videos für TikTok, YouTube Shorts und Instagram. Du entscheidest, ob du jedes Video vorher prüfst oder ob es automatisch gepostet wird.')}</p>
        <p className="text-sm leading-6 text-text-secondary"><strong className="text-white">{t('Vieles ist neu und noch nicht fertig.')}</strong> {t('Wenn dir etwas unklar ist, nicht funktioniert oder du nicht weißt, was ein Knopf macht, schreib es uns bitte. Auch Kleinigkeiten helfen, denn nur so sehen wir, wo es hakt.')}</p>
        <details className="text-sm text-text-secondary">
          <summary className="cursor-pointer font-medium text-white">{t('Was steckt in den einzelnen Bereichen?')}</summary>
          <dl className="mt-2 space-y-2">
            {SECTIONS.map(({ title, text }) => <div key={title}><dt className="font-medium text-white">{t(title)}</dt><dd>{t(text)}</dd></div>)}
          </dl>
        </details>
        <div className="flex flex-wrap gap-3 pt-1">
          <a className="feedback-button feedback-primary" href={feedbackHref('feedback', FEEDBACK_AREA)}>{t('Unklar oder kaputt? Sag es uns')}</a>
          <a className="feedback-button" href={feedbackHref('feature', FEEDBACK_AREA)}>{t('Feature wünschen')}</a>
        </div>
      </div>
    </div>
  </section>;
}
