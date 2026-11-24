'use strict';
// ─── [백그라운드 지연 2026-11-25] 가려진 창에서도 hls.js 타이머를 제때 돌린다 ───
//
// 문제: WebKit 은 가려진 페이지(창 숨김·최소화·다른 창에 완전히 가림 → document.hidden)의 DOM 타이머를 1초 경계로
//   모은다(Tart VM 실측: 250ms 간격 타이머가 매번 ≈1000ms). hls.js 는 모든 일정을 `self.setTimeout`·`self.setInterval`
//   로 짠다 — 100ms 틱, LL 파트 블로킹 재로드 예약, `tickImmediate`(setTimeout 0). 이것들이 1초씩 밀리면 ≈1초짜리
//   LL 파트를 제때 못 받아 끊기고, 끊길 때마다 지연이 늘었다. 그래서 종전에는 가려진 창에서 LL 을 끄고 목표를
//   "세그먼트 + 1초"로 올리는 안정 우선으로 돌았다(앞으로 돌아오면 웹 자리로 점프).
//
// 해결: 이 페이지(플레이어 iframe)의 타이머를 모두 이 모듈이 소유한다. 평소에는 원래 타이머에 그대로 맡기고,
//   **가려진 채 재생 중일 때만**(`setActive(true)`, player.html 의 bgTimersReconcile 이 정한다) 전용 Worker 의
//   타이머로 돌린다 — Worker 의 타이머는 문서 가림 정렬을 받지 않고, 그 알림(postMessage)도 정렬되지 않는다.
//   모드가 바뀌면 걸려 있는 타이머(시작 때 만든 hls.js 100ms 틱 같은 setInterval 포함)를 남은 시간 그대로 옮긴다.
//   requestAnimationFrame·화면 갱신은 건드리지 않는다 — 가려진 동안 그리기는 계속 멈춰 있다.
//
// 비용: 가려진 재생 중에만 Worker 하나(스레드 1개)를 두고, 그 밖에는 없앤다. 타이머 수는 보이는 창과 같다.
// Worker 를 만들 수 없으면(CSP·메모리) `available()` 이 거짓이 되고 player.html 은 종전 안정 우선으로 돌아간다.
(function () {
    if (window.__cviewBgTimers) return;
    const nativeSetTimeout = window.setTimeout;
    const nativeClearTimeout = window.clearTimeout;
    // 반복 타이머도 한 번짜리를 다시 거는 식으로 돌린다 — 모드를 바꿀 때 옮기기 쉽다. 원래 setInterval 은
    // 문자열 코드 타이머(쓰지 않음)에만 남긴다.
    const nativeSetInterval = window.setInterval;
    const nativeClearInterval = window.clearInterval;
    const WORKER_SOURCE =
        'const t=new Map();' +
        'onmessage=function(e){const d=e.data;' +
        'if(d[0]===1){const k=d[1];t.set(k,setTimeout(function(){t.delete(k);postMessage(k);},d[2]));}' +
        'else{const h=t.get(d[1]);if(h!==undefined){clearTimeout(h);t.delete(d[1]);}}};';
    // 원래 타이머 번호와 겹치지 않게 큰 수부터 센다(원래 번호는 1부터 작게 오른다).
    let seq = 0x40000000;
    let active = false;
    let worker = null;
    let workerBroken = false;
    const timers = new Map();   // id → { fn, args, delay, repeat, due, h(원래 타이머) | key(Worker) }
    const byKey = new Map();    // Worker 키 → id
    let keySeq = 0;

    function now() { return performance.now(); }

    function ensureWorker() {
        if (worker || workerBroken) return worker;
        try {
            const url = URL.createObjectURL(new Blob([WORKER_SOURCE], { type: 'text/javascript' }));
            worker = new Worker(url);
            URL.revokeObjectURL(url);
            worker.onmessage = onWorkerFire;
            worker.onerror = function () { workerBroken = true; dropWorker(); };
        } catch (e) {
            workerBroken = true;
            worker = null;
        }
        return worker;
    }
    function dropWorker() {
        const w = worker;
        worker = null;
        if (w) { try { w.terminate(); } catch (e) {} }
        // 걸려 있던 Worker 타이머는 원래 타이머로 다시 건다.
        rearmAll();
    }

    function cancel(entry) {
        if (entry.h !== undefined) { nativeClearTimeout.call(window, entry.h); entry.h = undefined; }
        if (entry.key !== undefined) {
            byKey.delete(entry.key);
            if (worker) worker.postMessage([0, entry.key]);
            entry.key = undefined;
        }
    }
    function schedule(id, entry) {
        const wait = Math.max(0, entry.due - now());
        if (active && ensureWorker()) {
            const key = ++keySeq;
            entry.key = key;
            byKey.set(key, id);
            worker.postMessage([1, key, wait]);
        } else {
            entry.h = nativeSetTimeout.call(window, function () { entry.h = undefined; fire(id); }, wait);
        }
    }
    function onWorkerFire(e) {
        const id = byKey.get(e.data);
        if (id === undefined) return;   // 이미 지웠거나 원래 타이머로 옮긴 것
        byKey.delete(e.data);
        const entry = timers.get(id);
        if (!entry) return;
        entry.key = undefined;
        fire(id);
    }
    function fire(id) {
        const entry = timers.get(id);
        if (!entry) return;
        if (entry.repeat) {
            // 원래 setInterval 처럼 예정 시각 기준으로 다음을 건다 — 늦었으면 지금부터 한 주기 뒤.
            entry.due += entry.delay;
            if (entry.due < now()) entry.due = now() + entry.delay;
            schedule(id, entry);
        } else {
            timers.delete(id);
        }
        // 예외는 원래 타이머처럼 이 작업 밖으로 그대로 나가게 둔다(장부는 이미 정리했다).
        entry.fn.apply(window, entry.args);
    }
    function add(fn, delay, args, repeat) {
        // 문자열 코드 타이머는 쓰지 않는다 — 원래 타이머에 맡긴다.
        if (typeof fn !== 'function') return null;
        let d = Number(delay);
        if (!(d > 0)) d = 0;
        if (repeat && d < 1) d = 1;
        const id = ++seq;
        const entry = { fn: fn, args: args, delay: d, repeat: repeat, due: now() + d, h: undefined, key: undefined };
        timers.set(id, entry);
        schedule(id, entry);
        return id;
    }
    function clear(id) {
        const entry = timers.get(id);
        if (entry) {
            cancel(entry);
            timers.delete(id);
            return;
        }
        // 이 모듈 밖에서 만든(설치 전) 원래 타이머 — 원래 함수로 지운다.
        if (id !== undefined && id !== null) {
            nativeClearTimeout.call(window, id);
            nativeClearInterval.call(window, id);
        }
    }
    function rearmAll() {
        timers.forEach(function (entry, id) { cancel(entry); schedule(id, entry); });
    }

    window.setTimeout = function (fn, delay) {
        const id = add(fn, delay, Array.prototype.slice.call(arguments, 2), false);
        return id === null ? nativeSetTimeout.apply(window, arguments) : id;
    };
    window.setInterval = function (fn, delay) {
        const id = add(fn, delay, Array.prototype.slice.call(arguments, 2), true);
        return id === null ? nativeSetInterval.apply(window, arguments) : id;
    };
    window.clearTimeout = clear;
    window.clearInterval = clear;

    window.__cviewBgTimers = {
        // 가려진 재생 중 정밀 모드를 켜고 끈다. 끄면 Worker 를 없애 스레드·메모리를 돌려준다.
        setActive: function (on) {
            on = !!on && !workerBroken;
            if (on === active) return active;
            active = on;
            if (on) {
                if (!ensureWorker()) { active = false; return false; }
                rearmAll();
            } else {
                rearmAll();
                if (worker) { const w = worker; worker = null; try { w.terminate(); } catch (e) {} }
            }
            return active;
        },
        active: function () { return active; },
        // Worker 를 쓸 수 있는가(한 번 실패하면 이 문서에서는 다시 시도하지 않는다).
        available: function () { return !workerBroken; },
        pending: function () { return timers.size; },
    };
})();
