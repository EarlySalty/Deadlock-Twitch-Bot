import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { dashboardDestination, navigateDashboard, subscribeNavigation } from '../src/navigation';

test('dashboard navigation retains the document only for owned unmodified links', () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const originalDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const pushed: unknown[] = [];
  const dispatched: unknown[] = [];
  let dirty = false;
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    location: { href: 'https://dashboard.example/twitch/dashboard', origin: 'https://dashboard.example' },
    history: { pushState: (...args: unknown[]) => pushed.push(args) },
    dispatchEvent: (event: unknown) => dispatched.push(event),
    scrollTo: () => undefined,
  } });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    querySelector: () => dirty ? {} : null,
  } });
  const originalPopState = Object.getOwnPropertyDescriptor(globalThis, 'PopStateEvent');
  Object.defineProperty(globalThis, 'PopStateEvent', { configurable: true, value: Event });
  const click = (href: string, extra: object = {}) => {
    let prevented = false;
    navigateDashboard({
      button: 0, defaultPrevented: false,
      currentTarget: { href, target: '', hasAttribute: () => false },
      preventDefault: () => { prevented = true; },
      ...extra,
    } as Parameters<typeof navigateDashboard>[0]);
    return prevented;
  };
  try {
    assert.equal(click('/twitch/challenges'), true);
    assert.equal(pushed.length, 1);
    assert.equal(dispatched.length, 1);
    assert.equal(click('/twitch/analyse', { ctrlKey: true }), false);
    assert.equal(click('/twitch/analyse', { button: 1 }), false);
    assert.equal(click('/twitch/auth/logout'), false);
    assert.equal(click('/twitch/dashboard#changelog'), false);
    assert.equal(click('https://external.example/twitch/analyse'), false);
    assert.equal(click('/twitch/analyse', { defaultPrevented: true }), false);
    assert.equal(dashboardDestination('/twitch/api/v2/auth-status'), null);
    dirty = true;
    assert.equal(click('/twitch/analyse'), false);
    assert.equal(pushed.length, 1);
  } finally {
    for (const [key, descriptor] of [['window', originalWindow], ['document', originalDocument], ['PopStateEvent', originalPopState]] as const) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});

test('history navigation retains unsaved editors and pending saves unless discard is approved', () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const originalDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const location = { href: 'https://dashboard.example/twitch/overlay' };
  const handlers = new Map<string, () => void>();
  let dirty = false;
  let saving = false;
  let editorBlocksUnload = false;
  let approved = false;
  let notifications = 0;
  let confirmations = 0;
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    location,
    history: { pushState: (_state: unknown, _title: string, url: string) => { location.href = url; } },
    addEventListener: (name: string, handler: () => void) => handlers.set(name, handler),
    removeEventListener: (name: string) => handlers.delete(name),
    dispatchEvent: (event: Event) => { if (editorBlocksUnload) event.preventDefault(); },
  } });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    querySelector: (selector: string) => selector.includes('data-saving') ? saving : dirty,
  } });
  try {
    const unsubscribe = subscribeNavigation(() => { notifications += 1; }, () => { confirmations += 1; return approved; });
    const navigateBack = () => { location.href = 'https://dashboard.example/twitch/dashboard'; handlers.get('popstate')?.(); };
    dirty = true;
    navigateBack();
    assert.equal(notifications, 0);
    assert.equal(location.href, 'https://dashboard.example/twitch/overlay');
    assert.equal(confirmations, 1);
    dirty = false;
    editorBlocksUnload = true;
    navigateBack();
    assert.equal(notifications, 0);
    assert.equal(location.href, 'https://dashboard.example/twitch/overlay');
    approved = true;
    saving = true;
    navigateBack();
    assert.equal(notifications, 0);
    assert.equal(confirmations, 2);
    saving = false;
    navigateBack();
    assert.equal(notifications, 1);
    assert.equal(location.href, 'https://dashboard.example/twitch/dashboard');
    editorBlocksUnload = false;
    navigateBack();
    assert.equal(notifications, 2);
    assert.equal(confirmations, 3);
    unsubscribe();
    assert.equal(handlers.size, 0);
  } finally {
    for (const [key, descriptor] of [['window', originalWindow], ['document', originalDocument]] as const) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});

test('root error recovery reloads the document to clear rejected lazy imports', () => {
  const app = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
  assert.match(app, /onRetry=\{\(\) => window\.location\.reload\(\)\}/);
});
