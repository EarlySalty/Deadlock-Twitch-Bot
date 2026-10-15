import assert from 'node:assert/strict';
import test from 'node:test';
import { dashboardDestination, navigateDashboard } from '../src/navigation';

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
