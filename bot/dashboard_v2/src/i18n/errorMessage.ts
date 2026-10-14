import { ApiHttpError } from '../api/httpError';
import { dictionaryFor, type TranslateParams } from './dictionary';

export type DisplayMessage =
  | { key: string; params?: TranslateParams }
  | { raw: string };

export function messageFromText(message: string): DisplayMessage {
  return Object.hasOwn(dictionaryFor('en'), message)
    ? { key: message }
    : { raw: message };
}

export function messageFromError(error: unknown, fallback: string): DisplayMessage {
  if (error instanceof ApiHttpError && error.presentation) return error.presentation;
  return error instanceof Error && error.message
    ? messageFromText(error.message)
    : { key: fallback };
}

export function presentMessage(
  message: DisplayMessage,
  t: (key: string, params?: TranslateParams) => string,
): string {
  return 'raw' in message ? message.raw : t(message.key, message.params);
}
