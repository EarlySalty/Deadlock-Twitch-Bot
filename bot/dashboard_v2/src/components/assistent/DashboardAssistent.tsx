import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { Bot, Loader2, MessageCircle, Send, X } from 'lucide-react';
import { useLanguage, useT } from '../../context/LanguageContext';
import { useAuthStatus } from '../../hooks/useAnalytics';
import { askAssistent, AssistentRateLimitError } from '../../api/assistent';
import { vorschlaegeFuer } from './vorschlaege';
import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
} from '../../preview/routes';
import { resolveTabParam } from '../../tabAliases';
import './assistent.css';

interface ChatNachricht {
  id: number;
  role: 'user' | 'bot';
  text: string;
  sources?: string[];
}

function seiteSlug(): string {
  const path = window.location.pathname.replace(/\/+$/, '') || '/';
  const search = window.location.search;
  let slug = 'standard';
  if (path === PREVIEW_HOME_ROUTE || path === '/twitch/dashboard' || path === '/dashboard') {
    slug = 'home';
  } else if (path === PREVIEW_VERWALTUNG_ROUTE || path === '/twitch/verwaltung') {
    slug = 'verwaltung';
  } else if (path === PREVIEW_UPLINK_ROUTE || path === '/twitch/uplink') {
    slug = 'uplink';
  } else if (path === '/social-media-admin') {
    slug = 'social-media';
  } else if (path === PREVIEW_OVERLAY_ROUTE || path === '/twitch/overlay') {
    slug = 'overlay';
  } else if (path === PREVIEW_PRICING_ROUTE || path === '/twitch/pricing') {
    slug = 'pricing';
  } else if (
    path === PREVIEW_ANALYTICS_ROUTE ||
    path === '/analyse' ||
    path === '/twitch/onboarding' ||
    path === '/dashboard-v2' ||
    path === '/twitch/dashboard-v2'
  ) {
    const tab = resolveTabParam(new URLSearchParams(search).get('tab'));
    slug = `analyse/${tab?.tab ?? 'overview'}`;
  }
  return slug.toLowerCase().replace(/[^a-z0-9/_-]/g, '').slice(0, 64);
}

export function DashboardAssistent() {
  const { language } = useLanguage();
  const t = useT();
  const { data: authStatus } = useAuthStatus();

  const [open, setOpen] = useState(false);
  const [question, setQuestion] = useState('');
  const [messages, setMessages] = useState<ChatNachricht[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const messageId = useRef(0);
  const [slug] = useState(() => seiteSlug());

  const name = authStatus?.displayName || authStatus?.twitchLogin || '';
  const csrfToken = authStatus?.csrfToken ?? authStatus?.csrf_token ?? null;
  const vorschlaege = vorschlaegeFuer(slug, language);

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [open]);

  function addMessage(role: ChatNachricht['role'], text: string, sources?: string[]) {
    messageId.current += 1;
    setMessages((current) => [...current, { id: messageId.current, role, text, sources }]);
  }

  async function ask(raw: string) {
    const text = raw.trim();
    if (!text || loading) return;

    const history = messages.slice(-8).map((message) => ({
      role: message.role === 'user' ? ('user' as const) : ('assistant' as const),
      content: message.text,
    }));

    addMessage('user', text);
    setQuestion('');
    setLoading(true);
    setError(null);

    try {
      const antwort = await askAssistent({ question: text, history, page: slug, language, csrfToken });
      const teile = antwort.parts.length ? antwort.parts : antwort.answer ? [antwort.answer] : [];
      if (!teile.length) {
        setError(t('Die Antwort konnte nicht geladen werden.'));
        return;
      }
      teile.forEach((teil, index) => {
        addMessage('bot', teil, index === teile.length - 1 ? antwort.sources : undefined);
      });
    } catch (err) {
      if (err instanceof AssistentRateLimitError) {
        setError(t('Zu viele Fragen gerade. Probier es gleich noch einmal.'));
      } else {
        setError(t('Die Antwort konnte nicht geladen werden.'));
      }
    } finally {
      setLoading(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void ask(question);
  }

  const gruss = name
    ? t('Hi {name}! Ich helfe dir hier im Dashboard weiter. Frag mich alles zum Bot, zum Partnernetz und zu deinem Kanal.', { name })
    : t('Hi! Ich helfe dir hier im Dashboard weiter. Frag mich alles zum Bot, zum Partnernetz und zu deinem Kanal.');

  return (
    <div className="assistent-wrap">
      {open && (
        <section role="dialog" aria-label={t('Hilfe im Dashboard')} className="assistent-panel">
          <header className="assistent-kopf">
            <div className="assistent-kopf-titel">
              <span className="assistent-abzeichen">
                <Bot size={20} />
              </span>
              <div>
                <p className="assistent-name">{t('Deine Hilfe')}</p>
                <p className="assistent-unterzeile">{t('Fragen zum Bot und deinem Kanal')}</p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label={t('Chat schließen')}
              className="assistent-schliessen"
            >
              <X size={19} />
            </button>
          </header>

          <div className="assistent-verlauf" aria-live="polite">
            {!messages.length && (
              <>
                <div className="assistent-gruss">{gruss}</div>
                <div className="assistent-chips">
                  {vorschlaege.map((vorschlag) => (
                    <button
                      key={vorschlag}
                      type="button"
                      onClick={() => void ask(vorschlag)}
                      className="assistent-chip"
                    >
                      {vorschlag}
                    </button>
                  ))}
                </div>
              </>
            )}

            {messages.map((message) => (
              <div
                key={message.id}
                className={`assistent-zeile ${message.role === 'user' ? 'assistent-zeile-user' : 'assistent-zeile-bot'}`}
              >
                <div
                  className={`assistent-blase ${message.role === 'user' ? 'assistent-blase-user' : 'assistent-blase-bot'}`}
                >
                  <p>{message.text}</p>
                  {message.sources?.length ? (
                    <p className="assistent-quelle">
                      {t('Quelle')}: {message.sources.join(', ')}
                    </p>
                  ) : null}
                </div>
              </div>
            ))}

            {loading && (
              <div className="assistent-laden">
                <Loader2 size={16} className="assistent-spinner" />
                {t('Antwort wird erstellt …')}
              </div>
            )}

            {error && <p className="assistent-fehler">{error}</p>}
          </div>

          <form onSubmit={submit} className="assistent-form">
            <input
              ref={inputRef}
              value={question}
              onChange={(event) => setQuestion(event.target.value)}
              maxLength={500}
              disabled={loading}
              placeholder={t('Deine Frage …')}
              aria-label={t('Deine Frage')}
              className="assistent-eingabe"
            />
            <button
              type="submit"
              disabled={loading || !question.trim()}
              aria-label={t('Frage senden')}
              className="assistent-senden"
            >
              {loading ? <Loader2 size={18} className="assistent-spinner" /> : <Send size={18} />}
            </button>
          </form>
        </section>
      )}

      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-label={open ? t('Hilfe schließen') : t('Hilfe bekommen')}
        aria-expanded={open}
        className="assistent-knopf"
      >
        {open ? <X size={20} /> : <MessageCircle size={20} />}
        <span>{open ? t('Schließen') : t('Hilfe bekommen')}</span>
      </button>
    </div>
  );
}
