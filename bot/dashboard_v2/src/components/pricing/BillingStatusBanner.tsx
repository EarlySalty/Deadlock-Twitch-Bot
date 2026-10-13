import { useMemo, useState } from 'react';
import { Rise } from '../../motion/Rise';
import { AlertTriangle, CheckCircle2, Info, X } from 'lucide-react';
import { useT } from '../../context/LanguageContext';
import { tr } from '../../i18n/dictionary';

type Tone = 'success' | 'info' | 'warn';

interface BannerMessage {
  tone: Tone;
  text: string;
}

function invoiceMessages(): Record<string, BannerMessage> {
  return {
    missing_customer: {
      tone: 'info',
      text: tr('Zu deinem Konto ist noch kein abgeschlossenes Abo mit Zahlungsdaten hinterlegt. Sobald du ein Abo abschließt, findest du deine Rechnungen hier im Kundenportal.'),
    },
    portal_unavailable: {
      tone: 'warn',
      text: tr('Das Rechnungsportal ist gerade nicht erreichbar. Bitte versuch es in ein paar Minuten noch einmal.'),
    },
    error: {
      tone: 'warn',
      text: tr('Beim Öffnen des Rechnungsportals ist etwas schiefgelaufen. Bitte versuch es später noch einmal.'),
    },
    portal_returned: {
      tone: 'success',
      text: tr('Willkommen zurück. Deine Änderungen im Kundenportal wurden übernommen.'),
    },
  };
}

function cancelMessages(): Record<string, BannerMessage> {
  return {
    scheduled: {
      tone: 'success',
      text: tr('Deine Kündigung ist zum Ende des Abrechnungszeitraums vorgemerkt. Bis dahin behältst du vollen Zugriff.'),
    },
    returned: {
      tone: 'success',
      text: tr('Willkommen zurück aus dem Kundenportal.'),
    },
    missing: {
      tone: 'info',
      text: tr('Wir konnten kein aktives Abo zum Kündigen finden.'),
    },
    post_required: {
      tone: 'warn',
      text: tr('Die Kündigung konnte so nicht ausgelöst werden. Bitte nutze im Kundenportal den Button „Abo verwalten".'),
    },
    csrf_invalid: {
      tone: 'warn',
      text: tr('Die Sitzung ist abgelaufen. Bitte lade die Seite neu und versuch es noch einmal.'),
    },
    error: {
      tone: 'warn',
      text: tr('Beim Kündigen ist etwas schiefgelaufen. Bitte versuch es später noch einmal oder öffne das Kundenportal.'),
    },
  };
}

const TONE_STYLES: Record<Tone, string> = {
  success: 'border-[#00D9FF]/30 bg-[#00D9FF]/10 text-[#5CE7FF]',
  info: 'border-white/15 bg-white/5 text-white/70',
  warn: 'border-[#C5A059]/30 bg-[#C5A059]/10 text-[#C5A059]',
};

const TONE_ICON = {
  success: CheckCircle2,
  info: Info,
  warn: AlertTriangle,
} as const;

function resolveMessage(): BannerMessage | null {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(window.location.search);
  const invoice = params.get('invoice');
  const invoiceMessage = invoice ? invoiceMessages()[invoice] : undefined;
  if (invoiceMessage) return invoiceMessage;
  const cancel = params.get('cancel');
  const cancelMessage = cancel ? cancelMessages()[cancel] : undefined;
  return cancelMessage ?? null;
}

export default function BillingStatusBanner() {
  const t = useT();
  const message = useMemo(() => resolveMessage(), []);
  const [dismissed, setDismissed] = useState(false);

  if (!message || dismissed) return null;

  const Icon = TONE_ICON[message.tone];

  return (
    <Rise
      className={`mb-6 flex items-start gap-3 rounded-xl border px-4 py-3 ${TONE_STYLES[message.tone]}`}
      role="status"
    >
      <Icon className="mt-0.5 h-4 w-4 flex-shrink-0" />
      <p className="flex-1 text-sm leading-relaxed">{message.text}</p>
      <button
        onClick={() => setDismissed(true)}
        className="-mr-1 flex-shrink-0 rounded-md p-1 opacity-60 transition-opacity hover:opacity-100"
        aria-label={t('Hinweis schließen')}
      >
        <X className="h-4 w-4" />
      </button>
    </Rise>
  );
}
