import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { dashboardDestination, navigateDashboard, subscribeNavigation } from '../src/navigation';

test('public tour paths stay inside dashboard navigation', () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      location: { href: 'https://dashboard.example/twitch/rundgang', origin: 'https://dashboard.example' },
    },
  });
  try {
    assert.ok(dashboardDestination('/twitch/rundgang'));
    assert.ok(dashboardDestination('/twitch/rundgang/analyse'));
    assert.ok(dashboardDestination('/twitch/rundgang/social-media'));
    assert.equal(dashboardDestination('/twitch/auth/logout'), null);
  } finally {
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});

test('dashboard navigation retains the document only for owned unmodified links', () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const originalDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const pushed: unknown[] = [];
  const dispatched: unknown[] = [];
  let dirty = false;
  let saving = false;
  let blocksUnload = false;
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    location: { href: 'https://dashboard.example/twitch/dashboard', origin: 'https://dashboard.example' },
    navigation: { currentEntry: { index: 0 } },
    history: { pushState: (...args: unknown[]) => pushed.push(args) },
    dispatchEvent: (event: Event) => {
      dispatched.push(event);
      if (blocksUnload && event.type === 'beforeunload') event.preventDefault();
    },
    confirm: () => false,
    scrollTo: () => undefined,
  } });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    querySelector: (selector: string) => selector.includes('data-saving') ? saving : dirty,
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
    assert.equal(dispatched.length, 2);
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
    dirty = false;
    saving = true;
    assert.equal(click('/twitch/analyse'), true);
    assert.equal(pushed.length, 1);
    saving = false;
    blocksUnload = true;
    assert.equal(click('/twitch/analyse'), true);
    assert.equal(pushed.length, 1);
    blocksUnload = false;
    Reflect.deleteProperty(window, 'navigation');
    assert.equal(click('/twitch/analyse'), false);
    assert.equal(pushed.length, 1);
    const unsubscribe = subscribeNavigation(() => assert.fail('legacy navigation must remain document-owned'));
    unsubscribe();
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
  const entries = ['dashboard', 'challenges', 'overlay'].map(path => ({
    url: `https://dashboard.example/twitch/${path}`,
    state: {} as Record<string, unknown>,
  }));
  const location = { href: entries[2].url };
  let position = 2;
  const pending: (() => void)[] = [];
  const handlers = new Map<string, () => void>();
  const history = {
    get state() { return entries[position].state; },
    replaceState(state: Record<string, unknown>, _title: string, url?: string | URL | null) {
      entries[position].state = state;
      if (url != null) entries[position].url = location.href = String(url);
    },
    pushState(state: Record<string, unknown>, _title: string, url: string) {
      entries.splice(position + 1);
      entries.push({ state, url });
      position += 1;
      location.href = url;
    },
    go(delta: number) {
      pending.push(() => {
        position += delta;
        assert.ok(position >= 0 && position < entries.length);
        location.href = entries[position].url;
        handlers.get('popstate')?.();
      });
    },
  };
  const traverse = (delta: number) => {
    history.go(delta);
    while (pending.length) pending.shift()?.();
  };
  let dirty = false;
  let saving = false;
  let editorBlocksUnload = false;
  let approved = false;
  let notifications = 0;
  let confirmations = 0;
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    location,
    navigation: { get currentEntry() { return { index: position }; } },
    history,
    addEventListener: (name: string, handler: () => void) => handlers.set(name, handler),
    removeEventListener: (name: string) => handlers.delete(name),
    dispatchEvent: (event: Event) => { if (editorBlocksUnload) event.preventDefault(); },
  } });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    querySelector: (selector: string) => selector.includes('data-saving') ? saving : dirty,
  } });
  try {
    const unsubscribe = subscribeNavigation(() => { notifications += 1; }, () => { confirmations += 1; return approved; });
    const originalEntries = structuredClone(entries);
    dirty = true;
    traverse(-1);
    assert.equal(notifications, 0);
    assert.equal(position, 2);
    assert.deepEqual(entries, originalEntries);
    traverse(-2);
    assert.equal(position, 2);
    assert.deepEqual(entries, originalEntries);
    approved = true;
    traverse(-1);
    assert.equal(position, 1);
    assert.equal(notifications, 1);
    approved = false;
    traverse(1);
    assert.equal(position, 1);
    assert.deepEqual(entries, originalEntries);
    dirty = false;
    editorBlocksUnload = true;
    traverse(1);
    assert.equal(position, 1);
    assert.deepEqual(entries, originalEntries);
    const confirmationsBeforeSave = confirmations;
    saving = true;
    traverse(-1);
    assert.equal(position, 1);
    assert.equal(confirmations, confirmationsBeforeSave);
    assert.deepEqual(entries, originalEntries);
    saving = false;
    approved = true;
    traverse(1);
    assert.equal(position, 2);
    assert.equal(notifications, 2);
    history.replaceState({}, '', `${location.href}?tab=profile`);
    assert.deepEqual(history.state, {});
    const replacedEntries = structuredClone(entries);
    approved = false;
    traverse(-2);
    assert.equal(position, 2);
    assert.deepEqual(entries, replacedEntries);
    editorBlocksUnload = false;
    traverse(-2);
    assert.equal(position, 0);
    assert.equal(notifications, 3);
    assert.deepEqual(entries, replacedEntries);
    history.pushState({}, '', `${location.href}#section`);
    handlers.get('popstate')?.();
    assert.deepEqual(history.state, {});
    history.pushState({}, '', 'https://dashboard.example/twitch/overlay');
    handlers.get('dashboard:navigate')?.();
    const fragmentEntries = structuredClone(entries);
    const notificationsBeforeFragment = notifications;
    dirty = true;
    traverse(-1);
    assert.equal(position, 2);
    assert.equal(notifications, notificationsBeforeFragment);
    assert.deepEqual(entries, fragmentEntries);
    dirty = false;
    saving = true;
    traverse(-1);
    assert.equal(position, 2);
    assert.deepEqual(entries, fragmentEntries);
    saving = false;
    traverse(-1);
    assert.equal(position, 1);
    dirty = true;
    history.pushState({}, '', `${location.href}-new`);
    handlers.get('popstate')?.();
    while (pending.length) pending.shift()?.();
    assert.equal(position, 1);
    assert.deepEqual(entries[2].state, {});
    assert.equal(entries.length, 3);
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
