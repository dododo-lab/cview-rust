import assert from 'node:assert/strict';
import test from 'node:test';

// These modules only need the Tauri global at import time. No browser renderer is mocked.
globalThis.window = {};
const { MediaPage } = await import('../js/media.js');
const { api } = await import('../js/api.js');

function page() {
  const instance = Object.create(MediaPage.prototype);
  instance.seq = 0;
  instance.info = null;
  instance.saveTimer = 0;
  instance.body = {
    innerHTML: '',
    querySelector: () => null,
  };
  return instance;
}

test('leaving a pending VOD request prevents hidden playback and a save timer', async () => {
  let resolve;
  const original = api.vodPlay;
  api.vodPlay = () => new Promise((done) => { resolve = done; });
  const view = page();
  try {
    const pending = view.show({ kind: 'vod', id: 1 });
    const loading = view.body.innerHTML;
    view.leave();
    resolve({ title: 'late response', qualities: [], streamUrl: 'https://example.com/video' });
    await pending;
    assert.equal(view.body.innerHTML, loading);
    assert.equal(view.info, null);
    assert.equal(view.saveTimer, 0);
  } finally {
    api.vodPlay = original;
    view.leave();
  }
});

test('leaving a pending clip request prevents an embedded player from appearing', async () => {
  let resolve;
  const original = api.clipPlay;
  api.clipPlay = () => new Promise((done) => { resolve = done; });
  const view = page();
  try {
    const pending = view.show({ kind: 'clip', id: 'clip' });
    const loading = view.body.innerHTML;
    view.leave();
    resolve({ detail: { clipTitle: 'late clip' } });
    await pending;
    assert.equal(view.body.innerHTML, loading);
    assert.equal(view.info, null);
  } finally {
    api.clipPlay = original;
  }
});

test('a failure received after leaving does not replace the page', async () => {
  let reject;
  const original = api.vodPlay;
  api.vodPlay = () => new Promise((_, fail) => { reject = fail; });
  const view = page();
  try {
    const pending = view.show({ kind: 'vod', id: 2 });
    view.leave();
    view.body.innerHTML = 'next page';
    reject(new Error('late failure'));
    await pending;
    assert.equal(view.body.innerHTML, 'next page');
  } finally {
    api.vodPlay = original;
  }
});

test('leaving an active clip unloads the remote iframe', () => {
  const view = page();
  const attributes = new Map([['src', 'https://chzzk.naver.com/embed/clip/a']]);
  view.body.querySelector = (selector) => selector === '.clip-embed'
    ? { removeAttribute: (name) => attributes.delete(name) } : null;
  view.info = { clipUid: 'a' };
  view.leave();
  assert.equal(attributes.has('src'), false);
  assert.equal(view.info, null);
});

test('leaving a VOD saves its position before unloading the video', () => {
  const view = page();
  const calls = [];
  const video = {
    currentTime: 42, duration: 100,
    pause: () => calls.push('pause'),
    removeAttribute: (name) => calls.push(`remove:${name}`),
    load: () => calls.push('load'),
  };
  const original = api.vodProgressSave;
  api.vodProgressSave = (...args) => { calls.push(args); return Promise.resolve(); };
  view.kind = 'vod';
  view.info = { videoNo: 7, title: 'title', channelName: 'channel', duration: 100 };
  view.body.querySelector = (selector) => selector === 'video' ? video : null;
  try {
    view.leave();
    assert.deepEqual(calls, [[7, 42, 100, 'title', 'channel', null], 'pause', 'remove:src', 'load']);
    assert.equal(view.info, null);
  } finally {
    api.vodProgressSave = original;
  }
});
