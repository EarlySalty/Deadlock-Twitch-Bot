import { buildApiUrl, withCookieCredentials } from './core';
import type { Language } from '../i18n/dictionary';

export interface AssistentVerlaufEintrag {
  role: 'user' | 'assistant';
  content: string;
}

export interface AssistentAntwort {
  answer: string;
  parts: string[];
  sources: string[];
  grounded: boolean;
  page: string;
}

export class AssistentRateLimitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'AssistentRateLimitError';
  }
}

const ASSISTENT_TIMEOUT_MS = 125_000;

export async function askAssistent(params: {
  question: string;
  history: AssistentVerlaufEintrag[];
  page: string;
  language: Language;
  csrfToken?: string | null;
}): Promise<AssistentAntwort> {
  const { question, history, page, language, csrfToken } = params;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ASSISTENT_TIMEOUT_MS);

  try {
    const response = await fetch(
      buildApiUrl('/dashboard/assistent/ask'),
      withCookieCredentials({
        method: 'POST',
        headers: {
          Accept: 'application/json',
          'Content-Type': 'application/json',
          ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
        },
        body: JSON.stringify({ question, history, page, language }),
        signal: controller.signal,
      }),
    );

    if (response.status === 429) {
      throw new AssistentRateLimitError('Zu viele Fragen gerade. Probier es gleich noch einmal.');
    }
    if (!response.ok) {
      throw new Error('Die Antwort konnte nicht geladen werden.');
    }

    const data = (await response.json()) as Partial<AssistentAntwort>;
    return {
      answer: typeof data.answer === 'string' ? data.answer : '',
      parts: Array.isArray(data.parts) ? data.parts : [],
      sources: Array.isArray(data.sources) ? data.sources : [],
      grounded: Boolean(data.grounded),
      page: typeof data.page === 'string' ? data.page : page,
    };
  } finally {
    clearTimeout(timer);
  }
}
