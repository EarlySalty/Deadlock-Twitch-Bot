import type { DisplayMessage } from '../i18n/errorMessage';

export interface ApiErrorSource {
  message?: string;
  error?: string;
  code?: string;
}

export class ApiHttpError extends Error {
  status: number;
  presentation?: DisplayMessage;
  source?: ApiErrorSource;

  constructor(message: string, status: number, presentation?: DisplayMessage, source?: ApiErrorSource) {
    super(message);
    this.name = 'ApiHttpError';
    this.status = status;
    this.presentation = presentation;
    this.source = source;
  }
}

export function shouldRetryApiQuery(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiHttpError && error.status >= 400 && error.status < 500) {
    return false;
  }
  return failureCount < 2;
}
