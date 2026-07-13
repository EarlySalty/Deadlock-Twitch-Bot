import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { AlertTriangle, CheckCircle2, Info, X } from 'lucide-react';

type Tone = 'success' | 'info' | 'warn';

interface BannerMessage {
  tone: Tone;
  text: string;
}

// Uebersetzt die ?invoice=… / ?cancel=… Redirect-Parameter der Billing-Routen
// in verstaendliche Hinweise. Schluessel = Parameterwert aus billing_page.rs.
const INVOICE_MESSAGES: Record<string, BannerMessage> = {
  missing_customer: {
    tone: 'info',
    text: 'Zu deinem Konto ist noch kein abgeschlossenes Abo mit Zahlungsdaten hinterlegt. Sobald du ein Abo abschließt, findest du deine Rechnungen hier im Kundenportal.',
  },
  portal_unavailable: {
    tone: 'warn',
    text: 'Das Rechnungsportal ist gerade nicht erreichbar. Bitte versuch es in ein paar Minuten noch einmal.',
  },
  error: {
    tone: 'warn',
    text: 'Beim Öffnen des Rechnungsportals ist etwas schiefgelaufen. Bitte versuch es später noch einmal.',
  },
  portal_returned: {
    tone: 'success',
    text: 'Willkommen zurück. Deine Änderungen im Kundenportal wurden übernommen.',
  },
};

const CANCEL_MESSAGES: Record<string, BannerMessage> = {
  scheduled: {
    tone: 'success',
    text: 'Deine Kündigung ist zum Ende des Abrechnungszeitraums vorgemerkt. Bis dahin behältst du vollen Zugriff.',
  },
  returned: {
    tone: 'success',
    text: 'Willkommen zurück aus dem Kundenportal.',
  },
  missing: {
    tone: 'info',
    text: 'Wir konnten kein aktives Abo zum Kündigen finden.',
  },
  post_required: {
    tone: 'warn',
    text: 'Die Kündigung konnte so nicht ausgelöst werden. Bitte nutze im Kundenportal den Button „Abo verwalten".',
  },
  csrf_invalid: {
    tone: 'warn',
    text: 'Die Sitzung ist abgelaufen. Bitte lade die Seite neu und versuch es noch einmal.',
  },
  error: {
    tone: 'warn',
    text: 'Beim Kündigen ist etwas schiefgelaufen. Bitte versuch es später noch einmal oder öffne das Kundenportal.',
  },
};

const TONE_STYLES: Record<Tone, string> = {
  success: 'border-[#55978f]/30 bg-[#55978f]/10 text-[#6fb3aa]',
  info: 'border-white/15 bg-white/5 text-white/70',
  warn: 'border-[#c8a86b]/30 bg-[#c8a86b]/10 text-[#c8a86b]',
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
  if (invoice && INVOICE_MESSAGES[invoice]) return INVOICE_MESSAGES[invoice];
  const cancel = params.get('cancel');
  if (cancel && CANCEL_MESSAGES[cancel]) return CANCEL_MESSAGES[cancel];
  return null;
}

/**
 * Zeigt — falls die Seite über einen Billing-Redirect erreicht wurde — einen
 * verständlichen Hinweis statt eines nackten URL-Parameters. Schließbar.
 */
export default function BillingStatusBanner() {
  const message = useMemo(resolveMessage, []);
  const [dismissed, setDismissed] = useState(false);

  if (!message || dismissed) return null;

  const Icon = TONE_ICON[message.tone];

  return (
    <motion.div
      initial={{ opacity: 0, y: -8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className={`mb-6 flex items-start gap-3 rounded-xl border px-4 py-3 ${TONE_STYLES[message.tone]}`}
      role="status"
    >
      <Icon className="mt-0.5 h-4 w-4 flex-shrink-0" />
      <p className="flex-1 text-sm leading-relaxed">{message.text}</p>
      <button
        onClick={() => setDismissed(true)}
        className="-mr-1 flex-shrink-0 rounded-md p-1 opacity-60 transition-opacity hover:opacity-100"
        aria-label="Hinweis schließen"
      >
        <X className="h-4 w-4" />
      </button>
    </motion.div>
  );
}
