import { Lightbulb, MessageSquare } from 'lucide-react';
import { feedbackHref } from '../../api/feedback';
import './feedback.css';

export function FeedbackBox({ area }: { area?: string }) {
  return <section className="panel-card feedback-card rounded-2xl p-5 md:p-6" data-tour-id="onboarding-feedback" data-tour-ready="true" tabIndex={-1} aria-label="Kritik und Wünsche">
    <div className="flex items-start gap-3">
      <MessageSquare className="mt-1 h-6 w-6 shrink-0 text-primary" aria-hidden="true" />
      <div><h2 className="text-xl font-bold text-white">Was fehlt dir? Was nervt?</h2>
        <p className="mt-2 text-sm leading-6 text-text-secondary"><strong className="text-white">Kritik ist ausdrücklich erwünscht.</strong> Sag uns, was unklar ist, dich stört oder dir noch fehlt.</p>
      </div>
    </div>
    <div className="mt-5 flex flex-wrap gap-3">
      <a className="feedback-button feedback-primary" href={feedbackHref('feedback', area)}>Feedback geben</a>
      <a className="feedback-button" href={feedbackHref('feature', area)}><Lightbulb className="h-4 w-4" aria-hidden="true" />Feature wünschen</a>
    </div>
    <a className="mt-4 inline-block text-sm text-text-secondary underline decoration-primary/50 underline-offset-4 hover:text-white" href={feedbackHref()}>Meine Rückmeldungen und Antworten</a>
  </section>;
}
