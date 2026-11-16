// 채팅 수명·전송 경쟁 회귀 검사. 모든 전송은 모의 IPC 안에서만 끝나며 실제 채팅방에 글을 보내지 않는다.
// NODE_PATH=<Playwright 경로> node tools/ui-regression/chat-lifecycle.mjs
// CVIEW_UI_BROWSER=webkit 이면 화면 없는 WebKit으로 검사한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const html = fs.readFileSync('app-ui/index.html', 'utf8')
  .replace('<script type="module" src="js/app.js"></script>', '');
const browser = await engine.launch({ headless: true });
const results = [];

async function open({ merged = false } = {}) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(String(error)));
  await page.route(`${base}__chat-lifecycle.html`, (route) => route.fulfill({ contentType: 'text/html', body: html }));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => {
    window.__sent = [];
    window.__pending = [];
    window.__disconnects = [];
    window.__holdSend = false;
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (cmd, args = {}) => {
      if (cmd === 'chat_connect') return { canSend: true };
      if (cmd === 'chat_disconnect') { window.__disconnects.push(args); return null; }
      if (cmd === 'chat_send') {
        window.__sent.push(args);
        if (window.__holdSend) return new Promise((resolve, reject) => window.__pending.push({ resolve, reject }));
        return null;
      }
      if (cmd === 'live_detail') return { channelId: args.channelId, channelName: args.channelId, chatChannelId: `cc-${args.channelId}`, isLive: true };
      if (cmd === 'chat_emoticons' || cmd === 'chat_donation_ranking') return [];
      if (cmd === 'chat_avatars') return args.userIds.map(() => null);
      try { return await invoke(cmd, args); } catch { return null; }
    };
  });
  await page.goto(`${base}__chat-lifecycle.html`);
  await page.evaluate(async (merged) => {
    document.getElementById('splash')?.remove();
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname('검사자');
    const content = document.createElement('div');
    content.style.cssText = 'position:fixed;inset:0;display:flex;z-index:50;background:var(--bg)';
    document.body.append(content);
    window.__say = (id = 'm1', chatChannelId = 'cc-one') => ({ type: 'messages', chatChannelId, messages: [{ id, kind: 'normal', userId: 'user', nickname: '시청자', content: id, timestamp: Date.now(), emojis: {} }] });
    if (merged) {
      const { MultiChatPage } = await import('/js/multichat.js');
      const mc = new MultiChatPage(content, { isLoggedIn: () => true });
      mc.root.hidden = false;
      await mc.add('one');
      await mc.add('two');
      for (const entry of mc.channels) entry.panel.onEvent({ type: 'connected' });
      mc.layout = 'merged';
      mc.render();
      mc.target.value = mc.channels[0].session;
      mc.onTargetChange();
      window.__mc = mc;
    } else {
      const root = document.querySelector('#viewer .chat').cloneNode(true);
      root.hidden = false;
      root.style.cssText = 'display:flex;flex-direction:column;width:380px;height:850px';
      content.append(root);
      const panel = new ChatPanel(root, { isLoggedIn: () => true, session: 'audit' });
      await panel.connect('cc-one', 'one');
      panel.onEvent({ type: 'connected' });
      window.__panel = panel;
    }
  }, merged);
  return { context, page, errors };
}

async function check(name, fn, options) {
  const fixture = await open(options);
  try {
    await fn(fixture.page);
    assert.deepEqual(fixture.errors, [], '페이지 오류가 발생했다');
    results.push({ name, ok: true });
  } catch (error) {
    results.push({ name, ok: false, error: error.message });
  } finally {
    await fixture.context.close();
  }
}

try {
  await check('CL01 전송 대기 중 같은 메시지를 중복 제출하지 않는다', async (page) => {
    const count = await page.evaluate(async () => {
      const p = window.__panel;
      p.input.value = '중복 전송 검사';
      window.__holdSend = true;
      const first = p.send();
      const second = p.send();
      for (const pending of window.__pending) pending.resolve();
      await Promise.all([first, second]);
      return window.__sent.length;
    });
    assert.equal(count, 1);
  });

  await check('CL02 이전 채널 전송 성공이 새 채널의 입력을 지우지 않는다', async (page) => {
    const state = await page.evaluate(async () => {
      const p = window.__panel;
      p.input.value = '이전 채널로 보낸 글';
      window.__holdSend = true;
      const sending = p.send();
      await p.connect('cc-two', 'two');
      p.onEvent({ type: 'connected' });
      p.input.value = '새 채널에서 쓰는 글';
      window.__pending[0].resolve();
      await sending;
      return { draft: p.input.value, history: p.history };
    });
    assert.equal(state.draft, '새 채널에서 쓰는 글');
    assert.deepEqual(state.history, []);
  });

  await check('CL03 이전 채널 전송 실패가 새 채널의 배너와 초점을 바꾸지 않는다', async (page) => {
    const state = await page.evaluate(async () => {
      const p = window.__panel;
      p.input.value = '이전 전송';
      window.__holdSend = true;
      const sending = p.send();
      await p.connect('cc-two', 'two');
      p.onEvent({ type: 'connected' });
      const other = document.createElement('button');
      document.body.append(other);
      other.focus();
      window.__pending[0].reject(new Error('이전 채널 오류'));
      await sending;
      return { banner: p.banner.textContent, focusKept: document.activeElement === other };
    });
    assert.equal(state.banner, '');
    assert.equal(state.focusKept, true);
  });

  await check('CL04 전송 완료가 같은 채널에서 새로 쓴 입력도 지우지 않는다', async (page) => {
    const draft = await page.evaluate(async () => {
      const p = window.__panel;
      p.input.value = '보낸 글';
      window.__holdSend = true;
      const sending = p.send();
      p.input.value = '다음 글';
      window.__pending[0].resolve();
      await sending;
      return p.input.value;
    });
    assert.equal(draft, '다음 글');
  });

  await check('CL05 이용 제한 중에는 입력·보내기·이모티콘을 비활성화한다', async (page) => {
    const state = await page.evaluate(() => {
      const p = window.__panel;
      p.onEvent({ type: 'penalized', userId: 'me', duration: 60 });
      return [p.input.disabled, p.sendButton.disabled, p.emojiButton.disabled];
    });
    assert.deepEqual(state, [true, true, true]);
  });

  await check('CL06 채널을 바꾸면 이전 제한·닉네임 후보를 초기화한다', async (page) => {
    const state = await page.evaluate(async () => {
      const p = window.__panel;
      p.recentNicks.set('이전 채널 사람', Date.now());
      p.onEvent({ type: 'penalized', userId: 'me', duration: 60 });
      await p.connect('cc-two', 'two');
      p.onEvent({ type: 'connected' });
      return { banner: p.banner.textContent, enabled: !p.input.disabled, oldNick: p.recentNicks.has('이전 채널 사람') };
    });
    assert.deepEqual(state, { banner: '', enabled: true, oldNick: false });
  });

  await check('CL07 채팅 채널 정보가 없으면 이전 연결을 끊고 메시지를 무시한다', async (page) => {
    const state = await page.evaluate(async () => {
      const p = window.__panel;
      await p.connect(null, 'two');
      p.onEvent(window.__say('이전 채널 메시지'));
      p.flushLines();
      return { disconnects: window.__disconnects.length, leaked: p.list.textContent.includes('이전 채널 메시지') };
    });
    assert.equal(state.disconnects, 1);
    assert.equal(state.leaked, false);
  });

  await check('CL08 전송 요청에 보낼 채팅 채널을 명시한다', async (page) => {
    const channel = await page.evaluate(async () => {
      window.__panel.input.value = '대상 확인';
      await window.__panel.send();
      return window.__sent[0].chatChannelId;
    });
    assert.equal(channel, 'cc-one');
  });

  await check('CL09 패널을 닫으면 대기 중인 메시지 묶음과 후크도 취소한다', async (page) => {
    const hooks = await page.evaluate(async () => {
      const p = window.__panel;
      let calls = 0;
      p.appendHooks.push(() => { calls += 1; });
      p.onEvent(window.__say('닫은 뒤 그리면 안 되는 글'));
      await p.destroy();
      p.root.remove();
      await new Promise((resolve) => setTimeout(resolve, 300));
      return calls;
    });
    assert.equal(hooks, 0);
  });

  await check('CL10 통합 채팅도 전송 대기 중 중복 제출을 막는다', async (page) => {
    const count = await page.evaluate(async () => {
      const mc = window.__mc;
      mc.mergedInput.value = '통합 중복 검사';
      window.__holdSend = true;
      const first = mc.sendMerged();
      const second = mc.sendMerged();
      for (const pending of window.__pending) pending.resolve();
      await Promise.all([first, second]);
      return window.__sent.length;
    });
    assert.equal(count, 1);
  }, { merged: true });

  await check('CL11 통합 채팅 대상 전환 후 이전 전송이 새 입력을 지우지 않는다', async (page) => {
    const state = await page.evaluate(async () => {
      const mc = window.__mc;
      mc.mergedInput.value = '첫 채널 글';
      window.__holdSend = true;
      const sending = mc.sendMerged();
      mc.target.value = mc.channels[1].session;
      mc.onTargetChange();
      mc.mergedInput.value = '둘째 채널에서 쓰는 글';
      window.__pending[0].resolve();
      await sending;
      return { draft: mc.mergedInput.value, target: window.__sent[0].chatChannelId };
    });
    assert.deepEqual(state, { draft: '둘째 채널에서 쓰는 글', target: 'cc-one' });
  }, { merged: true });

  await check('CL12 통합 입력도 선택 채널의 이용 제한을 따른다', async (page) => {
    const disabled = await page.evaluate(() => {
      const mc = window.__mc;
      mc.targetEntry().panel.onEvent({ type: 'penalized', userId: 'me', duration: 60 });
      return mc.mergedInput.disabled;
    });
    assert.equal(disabled, true);
  }, { merged: true });

  await check('CL13 퇴장 후 전송을 막고 새 연결 승인 후에는 입력을 복구한다', async (page) => {
    const state = await page.evaluate(() => {
      const p = window.__panel;
      p.onEvent({ type: 'kicked' });
      const disabled = p.input.disabled;
      p.onEvent({ type: 'connected' });
      return { disabled, resumed: !p.input.disabled };
    });
    assert.deepEqual(state, { disabled: true, resumed: true });
  });

  await check('CL14 통합 목록도 재접속 전 채팅방의 메시지와 삭제를 무시한다', async (page) => {
    const state = await page.evaluate(async () => {
      const mc = window.__mc;
      const entry = mc.channels[0];
      await entry.panel.connect('cc-current', 'one');
      entry.panel.onEvent({ type: 'connected' });
      mc.onSessionEvent({ session: entry.session, event: window.__say('이전 방의 글', 'cc-one') });
      mc.onSessionEvent({ session: entry.session, event: window.__say('현재 방의 글', 'cc-current') });
      mc.flushBacklog();
      mc.onSessionEvent({ session: entry.session, event: { type: 'blinded', messageId: '현재 방의 글', chatChannelId: 'cc-one' } });
      return { old: mc.mergedList.textContent.includes('이전 방의 글'), current: mc.mergedList.textContent.includes('현재 방의 글') };
    });
    assert.deepEqual(state, { old: false, current: true });
  }, { merged: true });

  await check('CL15 관리자 삭제는 멘션함과 고정한 복제본에도 반영된다', async (page) => {
    const state = await page.evaluate(() => {
      const p = window.__panel;
      const event = window.__say('지울 글');
      event.messages[0].content = '@검사자 삭제할 원문';
      p.onEvent(event);
      p.flushLines();
      p.pin(p.list.querySelector('.chat-line[data-id="지울 글"]'));
      p.toggleMentionTray();
      p.onEvent({ type: 'blinded', messageId: '지울 글', chatChannelId: 'cc-one' });
      return { count: p.mentions.length, stored: p.mentions[0]?.content, tray: p.tray.textContent, pin: p.pinBar.textContent };
    });
    assert.equal(state.count, 1);
    assert.equal(state.stored, '관리자에 의해 삭제된 메시지입니다.');
    assert.ok(!state.tray.includes('삭제할 원문') && !state.pin.includes('삭제할 원문'));
    assert.ok(state.tray.includes('삭제된 메시지') && state.pin.includes('삭제된 메시지'));
  });

  await check('CL16 메시지가 많아져도 보조 멘션·이모티콘 목록은 상한을 지킨다', async (page) => {
    const state = await page.evaluate(() => {
      const p = window.__panel;
      const messages = Array.from({ length: 1200 }, (_, i) => ({ id: `many-${i}`, kind: 'normal', userId: 'user', nickname: '시청자', content: `@검사자 ${i}`, timestamp: Date.now(), emojis: { [`emote-${i}`]: 'https://ssl.pstatic.net/static/nng/glive/icon/a.png' } }));
      p.append(messages);
      return { mentions: p.mentions.length, emojis: p.seenEmoji.size };
    });
    assert.ok(state.mentions > 0 && state.mentions <= 300, `멘션 ${state.mentions}개`);
    assert.ok(state.emojis > 0 && state.emojis <= 300, `이모티콘 ${state.emojis}개`);
  });
} finally {
  await browser.close();
  server.close();
}

for (const result of results) console.log(`${result.ok ? '통과' : '실패'} ${result.name}${result.ok ? '' : ` — ${result.error}`}`);
const failed = results.filter((result) => !result.ok).length;
console.log(`${results.length - failed}/${results.length} 통과`);
process.exitCode = failed ? 1 : 0;
