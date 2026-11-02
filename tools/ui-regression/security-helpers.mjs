// 보안 회귀 검사 — 브라우저 없이 Node 로. 채팅 이모티콘 치환(dom.js renderContent·emojiUrl, chat.js isEmoteOnly)과
// 서명 없는 업데이트 피드 경고(update-check.js updateDetailHtml).
// 저장소 루트에서: node tools/ui-regression/security-helpers.mjs
import assert from 'node:assert/strict';

const { renderContent, emojiUrl, esc } = await import('../../app-ui/js/dom.js');

// 네이버 이미지 서버만 이모티콘 주소로 받는다.
const ok = 'https://nng-phinf.pstatic.net/glive/emoji/a.png';
assert.equal(emojiUrl(ok), ok);
assert.equal(emojiUrl('https://ssl.pstatic.net/static/x.gif'), 'https://ssl.pstatic.net/static/x.gif');
for (const bad of [
  'https://evil.example/p.png?id=1', // 추적 픽셀
  'https://pstatic.net.evil.example/a.png',
  'https://evilpstatic.net/a.png',
  'http://nng-phinf.pstatic.net/a.png', // 평문
  'javascript:alert(1)',
  'data:image/png;base64,AAAA',
  '//nng-phinf.pstatic.net/a.png',
  '',
  undefined,
]) assert.equal(emojiUrl(bad), '', String(bad));

// 허용 호스트는 그림으로, 그 밖은 원문 텍스트로(이스케이프).
assert.match(renderContent('a{:x:}b', { x: ok }), /^a<img [^>]*src="https:\/\/nng-phinf\.pstatic\.net\/glive\/emoji\/a\.png"[^>]*>b$/);
assert.equal(renderContent('{:x:}', { x: 'https://evil.example/p.png' }), esc('{:x:}'));
// 상속 속성(__proto__·constructor)은 이모티콘 표 항목이 아니다.
assert.equal(renderContent('{:constructor:}', {}), esc('{:constructor:}'));
assert.equal(renderContent('{:__proto__:}', {}), esc('{:__proto__:}'));
// 이름·본문의 HTML 은 이스케이프된다.
const html = renderContent('<img src=x onerror=alert(1)>{:"><b>:}', { '"><b>': ok });
assert.ok(!html.includes('<img src=x'), html);
assert.ok(html.includes('alt="&quot;&gt;&lt;b&gt;"'), html);

// isEmoteOnly 는 renderContent 와 같은 기준이어야 한다(그림이 안 되는 줄을 이모티콘 줄로 크게 그리지 않게).
// chat.js 는 브라우저 전역을 쓰므로 함수 원문만 꺼내 dom.js 의 emojiUrl 과 묶어 평가한다.
const fs = await import('node:fs');
const src = fs.readFileSync(new URL('../../app-ui/js/chat.js', import.meta.url), 'utf8');
const body = src.match(/export function isEmoteOnly\(content, emojis\) \{[\s\S]*?\n\}/)[0].replace('export ', '');
const isEmoteOnly = new Function('emojiUrl', `${body}; return isEmoteOnly;`)(emojiUrl);
assert.equal(isEmoteOnly('{:x:}', { x: ok }), true);
assert.equal(isEmoteOnly('{:x:}', { x: 'https://evil.example/p.png' }), false);
assert.equal(isEmoteOnly('{:constructor:}', {}), false);

// 업데이트 시트 — 서명을 확인하지 못한 새 버전에만 경고. update-check.js 는 api.js(Tauri 전역)를 불러오므로
// import 줄을 빼고 esc 만 넘겨 평가한다.
const upd = fs.readFileSync(new URL('../../app-ui/js/update-check.js', import.meta.url), 'utf8')
  .replace(/^import .*$/gm, '')
  .replace(/^export /gm, '');
const { updateDetailHtml } = new Function('esc', 'api', 'listen', 'Events', `${upd}; return { updateDetailHtml };`)(esc, {}, () => {}, {});
const base = { current: '0.1.70', latest: '0.1.71', newer: true };
assert.ok(updateDetailHtml({ ...base, signed: false }).includes('update-unsigned'));
assert.ok(!updateDetailHtml({ ...base, signed: true }).includes('update-unsigned'));
assert.ok(!updateDetailHtml({ ...base, newer: false, signed: false }).includes('update-unsigned'));
// 노트는 이스케이프된다.
assert.ok(!updateDetailHtml({ ...base, signed: true, notes: '<img src=x>' }).includes('<img'));

console.log('security-helpers: ok');
