// Run from repository root: node --experimental-vm-modules docs/verification/2026-10-27/frontend-behavior.mjs
// Executes production modules with deterministic DOM, clock and IPC doubles.
// These checks do not replace WebKit rendering or real-server playback QA.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

function eventTarget() {
  const handlers = new Map();
  return {
    addEventListener(name, fn) { const xs = handlers.get(name) || []; xs.push(fn); handlers.set(name, xs); },
    fire(name, event = {}) { for (const fn of handlers.get(name) || []) fn(event); },
  };
}

async function fixture(search = '') {
  let now = 10000, serial = 0;
  const timers = new Map(), calls = [];
  const document = Object.assign(eventTarget(), { hidden: false });
  const window = eventTarget();
  const context = vm.createContext({
    document, window, location: { search }, URLSearchParams,
    performance: { now: () => now }, WeakRef,
    requestAnimationFrame: (fn) => fn(),
    setTimeout: (fn, ms) => { const id = ++serial; timers.set(id, { fn, at: now + ms }); return id; },
    clearTimeout: (id) => timers.delete(id),
    localStorage: { getItem: () => null },
  });
  const api = {
    chatConnect: (...args) => { calls.push(['connect', ...args]); return Promise.resolve(); },
    chatDisconnect: (...args) => { calls.push(['disconnect', ...args]); return Promise.resolve(); },
  };
  const stubs = {
    './api.js': { api, listen() {} }, './dom.js': { esc: String },
    './multichat.js': { mergedLinesHtml: (messages) => messages.map(m => m.text).join('|') },
    './chat-extras.js': { applyChatSkin() {} }, './chat-prefs.js': { onChatPrefs() {} },
    './ml-pane-controls.js': { bindMenuKeys() {}, focusFirstMenuItem() {} },
  };
  const follower = new vm.SourceTextModule(fs.readFileSync('app-ui/js/chat-follow.js', 'utf8'), { context });
  await follower.link(() => { throw new Error('Unexpected follower import'); });
  await follower.evaluate();
  const multi = new vm.SourceTextModule(fs.readFileSync('app-ui/js/ml-multichat.js', 'utf8'), { context });
  await multi.link((name) => {
    if (name === './chat-follow.js') return follower;
    const values = stubs[name]; assert.ok(values, name);
    return new vm.SyntheticModule(Object.keys(values), function () {
      for (const [k, v] of Object.entries(values)) this.setExport(k, v);
    }, { context });
  });
  await multi.evaluate();
  const list = () => {
    let top = 400;
    const element = Object.assign(eventTarget(), {
      scrollHeight: 500, clientHeight: 100, isConnected: true, children: [], html: [],
      querySelector: () => null,
      insertAdjacentHTML(_where, html) { this.html.push(html); this.children.push({}); this.scrollHeight += 20; },
    });
    Object.defineProperty(element, 'scrollTop', {
      get() { return top; }, set(v) { top = Math.max(0, Math.min(v, this.scrollHeight - this.clientHeight)); },
    });
    return element;
  };
  const panel = () => {
    const m = Object.create(multi.namespace.MultiLiveMergedChat.prototype);
    Object.assign(m, { mode: 'grid', conns: new Map(), list: list() });
    const c = { key: 'mlx:a', chatChannelId: 'chat-a', channelId: 'channel-a', name: 'A', hue: 20, connectAt: now, list: list(), cell: { remove() {} } };
    m.conns.set('a', c);
    return { m, c, send: event => m.onEvent({ session: 'mlx:a', event }) };
  };
  return {
    ...follower.namespace, document, window, list, panel, timers, calls,
    advance(ms) { now += ms; for (const [id, t] of [...timers]) if (t.at <= now) { timers.delete(id); t.fn(); } },
  };
}

const tests = [];
const test = (name, fn) => tests.push([name, fn]);
test('new messages keep a following list at the bottom', async () => {
  const f = await fixture(), l = f.list();
  f.followAppend(l, () => { l.scrollHeight += 200; });
  assert.equal(l.scrollTop, 600);
});
test('user scroll preserves the reading position before the resume deadline', async () => {
  const f = await fixture(), l = f.list(); f.chatFollower(l);
  l.fire('wheel'); l.scrollTop = 50; l.fire('scroll');
  f.advance(7900); assert.equal(f.followAppend(l, () => { l.scrollHeight += 20; }, { resumeMs: 8000 }), false);
  assert.equal(l.scrollTop, 50);
});
test('idle multichat resumes following after eight seconds', async () => {
  const f = await fixture(), l = f.list(); f.chatFollower(l);
  l.fire('wheel'); l.scrollTop = 50; l.fire('scroll'); f.advance(8100);
  assert.equal(f.followAppend(l, () => { l.scrollHeight += 20; }, { resumeMs: 8000 }), true);
  assert.equal(l.scrollTop, 420);
});
test('holding the scrollbar suppresses automatic resumption', async () => {
  const f = await fixture(), l = f.list(); f.chatFollower(l);
  l.fire('pointerdown'); l.scrollTop = 50; l.fire('scroll'); f.advance(9000);
  assert.equal(f.followAppend(l, () => {}, { resumeMs: 8000 }), false);
  f.window.fire('pointerup'); f.advance(8100);
  assert.equal(f.followAppend(l, () => {}, { resumeMs: 8000 }), true);
});
test('background geometry drift resynchronizes when the page becomes visible', async () => {
  const f = await fixture(), l = f.list(); f.chatFollower(l);
  f.document.hidden = true; l.scrollHeight += 200; f.document.hidden = false;
  f.document.fire('visibilitychange'); assert.equal(l.scrollTop, 600);
});
test('disabled ordinary-chat autoscroll is respected during focus resync', async () => {
  const f = await fixture(), l = f.list(); f.chatFollower(l, () => false);
  l.scrollHeight += 200; f.window.fire('focus'); assert.equal(l.scrollTop, 400);
});
test('a disconnected pane reconnects only once after the grace period', async () => {
  const f = await fixture(), { send } = f.panel(); f.advance(5000);
  send({ type: 'state', state: 'disconnected' }); send({ type: 'state', state: 'failed' });
  assert.equal(f.timers.size, 1); f.advance(4000); assert.equal(f.calls.length, 1);
});
test('messages arriving before retry cancel redundant reconnection', async () => {
  const f = await fixture(), { send, c } = f.panel(); f.advance(5000);
  send({ type: 'state', state: 'disconnected' });
  send({ type: 'messages', messages: [{ text: 'live' }] }); f.advance(5000);
  assert.equal(f.calls.length, 0); assert.deepEqual(c.list.html, ['live']);
});
test('late messages from a previous chat channel are discarded', async () => {
  const f = await fixture(), { send, c } = f.panel();
  send({ type: 'messages', chatChannelId: 'previous', messages: [{ text: 'stale' }] });
  assert.equal(c.list.html.length, 0);
});
test('dropping a pane cancels retry and disconnects its session', async () => {
  const f = await fixture(), { m, c, send } = f.panel(); f.advance(5000);
  send({ type: 'state', state: 'failed' }); m.drop('a', c); f.advance(5000);
  assert.deepEqual(f.calls, [['disconnect', 'mlx:a']]);
});
test('reconnection skips replayed history and accepts subsequent live messages', async () => {
  const f = await fixture(), { send, c } = f.panel(); f.advance(5000);
  send({ type: 'state', state: 'disconnected' }); f.advance(4000);
  send({ type: 'messages', recent: true, messages: [{ text: 'duplicate' }] });
  send({ type: 'messages', recent: false, messages: [{ text: 'fresh' }] });
  assert.deepEqual(c.list.html, ['fresh']);
});
test('auxiliary windows do not acquire main-window multichat sessions', async () => {
  const f = await fixture('?w=chat'), { m } = f.panel(); m.conns.clear();
  m.getState = () => ({ sessions: [{ id: 'a', chatChannelId: 'chat-a', channelId: 'channel-a' }] });
  m.getPanes = () => new Map(); m.layoutCells = () => {};
  m.sync(); assert.equal(f.calls.length, 0); assert.equal(m.conns.size, 0);
});

let failed = 0;
for (const [name, fn] of tests) {
  try { await fn(); console.log(`PASS ${name}`); }
  catch (e) { failed++; console.error(`FAIL ${name}\n${e.stack}`); }
}
console.log(JSON.stringify({ passed: tests.length - failed, failed, total: tests.length }));
process.exitCode = failed ? 1 : 0;
